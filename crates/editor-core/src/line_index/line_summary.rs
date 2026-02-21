#[derive(Clone, Copy, Debug, Default)]
pub struct LineSummary {
    pub line_count: usize,
    pub byte_len: u64,
}

impl LineSummary {
    #[allow(dead_code)]
    pub fn add(&mut self, other: &LineSummary) {
        self.line_count += other.line_count;
        self.byte_len += other.byte_len;
    }
}
