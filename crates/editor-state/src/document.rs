#[derive(Debug)]
pub struct Document {
    pub text_buffer: editor_core::text::TextBuffer,
    pub history: editor_core::history::History,
    pub cursor: editor_core::cursor::Cursor,

    /// Prevents undo/redo operations from being recorded as new edits
    is_recording: bool,
}

impl Document {
    pub fn new(text_buffer: editor_core::text::TextBuffer) -> Self {
        Self {
            text_buffer,
            history: editor_core::history::History {
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
            },
            cursor: editor_core::cursor::Cursor::default(),
            is_recording: true,
        }
    }
}

impl Document {
    /// Inserts text at the cursor. If text is selected, it replaces the selection.
    /// Structured to accommodate future bottom-to-top multi-cursor iteration.
    pub fn insert(&mut self, text: &str) {
        let cursor_before = self.cursor;

        // 1. Identify the range and the text being replaced (if any)
        // We do this before the buffer is modified.
        let selection_text = self
            .text_buffer
            .get_cursor_selection(&self.cursor)
            .expect("Unhandled error for now");
        let (range_start, range_end) = self.cursor.range();

        // 2. Perform the Buffer Operation
        // Whether it's a replacement or a simple insertion, TextBuffer::insert
        // now handles the deletion of the selection internally and returns the final position.
        let end_pos = self
            .text_buffer
            .insert(&self.cursor, text)
            .expect("Buffer insertion failed");

        let cursor_after = editor_core::cursor::Cursor::new(end_pos.row, end_pos.col);

        // 3. Record to History
        if self.is_recording {
            if let Some(deleted_text) = selection_text {
                // Scenario: Replacement
                self.history.record_replace(
                    range_start,
                    range_end,
                    &deleted_text,
                    text,
                    cursor_before,
                    cursor_after,
                );
            } else {
                // Scenario: Standard Insertion
                self.history
                    .record_insert(
                        range_start, // For no selection, range_start is just cursor.head
                        text,
                        cursor_before,
                        cursor_after,
                    )
                    .expect("History batching failed");
            }
        }

        // 4. Update Document state
        self.cursor = cursor_after;
    }

    /// Deletes text based on the cursor state (selection, backspace, or forward delete).
    /// `is_backspace` determines if we delete behind the cursor when no selection exists.
    pub fn delete(&mut self, is_backspace: bool) {
        let cursor_before = self.cursor;

        // 2. Perform the Buffer Operation
        let (new_pos, deleted_text) = if is_backspace {
            self.text_buffer.backspace(&self.cursor).expect("")
        } else {
            self.text_buffer.delete_forward(&self.cursor).expect("")
        };
        let cursor_after = editor_core::cursor::Cursor::new(new_pos.row, new_pos.col);

        // 3. Record to History
        if self.is_recording && !deleted_text.is_empty() {
            // Determine the bounding box of what was actually removed.
            // If it was a selection, we use the selection's range.
            // If it was a single char delete/backspace, we use the before/after positions.
            let (start, end) = if !cursor_before.no_selection() {
                cursor_before.range()
            } else if new_pos < cursor_before.head {
                (new_pos, cursor_before.head)
            } else {
                (cursor_before.head, new_pos)
            };

            self.history
                .record_delete(start, end, &deleted_text, cursor_before, cursor_after)
                .expect("History batching failed");
        }

        self.cursor = cursor_after;
    }
}

impl Document {
    pub fn undo(&mut self) {
        if let Some(transaction) = self.history.undo() {
            self.execute_transaction(transaction, true);
        }
    }

    pub fn redo(&mut self) {
        if let Some(transaction) = self.history.redo() {
            self.execute_transaction(transaction, false);
        }
    }

    /// Internal helper to play back a transaction without recording it.
    fn execute_transaction(
        &mut self,
        transaction: editor_core::history::Transaction,
        is_undo: bool,
    ) {
        self.is_recording = false;

        // When undoing, we apply actions in reverse order (stack logic).
        // When redoing, we apply them in the original forward order.
        let actions: Vec<_> = if is_undo {
            transaction.actions.iter().rev().collect()
        } else {
            transaction.actions.iter().collect()
        };

        for action in actions {
            match action {
                editor_core::enums::EditAction::Insert { pos, text } => {
                    if is_undo {
                        // Undo Insert -> Delete the text we added
                        let end_pos = self.calculate_end_position(*pos, text);
                        let temp_cursor = editor_core::cursor::Cursor::new_selection(*pos, end_pos);
                        println!(
                            "{:#?} {:#?} {:#?} {}",
                            end_pos,
                            temp_cursor.head,
                            temp_cursor.anchor,
                            temp_cursor.no_selection()
                        );
                        let _ = self.text_buffer.delete_selection(&temp_cursor);
                    } else {
                        // Redo Insert -> Re-insert the text
                        let temp_cursor = editor_core::cursor::Cursor::new(pos.row, pos.col);
                        let _ = self.text_buffer.insert(&temp_cursor, text);
                    }
                }
                editor_core::enums::EditAction::Delete {
                    pos: start, text, ..
                } => {
                    if is_undo {
                        // Undo Delete -> Put the deleted text back
                        let temp_cursor = editor_core::cursor::Cursor::new(start.row, start.col);
                        let _ = self.text_buffer.insert(&temp_cursor, text);
                    } else {
                        // Redo Delete -> Delete the text again
                        // Note: EditAction::Delete stores start/end, so we use them
                        let temp_cursor = editor_core::cursor::Cursor::new_selection(
                            *start,
                            self.calculate_end_position(*start, text),
                        );
                        let _ = self.text_buffer.delete_selection(&temp_cursor);
                    }
                }
            }
        }

        // Restore the appropriate cursor state
        self.cursor = if is_undo {
            transaction.cursor_before
        } else {
            transaction.cursor_after
        };

        self.is_recording = true;
    }

    /// Helper to find the 2D end position of a string starting at `start`.
    fn calculate_end_position(
        &self,
        start: editor_core::cursor::Position,
        text: &str,
    ) -> editor_core::cursor::Position {
        let mut row = start.row;
        let mut col = start.col;

        let lines: Vec<&str> = text.split('\n').collect();

        if lines.len() > 1 {
            // We moved down lines. The new row is start.row + count of newlines.
            row += lines.len() - 1;
            // The column is simply the length of the very last segment.
            col = lines.last().unwrap_or(&"").len();
        } else {
            // We stayed on the same line. Just add the length to the current column.
            col += text.len();
        }

        editor_core::cursor::Position::new(row, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::cursor::{Cursor, Position};
    use editor_core::text::TextBuffer;

    fn setup() -> Document {
        Document::new(TextBuffer::new().unwrap())
    }

    #[test]
    fn test_newline_insertion_math() {
        let mut doc = setup();

        // Scenario: Pressing Enter on an empty line
        doc.insert("\n");
        assert_eq!(
            doc.cursor.head,
            Position::new(1, 0),
            "Cursor should be at start of line 2"
        );

        // Scenario: Inserting text then Enter
        doc.insert("Hi\n");
        assert_eq!(
            doc.cursor.head,
            Position::new(2, 0),
            "Cursor should be at start of line 3"
        );
    }

    #[test]
    fn test_undo_redo_newline_boundaries_fixed() {
        let mut doc = setup();

        println!("BEFORE LINE 1: {:#?}", doc);
        // 1. First Insert
        doc.insert("Line1");
        // Manually break batching if your History allows it,
        // or just accept that "Line1\nLine2" might be one transaction.

        println!("BEFORE LINE 2: {:#?}", doc);
        // 2. Second Insert starting with newline
        doc.insert("\nLine2");

        println!("BEFORE UNDO: {:#?}", doc);
        // If batched, one undo goes to (0,0). If not, it goes to (0,5).
        doc.undo();

        let current_line = doc.text_buffer.get_line(0);
        assert_eq!(current_line.unwrap(), "Line1");
        assert_eq!(doc.cursor.head, Position::new(0, 5));
    }

    #[test]
    fn test_get_line_stripped_edge_cases() {
        let mut doc = setup();

        // Case 1: Empty line
        doc.insert("\n");
        assert_eq!(doc.text_buffer.get_line_stripped(0).unwrap(), "");

        // Case 2: Text without newline
        doc.insert("Hello");
        println!("{:#?}", doc);
        assert_eq!(doc.text_buffer.get_line_stripped(1).unwrap(), "Hello");

        // Case 3: Mixed content
        doc.undo();
        doc.undo(); // Clear
        doc.insert("First\nSecond\nThird");
        assert_eq!(doc.text_buffer.get_line_stripped(0).unwrap(), "First");
        assert_eq!(doc.text_buffer.get_line_stripped(1).unwrap(), "Second");
        assert_eq!(doc.text_buffer.get_line_stripped(2).unwrap(), "Third");
    }

    #[test]
    fn test_backspace_at_line_boundary() {
        let mut doc = setup();
        doc.insert("ABC\nDEF");
        doc.cursor = Cursor::new(1, 0); // Cursor at start of "DEF"

        // Backspace should delete the '\n'
        doc.delete(true);

        assert_eq!(doc.text_buffer.get_line_stripped(0).unwrap(), "ABCDEF");
        assert_eq!(doc.cursor.head, Position::new(0, 3));

        doc.undo();
        assert_eq!(doc.text_buffer.get_line_stripped(0).unwrap(), "ABC");
        assert_eq!(doc.text_buffer.get_line_stripped(1).unwrap(), "DEF");
        assert_eq!(doc.cursor.head, Position::new(1, 0));
    }

    #[test]
    fn test_redo_restores_correct_cursor() {
        let mut doc = setup();
        doc.insert("Hello");
        let pos_after_hello = doc.cursor.head;

        doc.undo();
        assert_eq!(doc.cursor.head, Position::new(0, 0));

        doc.redo();
        assert_eq!(doc.cursor.head, pos_after_hello);
        assert_eq!(doc.text_buffer.get_line_stripped(0).unwrap(), "Hello");
    }

    #[test]
    fn test_replace_selection_across_lines() {
        let mut doc = setup();
        doc.insert("Hello\nWorld\nEnd");

        // Select "ello\nWorld\nE"
        doc.cursor = Cursor::new_selection(Position::new(0, 1), Position::new(2, 1));

        // Replace with "!"
        doc.insert("!");

        // Buffer should now be "H!nd"
        // Line 0: "H!nd"
        let line = doc.text_buffer.get_line(0).unwrap();
        assert!(line.contains("H!nd"));
        assert_eq!(doc.cursor.head, Position::new(0, 2));

        doc.undo();
        // Should restore original 3 lines
        assert_eq!(doc.text_buffer.get_line(1).unwrap().trim_end(), "World");
    }

    #[test]
    fn test_backspace_at_start_of_line_wraps() {
        let mut doc = setup();
        doc.insert("A\nB");
        doc.cursor = Cursor::new(1, 0); // At start of 'B'

        doc.delete(true); // Backspace

        // Should have merged lines into "AB"
        let line = doc.text_buffer.get_line(0).unwrap();
        assert!(line.contains("AB"));
        assert_eq!(doc.cursor.head, Position::new(0, 1));

        doc.undo();
        assert_eq!(doc.cursor.head, Position::new(1, 0));
    }

    #[test]
    fn test_delete_forward_at_end_of_line() {
        let mut doc = setup();
        doc.insert("A\nB");
        doc.cursor = Cursor::new(0, 1); // After 'A', before '\n'

        doc.delete(false); // Forward Delete

        let line = doc.text_buffer.get_line(0).unwrap();
        assert!(line.contains("AB"));

        doc.undo();
        assert_eq!(doc.cursor.head, Position::new(0, 1));
    }

    #[test]
    fn test_consecutive_inserts_batching() {
        let mut doc = setup();
        doc.insert("a");
        doc.insert("b");
        doc.insert("c");

        // Since we are typing character by character, History should batch them
        assert_eq!(doc.history.undo_stack.len(), 1);

        doc.undo();
        assert_eq!(doc.cursor.head, Position::new(0, 0));
        assert!(
            doc.text_buffer.get_line(0).is_none()
                || doc.text_buffer.get_line(0).unwrap().is_empty()
        );
    }
}
