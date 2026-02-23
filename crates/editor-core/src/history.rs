#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    pub actions: Vec<crate::enums::EditAction>,
    pub cursor_before: crate::cursor::Cursor,
    pub cursor_after: crate::cursor::Cursor,
}

#[derive(Debug)]
pub struct History {
    pub undo_stack: Vec<Transaction>,
    pub redo_stack: Vec<Transaction>,
}

impl History {
    /// Records a replacement (deleting a selection and immediately inserting text).
    /// Creates a single composite transaction so it can be undone in one step.
    pub fn record_replace(
        &mut self,
        start: crate::cursor::Position,
        end: crate::cursor::Position,
        deleted_text: &str,
        inserted_text: &str,
        cursor_before: crate::cursor::Cursor,
        cursor_after: crate::cursor::Cursor,
    ) {
        self.redo_stack.clear();

        self.undo_stack.push(Transaction {
            actions: vec![
                crate::enums::EditAction::Delete {
                    pos: start,
                    end,
                    text: deleted_text.to_string(),
                },
                crate::enums::EditAction::Insert {
                    pos: start, // Insert always happens exactly where the deletion started
                    text: inserted_text.to_string(),
                },
            ],
            cursor_before,
            cursor_after,
        });
    }

    /// Records an insertion, batching it with the previous insertion if they are contiguous
    /// on the same row.
    pub fn record_insert(
        &mut self,
        pos: crate::cursor::Position,
        text: &str,
        cursor_before: crate::cursor::Cursor,
        cursor_after: crate::cursor::Cursor,
    ) -> Result<(), crate::enums::MathError> {
        // Any new action invalidates the redo stack
        self.redo_stack.clear();

        if let Some(last_tx) = self.undo_stack.last_mut()
            && let Some(crate::enums::EditAction::Insert {
                            pos: last_pos,
                            text: last_text,
                        }) = last_tx.actions.last_mut()
            && last_pos.row == pos.row // Must be on the same row to batch
            && !text.contains('\n')    // FIX: Do not batch if typing a newline
            && !last_text.contains('\n') // FIX: Do not batch if previous text has a newline
            && last_pos
            .col
            .checked_add(last_text.len())
            .ok_or(crate::enums::MathError::Overflow)?
            == pos.col
        {
            // Check if the new insert is exactly at the end of the last insert
            // Batch them together!
            last_text.push_str(text);
            last_tx.cursor_after = cursor_after;

            return Ok(());
        }

        // If we couldn't batch, push a new transaction
        self.undo_stack.push(Transaction {
            actions: vec![crate::enums::EditAction::Insert {
                pos,
                text: text.to_string(),
            }],
            cursor_before,
            cursor_after,
        });

        Ok(())
    }

    /// Records a deletion, batching consecutive backspaces or forward deletes on the same row.
    pub fn record_delete(
        &mut self,
        start: crate::cursor::Position,
        end: crate::cursor::Position,
        deleted_text: &str,
        cursor_before: crate::cursor::Cursor,
        cursor_after: crate::cursor::Cursor,
    ) -> Result<(), crate::enums::MathError> {
        self.redo_stack.clear();

        if let Some(last_tx) = self.undo_stack.last_mut()
            && let Some(crate::enums::EditAction::Delete {
                            pos: last_start,
                            end: last_end,
                            text: last_text,
                        }) = last_tx.actions.last_mut()
            // Strict constraint: Only batch if everything happens on the same row.
            // This prevents multi-line deletes from messing up the bounding box math.
            && last_start.row == start.row
            && !deleted_text.contains('\n')    // FIX: Do not batch if typing a newline
            && !last_text.contains('\n') // FIX: Do not batch if previous text has a newline
            && last_end.row == end.row
        {
            // SCENARIO 1: Backspace Batching
            // The end of the new delete hits the start of the previous delete.
            // e.g., Deleted "b" then backspaced "a"
            if end == *last_start {
                // Prepend the text
                *last_text = format!("{}{}", deleted_text, last_text);

                // Expand the bounding box backwards
                *last_start = start;

                // Update cursor
                last_tx.cursor_after = cursor_after;

                return Ok(());
            }
            // SCENARIO 2: Forward Delete Batching
            // The new delete happens AT the exact same start position.
            // e.g., User pressed Delete on "a", then Delete on "b"
            else if start == *last_start {
                // Append the text
                last_text.push_str(deleted_text);

                // Expand the bounding box forwards by the length of the newly deleted string
                last_end.col = last_end
                    .col
                    .checked_add(deleted_text.len())
                    .ok_or(crate::enums::MathError::Overflow)?;

                // Update cursor
                last_tx.cursor_after = cursor_after;

                return Ok(());
            }
        }

        // SCENARIO 3: No Batching Possible
        // Push a brand-new transaction with the exact bounding box provided.
        self.undo_stack.push(Transaction {
            actions: vec![crate::enums::EditAction::Delete {
                pos: start,
                end,
                text: deleted_text.to_string(),
            }],
            cursor_before,
            cursor_after,
        });

        Ok(())
    }

    pub fn undo(&mut self) -> Option<Transaction> {
        let tx = self.undo_stack.pop()?;
        self.redo_stack.push(tx.clone());
        Some(tx)
    }

    pub fn redo(&mut self) -> Option<Transaction> {
        let tx = self.redo_stack.pop()?;
        self.undo_stack.push(tx.clone());
        Some(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::{Cursor, Position};
    // Make sure Cursor, Position, and EditAction are in scope
    // use crate::cursor::{Cursor, Position};
    // use crate::enums::EditAction;

    #[track_caller]
    fn assert_insert(
        action: &crate::enums::EditAction,
        expected_pos: Position,
        expected_text: &str,
    ) {
        match action {
            crate::enums::EditAction::Insert { pos, text } => {
                assert_eq!(*pos, expected_pos, "Insert position mismatch");
                assert_eq!(text, expected_text, "Insert text mismatch");
            }
            _ => panic!("Expected Insert action but found a different EditAction"),
        }
    }

    #[track_caller]
    fn assert_delete(
        action: &crate::enums::EditAction,
        expected_start: Position,
        expected_end: Position,
        expected_text: &str,
    ) {
        match action {
            crate::enums::EditAction::Delete {
                pos: start,
                end,
                text,
            } => {
                assert_eq!(*start, expected_start, "Delete start mismatch");
                assert_eq!(*end, expected_end, "Delete end mismatch");
                assert_eq!(text, expected_text, "Delete text mismatch");
            }
            _ => panic!("Expected Delete action but found a different EditAction"),
        }
    }

    // --- Tests ---

    #[test]
    fn test_insert_batching() {
        let mut history = History {
            undo_stack: vec![],
            redo_stack: vec![],
        };

        // User types 'H' then 'i'
        history
            .record_insert(
                Position::new(0, 0),
                "H",
                Cursor::new(0, 0),
                Cursor::new(0, 1),
            )
            .unwrap();
        history
            .record_insert(
                Position::new(0, 1),
                "i",
                Cursor::new(0, 1),
                Cursor::new(0, 2),
            )
            .unwrap();

        assert_eq!(
            history.undo_stack.len(),
            1,
            "Should batch into a single transaction"
        );

        let tx = &history.undo_stack[0];
        assert_eq!(tx.cursor_before, Cursor::new(0, 0));
        assert_eq!(tx.cursor_after, Cursor::new(0, 2));

        assert_insert(&tx.actions[0], Position::new(0, 0), "Hi");
    }

    #[test]
    fn test_backspace_batching() {
        let mut history = History {
            undo_stack: vec![],
            redo_stack: vec![],
        };

        // User deletes 'b' then 'a' via backspace
        history
            .record_delete(
                Position::new(0, 1),
                Position::new(0, 2),
                "b",
                Cursor::new(0, 2),
                Cursor::new(0, 1),
            )
            .unwrap();
        history
            .record_delete(
                Position::new(0, 0),
                Position::new(0, 1),
                "a",
                Cursor::new(0, 1),
                Cursor::new(0, 0),
            )
            .unwrap();

        assert_eq!(
            history.undo_stack.len(),
            1,
            "Should batch consecutive backspaces"
        );

        let tx = &history.undo_stack[0];
        assert_delete(
            &tx.actions[0],
            Position::new(0, 0),
            Position::new(0, 2),
            "ab",
        );
    }

    #[test]
    fn test_forward_delete_batching() {
        let mut history = History {
            undo_stack: vec![],
            redo_stack: vec![],
        };

        // User presses 'Delete' on 'a' then 'b'
        history
            .record_delete(
                Position::new(0, 0),
                Position::new(0, 1),
                "a",
                Cursor::new(0, 0),
                Cursor::new(0, 0),
            )
            .unwrap();
        history
            .record_delete(
                Position::new(0, 0),
                Position::new(0, 1),
                "b",
                Cursor::new(0, 0),
                Cursor::new(0, 0),
            )
            .unwrap();

        assert_eq!(
            history.undo_stack.len(),
            1,
            "Should batch consecutive forward deletes"
        );

        let tx = &history.undo_stack[0];
        assert_delete(
            &tx.actions[0],
            Position::new(0, 0),
            Position::new(0, 2),
            "ab",
        );
    }

    #[test]
    fn test_record_replace() {
        let mut history = History {
            undo_stack: vec![],
            redo_stack: vec![],
        };

        // User highlights "apple" and types "p"
        history.record_replace(
            Position::new(0, 0),
            Position::new(0, 5),
            "apple",
            "p",
            Cursor::new_selection(Position::new(0, 0), Position::new(0, 5)),
            Cursor::new(0, 1),
        );

        assert_eq!(history.undo_stack.len(), 1);
        let tx = &history.undo_stack[0];
        assert_eq!(tx.actions.len(), 2);

        assert_delete(
            &tx.actions[0],
            Position::new(0, 0),
            Position::new(0, 5),
            "apple",
        );
        assert_insert(&tx.actions[1], Position::new(0, 0), "p");
    }

    #[test]
    fn test_replace_with_subsequent_insert_batching() {
        let mut history = History {
            undo_stack: vec![],
            redo_stack: vec![],
        };

        // User highlights "apple" and types "p", then continues typing "i" and "e"
        history.record_replace(
            Position::new(0, 0),
            Position::new(0, 5),
            "apple",
            "p",
            Cursor::new_selection(Position::new(0, 0), Position::new(0, 5)),
            Cursor::new(0, 1),
        );
        history
            .record_insert(
                Position::new(0, 1),
                "i",
                Cursor::new(0, 1),
                Cursor::new(0, 2),
            )
            .unwrap();
        history
            .record_insert(
                Position::new(0, 2),
                "e",
                Cursor::new(0, 2),
                Cursor::new(0, 3),
            )
            .unwrap();

        assert_eq!(history.undo_stack.len(), 1);
        let tx = &history.undo_stack[0];
        assert_eq!(tx.actions.len(), 2);

        // The insert action should have accumulated the keystrokes
        assert_insert(&tx.actions[1], Position::new(0, 0), "pie");
    }

    #[test]
    fn test_undo_redo_stack_movement() {
        let mut history = History {
            undo_stack: vec![],
            redo_stack: vec![],
        };

        history
            .record_insert(
                Position::new(0, 0),
                "A",
                Cursor::new(0, 0),
                Cursor::new(0, 1),
            )
            .unwrap();

        let undone = history.undo().unwrap();
        assert_eq!(history.undo_stack.len(), 0);
        assert_eq!(history.redo_stack.len(), 1);

        let redone = history.redo().unwrap();
        assert_eq!(undone, redone);
        assert_eq!(history.undo_stack.len(), 1);
        assert_eq!(history.redo_stack.len(), 0);
    }
}
