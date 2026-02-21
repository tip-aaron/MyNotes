/// Holds the complete in-memory state for one open note.
///
/// - [`piece_table`] owns the editable document bytes with O(1) amortised
///   insert/delete and a full undo/redo stack.
/// - [`line_index`] provides O(log n) line ↔ byte-offset queries; rebuild it
///   with [`EditorState::rebuild_index`] after each batch of edits.
pub struct EditorState {
    pub piece_table: editor_core::piece_table::table::PieceTable,
    pub line_index: editor_core::line_index::BTreeLineIndex,
}

impl EditorState {
    /// Opens a file via memory-mapped I/O and builds an initial line index.
    ///
    /// Returns an error if the file cannot be opened or if the byte-length
    /// arithmetic overflows (only possible on >4 GiB files on 32-bit targets).
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let mmap = io::mmap::MmapFile::open(path)?;
        let line_index = editor_core::line_index::BTreeLineIndex::build(mmap.as_slice())?;
        let piece_table = editor_core::piece_table::table::PieceTable::new(mmap)?;
        Ok(Self {
            piece_table,
            line_index,
        })
    }

    /// Rebuilds the line index from the current document content.
    ///
    /// Call this once after a batch of [`PieceTable`] edits to keep the index
    /// consistent.  The rebuild is O(n) in document bytes but very cache-friendly.
    ///
    /// **Note**: this method materialises the full document into a temporary
    /// `Vec<u8>`.  For very large documents a future optimisation could stream
    /// directly from [`PieceTable`] pieces to avoid the allocation.
    pub fn rebuild_index(&mut self) -> Result<(), editor_core::enums::MathError> {
        let bytes = self.piece_table.get_bytes_at(0, self.piece_table.len())?;
        self.line_index = editor_core::line_index::BTreeLineIndex::build(&bytes)?;
        Ok(())
    }
}
