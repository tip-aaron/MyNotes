use std::io::Write;

/// # The Core Philosophies of This API
///
/// - Coordinate-Based: The UI doesn't know what a byte offset is. It thinks in (line, column). The `TextBuffer`'s job is to take those coordinates, use your B-Tree to resolve them into absolute byte offsets, and feed those offsets to the Piece Table.
/// - Immutability for Reads: Functions that just query data (`get_line`, lines) take &self.
/// - Ownership of State: The `TextBuffer` owns the Piece Table and the B-Tree so they never drift out of sync. If an insert happens, the Buffer updates both simultaneously.
pub struct TextBuffer {
    piece_table: crate::piece_table::table::PieceTable,
    line_index: crate::line_index::btree::BTreeLineIndex,

    /// Tracks if the buffer has unsaved changes.
    is_dirty: bool,

    /// The file path, if this buffer is tied to a file on disk.
    filepath: Option<std::path::PathBuf>,

    /// Keeps the temporary backing file alive for new/unsaved buffers.
    /// Once the file is explicitly saved, we can drop this.
    _temp_backing: Option<tempfile::NamedTempFile>,
}

/*

==================================
===== CREATION, OPEN, & SAVE =====
==================================

*/

impl TextBuffer {
    /// Creates a new, empty text buffer backed by a temporary file.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying temporary file cannot be created
    /// or if the operating system fails to memory-map the temporary file.
    pub fn new() -> crate::errors::TextBufferResult<Self> {
        let tmp_file = tempfile::NamedTempFile::new()?;
        let mut file = tmp_file.as_file();

        file.write_all(b"Start writing...")?;
        file.sync_all()?;

        let mmap_file = io::mmap::MmapFile::open(tmp_file.path())?;
        let line_index = crate::line_index::btree::BTreeLineIndex::new(mmap_file.as_slice())?;
        let piece_table = crate::piece_table::table::PieceTable::new(mmap_file)?;

        Ok(Self {
            piece_table,
            line_index,
            is_dirty: false,
            filepath: Some(tmp_file.path().to_path_buf()),
            _temp_backing: Some(tmp_file),
        })
    }

    /// Opens a file, maps it into memory, and builds the initial indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file does not exist, lacks read permissions,
    /// or if the memory mapping operation fails.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Self> {
        // 1. Load MmapFile.
        // 2. Initialize PieceTable with the MmapFile.
        // 3. Scan the MmapFile slice to build the BTreeLineIndex.
        // 4. (Optional but recommended) Spawn the `notify` file watcher here.
        todo!("Implement mmap loading and initial B-Tree construction")
    }

    /// Safely flushes the evaluated state of the buffer to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no file path associated with the buffer,
    /// if the temporary save file cannot be written, or if the atomic rename fails.
    pub fn save(&mut self) -> std::io::Result<()> {
        // 1. Write the evaluated PieceTable to a temporary file.
        // 2. Atomically rename the temp file to `self.filepath`.
        // 3. Drop the old MmapFile and map the newly saved file.
        // 4. Reset `self.is_dirty = false`.
        // 5. Clear the `buf` (append buffer) since everything is now in the original file.
        todo!("Implement atomic save and remap")
    }
}

/*

==========================
===== INLINE METHODS =====
==========================

*/

impl TextBuffer {
    /// Returns the total number of lines in the buffer.
    #[inline]
    pub fn line_count(&self) -> usize {
        // Extract this directly from the root node's LineSummary
        match &self.line_index.root {
            crate::line_index::node::Node::Internal(n) => n.summary.line_count,
            crate::line_index::node::Node::Leaf(n) => n.summary.line_count,
        }
    }

    /// Returns the total byte size of the document.
    #[inline]
    pub fn byte_length(&self) -> u64 {
        // Extract this directly from the root node's LineSummary
        match &self.line_index.root {
            crate::line_index::node::Node::Internal(n) => n.summary.byte_len,
            crate::line_index::node::Node::Leaf(n) => n.summary.byte_len,
        }
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    #[inline]
    pub fn path(&self) -> Option<&std::path::Path> {
        self.filepath.as_deref()
    }
}

/*

===========================
========= GETTERS =========
===========================

*/

impl TextBuffer {
    /// Fetches a single line of text as a String.
    pub fn get_line(&self, line: usize) -> Option<String> {
        // 1. Query `self.line_index` to get the absolute byte offset and length of `line`.
        // 2. Pass that byte range to `self.piece_table` to resolve the actual string.
        todo!("Coordinate B-Tree lookup with PieceTable resolution")
    }

    /// Returns the LineRangeIter to traverse the B-Tree for a specific range of lines.
    /// This is your hyper-fast path for rendering the visible viewport on screen.
    pub fn lines(
        &self,
        start_line: usize,
        end_line: usize,
    ) -> crate::line_index::line_iter::LineRangeIter<'_> {
        // Return your specialized iterator, seeded with the correct starting Node
        // and offsets from the BTreeLineIndex.
        todo!("Initialize and return LineRangeIter")
    }
}

/*

========================================
========= INSERTION & DELETION =========
========================================

*/

impl TextBuffer {
    /// Inserts text at the given cursor position.
    pub fn insert(&mut self, cursor: &crate::cursor::Cursor, text: &str) {
        // 1. Handle Selection Replacement
        // If the user has text highlighted and starts typing, we delete the highlight first.
        let insert_position = if cursor.no_selection() {
            cursor.head
        } else {
            // We reuse our own delete logic to clear the selection
            self.delete_selection(cursor);
            // After deletion, the new insertion point is the start of where the selection was
            cursor.start()
        };

        // 2. Translate `insert_pos` to an absolute byte offset using `self.line_index`.
        // 3. Insert `text` into `self.piece_table` (which pushes to `buf` and updates `pieces`).
        // 4. Update `self.line_index`:
        //    a. Shift all subsequent offsets by `text.len()`.
        //    b. If `text` contains '\n', split the LeafNode at `insert_pos.line` and insert new summaries.
        // 5. Record the edit in `self.piece_table.undo_stack`.
        // 6. self.is_dirty = true;
        todo!("Implement insert coordination")
    }

    /// Deletes the text bounded by the given cursor's selection.
    /// If there is no selection, this does nothing. (Use `backspace` or `delete_forward` instead).
    pub fn delete_selection(&mut self, cursor: &crate::cursor::Cursor) {
        if cursor.no_selection() {
            return;
        }

        let start = cursor.start();
        let end = cursor.end();

        // 1. Translate `start` and `end` to absolute bytes using `self.line_index`.
        // 2. Remove byte range from `self.piece_table`.
        // 3. Update `self.line_index` (shrink offsets, merge nodes).
        todo!("Implement delete coordination")
    }

    /// Simulates the Backspace key.
    /// Deletes the selection, or the character immediately behind the cursor.
    ///
    /// # Panics
    ///
    /// Panics if the B-Tree line index is out of sync and fails to find the previous row,
    /// or if the previous row's byte length exceeds `usize::MAX` (e.g., >4GB on a 32-bit system).
    pub fn backspace(&mut self, cursor: &crate::cursor::Cursor) {
        if !cursor.no_selection() {
            return self.delete_selection(cursor);
        }

        if cursor.head.row == 0 && cursor.head.column == 0 {
            return; // At the very beginning, nothing to backspace
        }

        // --- Calculate previous position (Line Wrapping Logic) ---
        let start_position = if cursor.head.column > 0 {
            // Simple case: just move back one character on the same line.
            // Note: For full UTF-8 support later, this needs to step back by grapheme size,
            // but byte/char steps are fine for now.
            crate::cursor::Position {
                row: cursor.head.row,
                column: cursor.head.column - 1,
            }
        } else {
            // Wrapping case: Move to the very end of the PREVIOUS row.
            let prev_row = cursor.head.row - 1;

            // Ask the B-Tree for the exact length of the previous row
            let prev_row_len_u64 = self
                .line_index
                .get_line_length_at(prev_row)
                .expect("Row out of bounds in BTree");

            // Safely convert u64 to usize, panicking with a clear message on 32-bit bounds failure
            let safe_column: usize = prev_row_len_u64
                .try_into()
                .expect("Line length exceeds memory capacity for this architecture");

            crate::cursor::Position {
                row: prev_row,
                column: safe_column, // Snap to the end of that row
            }
        };

        // Create a temporary cursor to represent the single character we are deleting
        let delete_cursor = crate::cursor::Cursor::new_selection(start_position, cursor.head);
        self.delete_selection(&delete_cursor);
    }

    /// Simulates the Delete key.
    /// Deletes the selection, or the character immediately in front of the cursor.
    ///
    /// # Panics
    ///
    /// Panics if the B-Tree line index is out of sync and fails to find the current row,
    /// or if the current row's byte length exceeds `usize::MAX` (e.g., >4GB on a 32-bit system).
    pub fn delete_forward(&mut self, cursor: &crate::cursor::Cursor) {
        if !cursor.no_selection() {
            return self.delete_selection(cursor);
        }

        // --- Calculate next position ---
        let current_row_len_u64 = self
            .line_index
            .get_line_length_at(cursor.head.row)
            .expect("Row out of bounds in BTree");

        // Safely convert u64 to usize
        let current_row_len: usize = current_row_len_u64
            .try_into()
            .expect("Line length exceeds memory capacity for this architecture");

        let end_position = if cursor.head.column < current_row_len {
            // Move forward one character
            crate::cursor::Position {
                row: cursor.head.row,
                column: cursor.head.column + 1,
            }
        } else {
            // Wrapping case: If we are at the end of the line, Delete removes the newline character
            // bringing the next line up.
            let total_rows = self.line_count();
            if cursor.head.row + 1 >= total_rows {
                return; // At the very end of the document, nothing to delete forward
            }

            crate::cursor::Position {
                row: cursor.head.row + 1,
                column: 0,
            }
        };

        let delete_cursor = crate::cursor::Cursor::new_selection(cursor.head, end_position);
        self.delete_selection(&delete_cursor);
    }
}

/*

===============================
========= UNDO & REDO =========
===============================

*/

impl TextBuffer {
    pub fn undo(&mut self) {
        // 1. Pop from `self.piece_table.undo_stack`.
        // 2. Revert the piece table state.
        // 3. Critically: You must also apply the inverse structural changes to `self.line_index`!
        // 4. Push to `self.piece_table.redo_stack`.
        todo!("Implement undo")
    }

    pub fn redo(&mut self) {
        // 1. Pop from `self.piece_table.redo_stack`.
        // 2. Reapply the piece table state.
        // 3. Apply the structural changes to `self.line_index`.
        // 4. Push to `self.piece_table.undo_stack`.
        todo!("Implement redo")
    }
}
