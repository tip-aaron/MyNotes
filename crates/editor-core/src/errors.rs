pub type TextBufferResult<T> = Result<T, TextBufferError>;

#[derive(Debug)]
pub enum TextBufferError {
    CreationError,
    IoError(std::io::Error),
    ConversionError(std::num::TryFromIntError),
    IndexOutOfBounds(usize),
    Overflow,
}

impl From<std::io::Error> for TextBufferError {
    fn from(value: std::io::Error) -> Self {
        TextBufferError::IoError(value)
    }
}

impl From<crate::enums::MathError> for TextBufferError {
    fn from(value: crate::enums::MathError) -> Self {
        match value {
            crate::enums::MathError::ConversionFailed(val) => TextBufferError::ConversionError(val),
            crate::enums::MathError::OutOfBounds(val) => TextBufferError::IndexOutOfBounds(val),
            crate::enums::MathError::Overflow => TextBufferError::Overflow,
        }
    }
}
