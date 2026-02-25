use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    LF,   // \n
    CRLF, // \r\n
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::LF => "\n",
            LineEnding::CRLF => "\r\n",
        }
    }
}

pub fn detect_line_ending(bytes: &[u8]) -> LineEnding {
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                // Found LF without preceding CR
                return LineEnding::LF;
            }
            b'\r' => {
                return if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    LineEnding::CRLF
                } else {
                    // Lone CR → treat as LF (or normalize)
                    LineEnding::LF
                };
            }
            _ => {}
        }
        i += 1;
    }

    // No newline found → default
    LineEnding::LF
}

/// # The Core Philosophies of This API
///
/// - Coordinate-Based: The UI doesn't know what a byte offset is. It thinks in (line, column). The `TextBuffer`'s job is to take those coordinates, use your B-Tree to resolve them into absolute byte offsets, and feed those offsets to the Piece Table.
/// - Immutability for Reads: Functions that just query data (`get_line`, lines) take &self.
/// - Ownership of State: The `TextBuffer` owns the Piece Table and the B-Tree so they never drift out of sync. If an insert happens, the Buffer updates both simultaneously.
#[derive(Debug)]
pub struct TextBuffer {
    piece_table: crate::piece_table::table::PieceTable,
    line_index: crate::line_index::btree::BTreeLineIndex,

    pub line_ending: LineEnding,

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

        file.write_all(b"")?;
        file.sync_all()?;

        let mmap_file = io::mmap::MmapFile::open(tmp_file.path())?;
        let line_index = crate::line_index::btree::BTreeLineIndex::new(mmap_file.as_slice())?;
        let piece_table = crate::piece_table::table::PieceTable::new(mmap_file)?;

        Ok(Self {
            piece_table,
            line_index,
            line_ending: LineEnding::LF,
            is_dirty: false,
            filepath: None,
            _temp_backing: Some(tmp_file),
        })
    }

    /// Creates a new, empty text buffer backed by a temporary file
    /// with base text.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying temporary file cannot be created
    /// or if the operating system fails to memory-map the temporary file.
    pub fn new_with_text(text: &str) -> crate::errors::TextBufferResult<Self> {
        let tmp_file = tempfile::NamedTempFile::new()?;
        let mut file = tmp_file.as_file();
        let line_ending = if text.contains("\r\n") {
            LineEnding::CRLF
        } else {
            LineEnding::LF
        };

        file.write_all(text.as_bytes())?;
        file.sync_all()?;

        let mmap_file = io::mmap::MmapFile::open(tmp_file.path())?;
        let line_index = crate::line_index::btree::BTreeLineIndex::new(mmap_file.as_slice())?;
        let piece_table = crate::piece_table::table::PieceTable::new(mmap_file)?;

        Ok(Self {
            piece_table,
            line_index,
            line_ending,
            is_dirty: false,
            filepath: None,
            _temp_backing: Some(tmp_file),
        })
    }

    /// Opens a file, maps it into memory, and builds the initial indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file does not exist, lacks read permissions,
    /// or if the memory mapping operation fails.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> crate::errors::TextBufferResult<Self> {
        let path_buf = path.as_ref().to_path_buf();
        // 1. Load MmapFile.
        // The OS sets up the page tables but doesn't read the whole file into RAM yet.
        let mmap_file = io::mmap::MmapFile::open(&path_buf)?;
        // 3. Scan the MmapFile slice to build the BTreeLineIndex.
        // We do this BEFORE transferring ownership of the mmap_file to the PieceTable.
        // The slice borrow is immediately dropped when `BTreeLineIndex::new` returns.
        // (Assuming `new` returns a Result, if not, remove the `?`).
        let line_ending = detect_line_ending(mmap_file.as_slice());
        let line_index = crate::line_index::btree::BTreeLineIndex::new(mmap_file.as_slice())?;
        // 2. Initialize PieceTable with the MmapFile.
        // This moves `mmap_file` into the PieceTable, where it will live as read-only backing storage.
        let piece_table = crate::piece_table::table::PieceTable::new(mmap_file)?;

        // 4. (Optional but recommended) Spawn the `notify` file watcher here.
        // Note: Architecturally, it is better to have `editor-state` handle `notify`
        // so it can route the filesystem events into your main UI event loop.
        // We leave this un-implemented in `editor-core`.

        Ok(Self {
            piece_table,
            line_index,
            line_ending,
            is_dirty: false,
            filepath: Some(path_buf),
            _temp_backing: None, // This is a real file on disk, no temp backing needed
        })
    }

    pub fn open_from<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
    ) -> crate::errors::TextBufferResult<()> {
        let path_buf = path.as_ref().to_path_buf();
        // 1. Load MmapFile.
        // The OS sets up the page tables but doesn't read the whole file into RAM yet.
        let mmap_file = io::mmap::MmapFile::open(&path_buf)?;
        let line_ending = detect_line_ending(mmap_file.as_slice());
        // 3. Scan the MmapFile slice to build the BTreeLineIndex.
        // We do this BEFORE transferring ownership of the mmap_file to the PieceTable.
        // The slice borrow is immediately dropped when `BTreeLineIndex::new` returns.
        // (Assuming `new` returns a Result, if not, remove the `?`).
        let line_index = crate::line_index::btree::BTreeLineIndex::new(mmap_file.as_slice())?;
        // 2. Initialize PieceTable with the MmapFile.
        // This moves `mmap_file` into the PieceTable, where it will live as read-only backing storage.
        let piece_table = crate::piece_table::table::PieceTable::new(mmap_file)?;

        self.piece_table = piece_table;
        self.line_ending = line_ending;
        self.line_index = line_index;
        self.filepath = Some(path_buf);
        self._temp_backing = None;

        // 4. (Optional but recommended) Spawn the `notify` file watcher here.
        // Note: Architecturally, it is better to have `editor-state` handle `notify`
        // so it can route the filesystem events into your main UI event loop.
        // We leave this un-implemented in `editor-core`.

        Ok(())
    }

    /// Safely flushes the evaluated state of the buffer to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no file path associated with the buffer,
    /// if the temporary save file cannot be written, or if the atomic rename fails.
    pub fn save(&mut self) -> std::io::Result<()> {
        // Ensure we actually have a file path to save to.
        let filepath = self.filepath.as_ref().ok_or_else(|| {
            // Assuming your TextBufferError can be constructed from an io::Error.
            // Adjust this if your error enum has a specific `MissingFilePath` variant.
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "No file path associated with this buffer. Use save_as().",
            )
        })?;

        // 1. Create a temporary file in the *same directory* as the target file.
        // This is strictly required for atomic renames; if the temp file is in /tmp
        // but the target is on a different hard drive, the OS rename will fail.
        let parent_dir = filepath
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut temp_save_file = tempfile::Builder::new()
            .prefix(".save_tmp_")
            .tempfile_in(parent_dir)?;

        // 2. Write the evaluated PieceTable to the temporary file.
        // (Assuming you have a method on PieceTable that iterates through the pieces
        // and returns their byte slices, or a dedicated `write_to` method).
        for chunk in self.piece_table.iter_bytes() {
            temp_save_file.write_all(chunk)?;
        }

        // Ensure all bytes are physically flushed to the disk drive controller.
        temp_save_file.as_file().sync_all()?;
        // 3. Atomically rename the temp file to `self.filepath`.
        // `persist` moves the file to the target path. We map its specific PersistError
        // back into a standard io::Error so it easily converts into TextBufferResult.
        temp_save_file.persist(filepath).map_err(|e| e.error)?;

        // 4. Drop the old MmapFile and map the newly saved file.
        let new_mmap = io::mmap::MmapFile::open(filepath)?;

        // 5. Reset the PieceTable state.
        // This method on your PieceTable should:
        // - Clear the `buf` (append buffer).
        // - Replace the old MmapFile with `new_mmap`.
        // - Collapse the `pieces` vector down into a single Piece spanning the whole file.
        self.piece_table.reset_to_mmap(new_mmap);

        // 6. Reset dirty flag.
        self.is_dirty = false;

        Ok(())
    }

    /// Saves the buffer to a new file path.
    ///
    /// This updates the internal file path, releases any temporary backing file,
    /// and performs a safe atomic save to the new destination.
    ///
    /// # Errors
    ///
    /// Returns an error if the new destination cannot be written to or if the
    /// atomic rename within `save()` fails.
    pub fn save_as<P: AsRef<std::path::Path>>(&mut self, path: P) -> std::io::Result<()> {
        let new_path = path.as_ref().to_path_buf();

        // 1. Update the internal path
        self.filepath = Some(new_path);

        // 2. Drop the temp backing file. If this buffer was created via `new()`,
        // it no longer needs the hidden /tmp file because it now has a real home.
        self._temp_backing = None;

        // 3. Delegate to your bulletproof atomic save logic!
        self.save()
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

    #[inline]
    pub fn get_line_len_at(&self, line_idx: usize) -> Option<u64> {
        self.line_index.get_line_length_at(line_idx)
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
    pub fn get_line(&self, line_idx: usize) -> Option<String> {
        // 1. Query `self.line_index` to get the absolute byte offset and length of `line`.
        // 2. Pass that byte range to `self.piece_table` to resolve the actual string.
        let line_length = self.line_index.get_line_length_at(line_idx)?;
        let start_abs_idx = self.line_index.line_idx_to_abs_idx(line_idx, false)?;

        self.piece_table.get_string(start_abs_idx, line_length).ok()
    }

    pub fn get_line_stripped(&self, line_idx: usize) -> Option<String> {
        let mut line = self.get_line(line_idx)?;

        // Remove trailing \r\n or \n
        if line.ends_with("\r\n") {
            line.truncate(line.len() - 2);
        } else if line.ends_with('\n') {
            line.truncate(line.len() - 1);
        }

        Some(line)
    }

    /// Returns the LineRangeIter to traverse the B-Tree for a specific range of lines.
    /// This is your hyper-fast path for rendering the visible viewport on screen.
    pub fn lines(
        &self,
        start_line: usize,
        end_line: usize,
    ) -> crate::line_index::line_iter::LineRangeIter<'_> {
        self.line_index.lines(start_line, end_line)
    }

    pub fn iter(&self) -> crate::line_index::line_iter::LineRangeIter<'_> {
        self.line_index.iter()
    }

    /// Converts a 2D screen coordinate (row, col) into a 1D absolute byte offset.
    ///
    /// `row` is the 0-indexed line number.
    /// `col` is the 0-indexed byte offset within that specific line.
    pub fn point_to_abs_offset(&self, row: usize, col: usize) -> Option<u64> {
        // 1. Find where the line starts in the 1D byte stream
        let line_start_abs_idx = self.line_index.line_idx_to_abs_idx(row, false)?;
        // 2. Validate the column doesn't exceed the line's length for safety
        let line_len = self.line_index.get_line_length_at(row)?;
        let col_u64 = col as u64;

        if col_u64 > line_len {
            // Depending on your editor's behavior, you might want to clamp this
            // to line_len instead of returning None.
            return None;
        }

        // 3. Add them together
        Some(line_start_abs_idx + col_u64)
    }

    pub fn get_cursor_selection(
        &self,
        cursor: &crate::cursor::Cursor,
    ) -> crate::errors::TextBufferResult<Option<String>> {
        // 1. Bail early if there's no selection to avoid unnecessary lookups
        if cursor.no_selection() {
            return Ok(None);
        }

        // 2. Get the normalized 2D start and end coordinates (ensuring start <= end)
        let (start, end) = cursor.range();
        // 3. Convert the 2D coordinates into 1D absolute byte offsets
        let start_abs = self
            .point_to_abs_offset(start.row, start.col)
            .ok_or(crate::errors::TextBufferError::PositionToAbsIdxError)?;
        let end_abs = self
            .point_to_abs_offset(end.row, end.col)
            .ok_or(crate::errors::TextBufferError::PositionToAbsIdxError)?;

        // Safety check to prevent math underflow just in case coordinates got mangled
        if start_abs > end_abs {
            return Ok(None);
        }

        // 4. Calculate the length of the selection in bytes
        let length = end_abs - start_abs;

        // 5. Query the piece table directly for that exact slice
        Ok(Some(self.piece_table.get_string(start_abs, length)?))
    }
}

/*

========================================
========= INSERTION & DELETION =========
========================================

*/

impl TextBuffer {
    /// Inserts text at the given cursor position and returns the new position
    /// exactly at the end of the inserted text.
    pub fn insert(
        &mut self,
        cursor: &crate::cursor::Cursor,
        text: &str,
    ) -> crate::errors::TextBufferResult<crate::cursor::Position> {
        // 1. Handle Selection Replacement
        // If the user has text highlighted and starts typing, we delete the highlight first.
        let insert_position = if cursor.no_selection() {
            cursor.head
        } else {
            // We reuse our own delete logic to clear the selection
            self.delete_selection(cursor)?;
            // After deletion, the new insertion point is the start of where the selection was
            cursor.start()
        };

        // 2. Translate `insert_position` to an absolute byte offset using `self.line_index`.
        let abs_offset = self
            .point_to_abs_offset(insert_position.row, insert_position.col)
            .ok_or(crate::enums::MathError::OutOfBounds(insert_position.row))?;

        let bytes = text.as_bytes();

        // 3. Insert `text` into `self.piece_table`.
        self.piece_table.insert(abs_offset, bytes)?;

        // 4. Update `self.line_index` (The B-Tree).
        self.line_index.insert(abs_offset, bytes)?;

        // 5. Mark the file as modified
        self.is_dirty = true;

        // 6. Calculate where the cursor should end up after this insertion.
        // We split by '\n' to handle multi-line pastes correctly.
        let mut split_lines = text.split('\n');

        // The first segment adds to the existing column.
        // Subsequent segments start at column 0.
        let first_segment = split_lines.next().unwrap_or("");
        let remaining_lines: Vec<&str> = split_lines.collect();

        let new_position = if remaining_lines.is_empty() {
            // Single-line insert: Row stays the same, column increases by byte length
            crate::cursor::Position::new(
                insert_position.row,
                insert_position.col + first_segment.len(),
            )
        } else {
            // Multi-line insert: Row increases, column is exactly the length of the final line
            crate::cursor::Position::new(
                insert_position.row + remaining_lines.len(),
                remaining_lines.last().unwrap().len(),
            )
        };

        Ok(new_position)
    }

    //// Deletes the text bounded by the given cursor's selection.
    /// Returns the new `Position` the cursor should collapse to.
    pub fn delete_selection(
        &mut self,
        cursor: &crate::cursor::Cursor,
    ) -> crate::errors::TextBufferResult<(crate::cursor::Position, String)> {
        if cursor.no_selection() {
            return Ok((cursor.head, String::new()));
        }

        // Use the helper we wrote earlier to grab the text before it's gone!
        let deleted_text = self.get_cursor_selection(cursor)?;

        let (top_left, bottom_right) = cursor.range();

        let start_offset = self
            .point_to_abs_offset(top_left.row, top_left.col)
            .unwrap();
        let end_offset = self
            .point_to_abs_offset(bottom_right.row, bottom_right.col)
            .unwrap();
        let length = end_offset - start_offset;

        self.piece_table.delete(start_offset, length)?;
        self.line_index.remove(start_offset, length)?;
        self.is_dirty = true;

        Ok((top_left, deleted_text.unwrap_or("".to_string())))
    }

    /// Simulates the Backspace key.
    /// Deletes the selection, or the character immediately behind the cursor.
    pub fn backspace(
        &mut self,
        cursor: &crate::cursor::Cursor,
    ) -> crate::errors::TextBufferResult<(crate::cursor::Position, String)> {
        if !cursor.no_selection() {
            return self.delete_selection(cursor);
        }

        if cursor.head.row == 0 && cursor.head.col == 0 {
            return Ok((cursor.head, "".to_string())); // At the very beginning, nothing to backspace
        }

        let start_position = if cursor.head.col > 0 {
            crate::cursor::Position {
                row: cursor.head.row,
                col: cursor.head.col - 1,
            }
        } else {
            // Wrapping case: Move to the character *just before* the next line starts
            let prev_row = cursor
                .head
                .row
                .checked_sub(1)
                .ok_or(crate::enums::MathError::OutOfBounds(0))?;
            let prev_row_len_u64 = self
                .line_index
                .get_line_length_at(prev_row)
                .ok_or(crate::enums::MathError::OutOfBounds(prev_row))?;
            let safe_column = <u64 as TryInto<usize>>::try_into(prev_row_len_u64)
                .map_err(|_| crate::enums::MathError::Overflow)?;

            crate::cursor::Position {
                row: prev_row,
                col: safe_column.saturating_sub(1), // Target the \n character
            }
        };

        let delete_cursor = crate::cursor::Cursor::new_selection(start_position, cursor.head);

        // delete_selection will naturally return `start_position` for us!
        self.delete_selection(&delete_cursor)
    }

    /// Simulates the Delete key.
    /// Deletes the selection, or the character immediately in front of the cursor.
    pub fn delete_forward(
        &mut self,
        cursor: &crate::cursor::Cursor,
    ) -> crate::errors::TextBufferResult<(crate::cursor::Position, String)> {
        if !cursor.no_selection() {
            return self.delete_selection(cursor);
        }

        let current_row_len_u64 = self
            .line_index
            .get_line_length_at(cursor.head.row)
            .ok_or(crate::enums::MathError::OutOfBounds(cursor.head.row))?;
        let current_row_len = <u64 as TryInto<usize>>::try_into(current_row_len_u64)
            .map_err(|_| crate::enums::MathError::Overflow)?;
        let end_position = if cursor.head.col >= current_row_len.saturating_sub(1) {
            let total_rows = self.line_count(); // Assuming you have this implemented

            if cursor.head.row + 1 >= total_rows {
                return Ok((cursor.head, "".to_string())); // End of file, nothing to forward delete
            }

            crate::cursor::Position {
                row: cursor.head.row + 1,
                col: 0,
            }
        } else {
            crate::cursor::Position {
                row: cursor.head.row,
                col: cursor.head.col + 1,
            }
        };

        let delete_cursor = crate::cursor::Cursor::new_selection(cursor.head, end_position);

        // delete_selection will naturally return `cursor.head` for us!
        self.delete_selection(&delete_cursor)
    }
}

impl std::fmt::Display for TextBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let total_len = self.line_index.root.summary().byte_len;

        self.piece_table.fmt_helper(0, total_len, f)
    }
}

#[cfg(test)]
mod text_buffer_creation_save_tests {
    use crate::text::TextBuffer;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_textbuffer_new() {
        let buffer = TextBuffer::new().expect("Failed to create new TextBuffer");

        assert!(buffer.filepath.is_none());
        assert!(buffer._temp_backing.is_some());
        assert!(!buffer.is_dirty);

        let bytes: Vec<u8> = buffer.piece_table.iter_bytes().flatten().copied().collect();
        assert_eq!(bytes, b"");
    }

    #[test]
    fn test_textbuffer_open() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Hello from disk").unwrap();
        temp_file.as_file().sync_all().unwrap();
        let path = temp_file.path().to_path_buf();

        let buffer = TextBuffer::open(&path).expect("Failed to open TextBuffer");

        assert_eq!(buffer.filepath, Some(path));
        assert!(buffer._temp_backing.is_none());
        assert!(!buffer.is_dirty);

        let bytes: Vec<u8> = buffer.piece_table.iter_bytes().flatten().copied().collect();
        assert_eq!(bytes, b"Hello from disk");
    }

    #[test]
    fn test_textbuffer_save_without_filepath_fails() {
        let mut buffer = TextBuffer::new().unwrap();
        let result = buffer.save();

        assert!(matches!(result, Err(e) if e.kind() == std::io::ErrorKind::InvalidInput));
    }

    #[test]
    fn test_textbuffer_save_as() {
        let mut buffer = TextBuffer::new().unwrap();
        let target_dir = tempfile::tempdir().unwrap();
        let target_path = target_dir.path().join("my_new_file.txt");

        // Execute save_as
        buffer
            .save_as(&target_path)
            .expect("save_as should succeed");

        // Assert only the state transitions unique to save_as
        // (Disk writes and clean flags are tested in save_success)
        assert_eq!(buffer.filepath, Some(target_path));
        assert!(buffer._temp_backing.is_none());
    }

    #[test]
    fn test_textbuffer_save_success() {
        // Use a temporary directory instead of NamedTempFile to avoid Windows file locks
        let target_dir = tempfile::tempdir().unwrap();
        let path = target_dir.path().join("save_success_test.txt");

        // std::fs::write opens, writes, and immediately closes the file handle
        std::fs::write(&path, b"Original text").unwrap();

        let mut buffer = TextBuffer::open(&path).unwrap();
        buffer.piece_table.insert_last(0, b" plus edits").unwrap();
        buffer.is_dirty = true;

        // Execute Save (This will now succeed on Windows!)
        buffer.save().expect("Save should succeed");

        // Assert core save transitions and data integrity
        assert!(!buffer.is_dirty);

        let disk_contents = std::fs::read(&path).unwrap();
        assert_eq!(disk_contents, b"Original text plus edits");

        let bytes: Vec<u8> = buffer.piece_table.iter_bytes().flatten().copied().collect();
        assert_eq!(bytes, b"Original text plus edits");
    }
}

#[cfg(test)]
mod text_buffer_getter_tests {
    use super::*;
    use crate::cursor::{Cursor, Position};

    #[test]
    fn test_get_line() {
        let mut text_buffer = TextBuffer::new().expect("Failed to create new TextBuffer");

        text_buffer
            .line_index
            .insert(0, b"hello, there\nhaha\nwoah")
            .unwrap();
        text_buffer
            .piece_table
            .insert(0, b"hello, there\nhaha\nwoah")
            .unwrap();

        let line1 = text_buffer.get_line(0);
        let line2 = text_buffer.get_line(1);
        let line3 = text_buffer.get_line(2);
        let line4 = text_buffer.get_line(3);

        assert_eq!(line1, Some(String::from("hello, there\n")));
        assert_eq!(line2, Some(String::from("haha\n")));
        assert_eq!(line3, Some(String::from("woah")));
        assert_eq!(line4, None);
    }

    #[test]
    fn test_get_cursor_selection_logic() {
        // Setup: Buffer with "Hello\nWorld"
        let mut buffer = TextBuffer::new().unwrap(); // Assuming initial state setup
        buffer.insert(&Cursor::default(), "Hello\nWorld").unwrap();

        // 1. Test forward selection (left to right)
        // Select "ello" (0,1) to (0,5)
        let forward_cursor = Cursor::new_selection(Position::new(0, 1), Position::new(0, 5));
        assert_eq!(
            buffer.get_cursor_selection(&forward_cursor).unwrap(),
            Some("ello".to_string())
        );

        // 2. Test backward selection (right to left)
        // Select "Worl" (1,0) to (1,4) but anchor at 4, head at 0
        let backward_cursor = Cursor::new_selection(Position::new(1, 4), Position::new(1, 0));
        assert_eq!(
            buffer.get_cursor_selection(&backward_cursor).unwrap(),
            Some("Worl".to_string())
        );

        // 3. Test multi-line selection
        // Select "o\nWo"
        let multiline_cursor = Cursor::new_selection(Position::new(0, 4), Position::new(1, 2));
        assert_eq!(
            buffer.get_cursor_selection(&multiline_cursor).unwrap(),
            Some("o\nWo".to_string())
        );

        // 4. Test no selection
        let empty_cursor = Cursor::new(0, 0);
        assert!(
            buffer
                .get_cursor_selection(&empty_cursor)
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(test)]
mod text_buffer_editing_tests {
    use super::*;
    use crate::cursor::{Cursor, Position};

    /// Helper to create a cursor with no selection
    fn make_cursor(row: usize, col: usize) -> Cursor {
        let pos = Position { row, col };
        Cursor::new_selection(pos, pos)
    }

    /// Helper to create a cursor with a selection
    fn make_selection(
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    ) -> Cursor {
        Cursor::new_selection(
            Position {
                row: start_row,
                col: start_col,
            },
            Position {
                row: end_row,
                col: end_col,
            },
        )
    }

    // ==========================================
    // INSERT TESTS
    // ==========================================

    #[test]
    fn test_insert_basic_and_multiline() {
        let mut buffer = TextBuffer::new_with_text("Hello").unwrap();

        // 1. Basic Insert
        let cursor = make_cursor(0, 5);
        buffer.insert(&cursor, " World").unwrap();
        assert_eq!(buffer.to_string(), "Hello World");

        // 2. Multiline Insert (Testing B-Tree node splitting)
        let cursor = make_cursor(0, 5);
        buffer.insert(&cursor, "\nBrave\n").unwrap();
        assert_eq!(buffer.to_string(), "Hello\nBrave\n World");
    }

    #[test]
    fn test_insert_with_selection_replaces_text() {
        let mut buffer = TextBuffer::new_with_text("Hello World").unwrap();

        // Select "World" (row 0, col 6 to row 0, col 11)
        let cursor = make_selection(0, 6, 0, 11);

        // Typing "Rust" should delete "World" and insert "Rust"
        buffer.insert(&cursor, "Rust").unwrap();
        assert_eq!(buffer.to_string(), "Hello Rust");
    }

    // ==========================================
    // DELETE SELECTION TESTS
    // ==========================================

    #[test]
    fn test_delete_selection_single_and_multiline() {
        let mut buffer = TextBuffer::new_with_text("Line 1\nLine 2\nLine 3").unwrap();

        // 1. Single line deletion (Delete " 2")
        let cursor = make_selection(1, 4, 1, 6);
        buffer.delete_selection(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "Line 1\nLine\nLine 3");

        // 2. Multi-line deletion (Delete from end of "1" to start of "3")
        let cursor = make_selection(0, 6, 2, 5);
        buffer.delete_selection(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "Line 13");
    }

    #[test]
    fn test_delete_selection_backwards() {
        let mut buffer = TextBuffer::new_with_text("Hello World").unwrap();

        // Simulate a user dragging the mouse right-to-left
        // Head is before Tail
        let cursor = make_selection(0, 11, 0, 6);
        buffer.delete_selection(&cursor).unwrap();

        assert_eq!(buffer.to_string(), "Hello ");
    }

    // ==========================================
    // BACKSPACE TESTS
    // ==========================================

    #[test]
    fn test_backspace_basic_and_wrapping() {
        let mut buffer = TextBuffer::new_with_text("A\nB").unwrap();

        // 1. Basic backspace (Delete 'B')
        let cursor = make_cursor(1, 1);
        buffer.backspace(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "A\n");

        // 2. Line Wrapping (Delete '\n', joining lines)
        let cursor = make_cursor(1, 0);
        buffer.backspace(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "A");
    }

    #[test]
    fn test_backspace_at_document_start_does_nothing() {
        let mut buffer = TextBuffer::new_with_text("Hello").unwrap();
        let cursor = make_cursor(0, 0);

        buffer.backspace(&cursor).unwrap();

        assert_eq!(
            buffer.to_string(),
            "Hello",
            "Backspacing at 0,0 should not modify the document"
        );
    }

    #[test]
    fn test_backspace_with_selection_acts_as_delete() {
        let mut buffer = TextBuffer::new_with_text("Hello World").unwrap();
        let cursor = make_selection(0, 0, 0, 6);

        buffer.backspace(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "World");
    }

    // ==========================================
    // DELETE FORWARD TESTS
    // ==========================================

    #[test]
    fn test_delete_forward_basic_and_wrapping() {
        let mut buffer = TextBuffer::new_with_text("A\nB").unwrap();

        // 1. Basic delete forward (Delete 'A')
        let cursor = make_cursor(0, 0);

        buffer.delete_forward(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "\nB");

        // 2. Line Wrapping (Cursor at end of line 0, delete forward removes '\n')
        let cursor = make_cursor(0, 0); // Note: Since 'A' is gone, line 0 is now empty
        buffer.delete_forward(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "B");
    }

    #[test]
    fn test_delete_forward_at_document_end_does_nothing() {
        let mut buffer = TextBuffer::new_with_text("Hello").unwrap();
        let cursor = make_cursor(0, 5);

        buffer.delete_forward(&cursor).unwrap();

        assert_eq!(
            buffer.to_string(),
            "Hello",
            "Deleting forward at the end of the document should not modify it"
        );
    }

    #[test]
    fn test_delete_forward_with_selection_acts_as_delete() {
        let mut buffer = TextBuffer::new_with_text("Hello World").unwrap();
        let cursor = make_selection(0, 6, 0, 11);

        buffer.delete_forward(&cursor).unwrap();
        assert_eq!(buffer.to_string(), "Hello ");
    }

    /*

    ==== ACCOMMODATE CURSOR POSITIONS ====

    */

    #[test]
    fn test_insert_returns_correct_position() {
        let mut buffer = TextBuffer::new().unwrap();
        let cursor = Cursor::default();

        // Single line insert
        let new_pos = buffer.insert(&cursor, "Rust").unwrap();
        assert_eq!(new_pos, Position::new(0, 4));

        // Multi-line insert (Paste)
        let paste_cursor = Cursor::new(0, 4);
        let pos_after_paste = buffer.insert(&paste_cursor, "\nIs\nCool").unwrap();

        // Should be on row 2 (0 + 2 newlines), column 4 ("Cool" is 4 bytes)
        assert_eq!(pos_after_paste, Position::new(2, 4));
    }

    #[test]
    fn test_delete_selection_returns_text_and_pos() {
        let mut buffer = TextBuffer::new().unwrap();
        buffer.insert(&Cursor::default(), "Delete Me").unwrap();

        // Select "Delete"
        let cursor = Cursor::new_selection(Position::new(0, 0), Position::new(0, 6));

        let (new_pos, deleted_text) = buffer.delete_selection(&cursor).unwrap();

        // 1. Check if the returned text is correct
        assert_eq!(deleted_text, "Delete");

        // 2. Check if the cursor collapsed to the start
        assert_eq!(new_pos, Position::new(0, 0));

        // 3. Check buffer state
        assert_eq!(buffer.get_line(0), Some(" Me".to_string()));
    }

    #[test]
    fn test_backspace_returns_deleted_char() {
        let mut buffer = TextBuffer::new().unwrap();
        buffer.insert(&Cursor::default(), "Rust").unwrap();

        // Cursor is at the end "Rust|"
        let cursor = Cursor::new(0, 4);
        let (new_pos, deleted_text) = buffer.backspace(&cursor).unwrap();

        assert_eq!(deleted_text, "t");
        assert_eq!(new_pos, Position::new(0, 3));
        assert_eq!(buffer.get_line(0), Some("Rus".to_string()));
    }

    #[test]
    fn test_delete_forward_across_lines() {
        let mut buffer = TextBuffer::new().unwrap();
        buffer.insert(&Cursor::default(), "A\nB").unwrap();

        // Cursor is at "A|\nB" (row 0, col 1)
        let cursor = Cursor::new(0, 1);
        let (new_pos, deleted_text) = buffer.delete_forward(&cursor).unwrap();

        // It should have deleted the newline character
        assert_eq!(deleted_text, "\n");

        // Position should stay at (0, 1) but the text "B" is pulled up
        assert_eq!(new_pos, Position::new(0, 1));
        assert_eq!(buffer.get_line(0), Some("AB".to_string()));
    }

    #[test]
    fn test_empty_delete_selection() {
        let mut buffer = TextBuffer::new().unwrap();
        buffer.insert(&Cursor::default(), "Safe").unwrap();

        let cursor = Cursor::new(0, 2); // No selection
        let (new_pos, deleted_text) = buffer.delete_selection(&cursor).unwrap();

        assert_eq!(deleted_text, "");
        assert_eq!(new_pos, Position::new(0, 2));
    }

    #[test]
    fn test_replace_via_insert_position() {
        let mut buffer = TextBuffer::new().unwrap();
        buffer.insert(&Cursor::default(), "Replace Me").unwrap();

        // Select "Replace"
        let selection = Cursor::new_selection(Position::new(0, 0), Position::new(0, 7));

        // Replacing "Replace" with "Fixed"
        let final_pos = buffer.insert(&selection, "Fixed").unwrap();

        // The cursor should end up at the end of "Fixed" (col 5)
        assert_eq!(final_pos, Position::new(0, 5));

        // Verify buffer state via get_line (assuming line 0)
        assert_eq!(buffer.get_line(0), Some("Fixed Me".to_string()));
    }
}
