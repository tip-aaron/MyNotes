/// Represents a specific location in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    pub row: usize,
    /// The byte offset or character index within the line.
    pub column: usize,
}

impl Position {
    #[must_use]
    pub fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

/// Represents a cursor and its associated selection range.
/// Uses the "Anchor and Head" directional selection model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The fixed starting point of a selection.
    pub anchor: Position,
    /// The active, moving end of a selection (where the blinking caret is).
    pub head: Position,
    /// The preferred visual column. Used to maintain horizontal position
    /// when moving vertically across shorter lines.
    pub preferred_column: Option<usize>,
}

impl Cursor {
    #[must_use]
    pub fn new(row: usize, column: usize) -> Self {
        let pos = Position::new(row, column);

        Self {
            anchor: pos,
            head: pos,
            preferred_column: Some(column),
        }
    }

    /// Creates a selection from an anchor to a head.
    #[must_use]
    pub fn new_selection(anchor: Position, head: Position) -> Self {
        Self {
            anchor,
            head,
            preferred_column: Some(head.column),
        }
    }

    /// Returns true if this is just a cursor (no text selected).
    #[inline]
    #[must_use]
    pub fn no_selection(&self) -> bool {
        self.anchor == self.head
    }

    /// Returns the top-left most position of the selection.
    /// Crucial for `TextBuffer::delete()` which expects a normalized range.
    #[inline]
    #[must_use]
    pub fn start(&self) -> Position {
        std::cmp::min(self.anchor, self.head)
    }

    /// Returns the bottom-right most position of the selection.
    #[inline]
    #[must_use]
    pub fn end(&self) -> Position {
        std::cmp::max(self.anchor, self.head)
    }

    /// Returns the normalized tuple (start, end) regardless of selection direction.
    #[inline]
    #[must_use]
    pub fn range(&self) -> (Position, Position) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Moves the head to a new position, updating the selection.
    pub fn set_head(&mut self, pos: Position) {
        self.head = pos;
        self.preferred_column = Some(pos.column);
    }

    /// Moves both anchor and head to the same position (clears selection).
    pub fn clear_selection(&mut self) {
        self.anchor = self.head;
    }

    /// Inverts the direction of the selection.
    /// Useful for advanced editing commands.
    pub fn invert(&mut self) {
        std::mem::swap(&mut self.anchor, &mut self.head);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_creation() {
        let cursor = Cursor::new(5, 10);
        assert_eq!(cursor.anchor, Position::new(5, 10));
        assert_eq!(cursor.head, Position::new(5, 10));
        assert_eq!(cursor.preferred_column, Some(10));
    }

    #[test]
    fn test_cursor_selection() {
        let anchor = Position::new(3, 5);
        let head = Position::new(6, 15);
        let cursor = Cursor::new_selection(anchor, head);

        assert_eq!(cursor.anchor, anchor);
        assert_eq!(cursor.head, head);
        assert_eq!(cursor.preferred_column, Some(15));
    }

    #[test]
    fn test_cursor_no_selection() {
        let mut cursor = Cursor::new(2, 8);

        assert!(cursor.no_selection());
        cursor.set_head(Position::new(2, 10));
        assert!(!cursor.no_selection());
        cursor.clear_selection();
        assert!(cursor.no_selection());
    }

    #[test]
    fn test_cursor_range() {
        let cursor = Cursor::new_selection(Position::new(4, 20), Position::new(2, 10));
        let (start, end) = cursor.range();

        assert_eq!(start, Position::new(2, 10));
        assert_eq!(end, Position::new(4, 20));
    }

    #[test]
    fn test_cursor_invert() {
        let mut cursor = Cursor::new_selection(Position::new(1, 5), Position::new(3, 15));

        cursor.invert();
        assert_eq!(cursor.anchor, Position::new(3, 15));
        assert_eq!(cursor.head, Position::new(1, 5));

        let (start, end) = cursor.range();

        assert_eq!(start, Position::new(1, 5));
        assert_eq!(end, Position::new(3, 15));
        // Inverting again should restore original state
        cursor.invert();
        assert_eq!(cursor.anchor, Position::new(1, 5));
        assert_eq!(cursor.head, Position::new(3, 15));

        let (start, end) = cursor.range();

        assert_eq!(start, Position::new(1, 5));
        assert_eq!(end, Position::new(3, 15));

        // Inverting a cursor with no selection should have no effect
        let mut cursor = Cursor::new(2, 8);

        cursor.invert();
        assert_eq!(cursor.anchor, Position::new(2, 8));
        assert_eq!(cursor.head, Position::new(2, 8));

        let (start, end) = cursor.range();

        assert_eq!(start, Position::new(2, 8));
        assert_eq!(end, Position::new(2, 8));
    }
}
