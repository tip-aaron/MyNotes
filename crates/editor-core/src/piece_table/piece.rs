#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub buf_kind: crate::enums::BufferKind,
    pub range: std::ops::Range<u64>,
}

impl Piece {
    #[inline]
    pub fn len(&self) -> u64 {
        self.range.end - self.range.start
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.range.start == self.range.end
    }
}
