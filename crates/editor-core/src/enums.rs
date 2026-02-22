#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Original,
    Add,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Insert {
        /// The position where the insertion takes place.
        /// This starts at 0.
        pos: u64,
        /// From existing append-only buffer's length up to
        /// it plus `piece_table` length being added
        range: std::ops::Range<u64>,
    },
    Delete {
        /// The position where the deletion takes place.
        /// This starts at 0.
        pos: u64,
        /// The length of `piece_table` to be deleted
        len: u64,
        /// The characters being deleted.
        removed: Vec<crate::piece_table::piece::Piece>,
    },
}

#[derive(Debug, PartialEq)]
pub enum MathError {
    /// Wraps the specific error `TryInto` generates
    ConversionFailed(std::num::TryFromIntError),
    /// Represents the `None` case from checked math
    Overflow,
    OutOfBounds(usize),
}

impl From<std::num::TryFromIntError> for MathError {
    fn from(err: std::num::TryFromIntError) -> Self {
        MathError::ConversionFailed(err)
    }
}
