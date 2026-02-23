#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Original,
    Add,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditAction {
    Insert { pos: u64, text: String },
    Delete { pos: u64, text: String },
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
