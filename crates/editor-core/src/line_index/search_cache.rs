#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct SearchCache {
    pub line_idx: usize,
    pub byte_offset: u64,
}
