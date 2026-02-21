use crate::line_index::{
    line_summary::LineSummary,
    node::{InternalNode, LeafNode, MAX_CHILDREN, Node},
};

/// A balanced B-tree index mapping line numbers ↔ byte offsets.
///
/// # Performance
/// - **Build**: O(n) where n = number of bytes
/// - **Queries**: O(log n) where n = number of lines
/// - **Memory**: one `u64` per line, plus tree-node overhead (~64 bytes per 16-line leaf)
///
/// # Design
/// The tree is built once via [`BTreeLineIndex::build`] from a complete byte slice.
/// After document edits, rebuild with the updated content from the [`PieceTable`].
pub struct BTreeLineIndex {
    root: Option<Node>,
}

impl Default for BTreeLineIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl BTreeLineIndex {
    /// Creates an empty line index (0 lines, 0 bytes).
    #[inline]
    pub fn new() -> Self {
        Self { root: None }
    }

    /// Builds a balanced line index from a complete byte slice in O(n) time.
    ///
    /// Lines are separated by `\n`. A trailing partial line with no `\n` is
    /// included as the last entry. An empty `bytes` slice produces an empty index.
    pub fn build(bytes: &[u8]) -> Result<Self, crate::enums::MathError> {
        if bytes.is_empty() {
            return Ok(Self::new());
        }

        let line_lengths = collect_line_lengths(bytes)?;
        let root = build_balanced_tree(line_lengths);
        Ok(Self { root: Some(root) })
    }

    /// Total number of lines (including a trailing line with no newline).
    /// Returns 0 for an empty document.
    #[inline]
    pub fn line_count(&self) -> usize {
        self.root.as_ref().map_or(0, count_leaf_lines)
    }

    /// Total byte length of the indexed content.
    #[inline]
    pub fn byte_len(&self) -> u64 {
        self.root.as_ref().map_or(0, |r| r.summary().byte_len)
    }

    /// Returns the byte offset of the **start** of `line_idx` (0-indexed).
    ///
    /// Returns `None` if `line_idx >= line_count()`.
    #[inline]
    pub fn line_to_byte_offset(&self, line_idx: usize) -> Option<u64> {
        self.root
            .as_ref()
            .and_then(|r| node_line_to_byte_offset(r, line_idx))
    }

    /// Returns the 0-indexed line number that contains `byte_offset`.
    ///
    /// Returns `None` if `byte_offset >= byte_len()`.
    #[inline]
    pub fn byte_offset_to_line(&self, byte_offset: u64) -> Option<usize> {
        if byte_offset >= self.byte_len() {
            return None;
        }
        Some(
            self.root
                .as_ref()
                .map_or(0, |r| node_byte_offset_to_line(r, byte_offset)),
        )
    }
}

// ─── private helpers ──────────────────────────────────────────────────────────

/// Scans `bytes` with `memchr` and collects per-line byte lengths (including `\n`).
/// The final element covers any trailing text without a newline.
fn collect_line_lengths(bytes: &[u8]) -> Result<Vec<u64>, crate::enums::MathError> {
    let total: u64 = <usize as TryInto<u64>>::try_into(bytes.len())?;
    let mut lengths = Vec::new();
    let mut last: u64 = 0;

    for pos in memchr::memchr_iter(b'\n', bytes) {
        let after = <usize as TryInto<u64>>::try_into(pos)?
            .checked_add(1)
            .ok_or(crate::enums::MathError::Overflow)?;
        lengths.push(after - last);
        last = after;
    }

    // `collect_line_lengths` is only called when `bytes` is non-empty
    // (guarded by `build`), so we always produce at least one element here.
    // The final push covers the trailing partial line (or the empty line that
    // follows a terminal '\n', e.g. "Hi\n" → [3, 0]).
    lengths.push(total - last);
    Ok(lengths)
}

/// Builds a properly balanced tree from a flat vec of line lengths.
fn build_balanced_tree(line_lengths: Vec<u64>) -> Node {
    // Create leaf nodes, each holding at most MAX_CHILDREN line lengths.
    let leaves: Vec<Node> = line_lengths
        .chunks(MAX_CHILDREN)
        .map(|chunk| {
            let line_count = chunk.len();
            let byte_len = chunk.iter().sum();
            Node::Leaf(LeafNode {
                summary: LineSummary {
                    line_count,
                    byte_len,
                },
                line_lengths: chunk.to_vec(),
            })
        })
        .collect();

    build_level(leaves)
}

/// Collapses a flat list of nodes into one level of internal nodes, then recurses
/// until a single root remains.
fn build_level(mut nodes: Vec<Node>) -> Node {
    if nodes.len() == 1 {
        return nodes.remove(0);
    }

    let mut next: Vec<Node> = Vec::new();
    let mut chunk: Vec<Node> = Vec::with_capacity(MAX_CHILDREN);

    for node in nodes {
        chunk.push(node);
        if chunk.len() == MAX_CHILDREN {
            next.push(make_internal(std::mem::take(&mut chunk)));
        }
    }
    if !chunk.is_empty() {
        next.push(make_internal(chunk));
    }

    build_level(next)
}

fn make_internal(children: Vec<Node>) -> Node {
    if children.len() == 1 {
        return children.into_iter().next().unwrap();
    }
    let summary = LineSummary {
        line_count: children.iter().map(|n| n.summary().line_count).sum(),
        byte_len: children.iter().map(|n| n.summary().byte_len).sum(),
    };
    Node::Internal(InternalNode { summary, children })
}

/// Counts the actual number of line-length entries across all leaves.
fn count_leaf_lines(node: &Node) -> usize {
    match node {
        Node::Leaf(leaf) => leaf.line_lengths.len(),
        Node::Internal(internal) => internal.children.iter().map(count_leaf_lines).sum(),
    }
}

/// Returns the byte offset of the start of `line_idx` within this subtree,
/// or `None` if `line_idx` is out of bounds.
fn node_line_to_byte_offset(node: &Node, mut line_idx: usize) -> Option<u64> {
    match node {
        Node::Leaf(leaf) => {
            if line_idx >= leaf.line_lengths.len() {
                return None;
            }
            Some(leaf.line_lengths[..line_idx].iter().sum())
        }
        Node::Internal(internal) => {
            let mut byte_base: u64 = 0;
            for child in &internal.children {
                let child_lines = count_leaf_lines(child);
                if line_idx < child_lines {
                    return node_line_to_byte_offset(child, line_idx).map(|o| o + byte_base);
                }
                byte_base += child.summary().byte_len;
                line_idx -= child_lines;
            }
            None
        }
    }
}

/// Returns the line number (0-indexed) that contains `offset` bytes from the
/// start of this subtree.  Clamps to the last line if `offset` is past the end.
fn node_byte_offset_to_line(node: &Node, mut offset: u64) -> usize {
    match node {
        Node::Leaf(leaf) => {
            // Invariant: build_balanced_tree never produces empty leaves.
            debug_assert!(!leaf.line_lengths.is_empty());
            let mut line = 0usize;
            for &len in &leaf.line_lengths {
                if offset < len {
                    return line;
                }
                offset -= len;
                line += 1;
            }
            // `offset` was already validated against `byte_len` by the caller;
            // reaching here means we consumed all lines, so clamp to the last.
            line.saturating_sub(1)
        }
        Node::Internal(internal) => {
            // Invariant: build_balanced_tree never produces empty internal nodes.
            debug_assert!(!internal.children.is_empty());
            let mut line_base = 0usize;
            for child in &internal.children {
                let child_byte_len = child.summary().byte_len;
                if offset < child_byte_len {
                    return line_base + node_byte_offset_to_line(child, offset);
                }
                offset -= child_byte_len;
                line_base += count_leaf_lines(child);
            }
            line_base.saturating_sub(1)
        }
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index() {
        let idx = BTreeLineIndex::new();
        assert_eq!(idx.line_count(), 0);
        assert_eq!(idx.byte_len(), 0);
        assert!(idx.line_to_byte_offset(0).is_none());
        assert!(idx.byte_offset_to_line(0).is_none());
    }

    #[test]
    fn single_line_no_newline() {
        let idx = BTreeLineIndex::build(b"Hello").unwrap();
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.byte_len(), 5);
        assert_eq!(idx.line_to_byte_offset(0), Some(0));
        assert!(idx.line_to_byte_offset(1).is_none());
        assert_eq!(idx.byte_offset_to_line(0), Some(0));
        assert_eq!(idx.byte_offset_to_line(4), Some(0));
        assert!(idx.byte_offset_to_line(5).is_none());
    }

    #[test]
    fn two_lines() {
        // "Hello\nWorld" → line 0 = 6 bytes, line 1 = 5 bytes
        let idx = BTreeLineIndex::build(b"Hello\nWorld").unwrap();
        assert_eq!(idx.line_count(), 2);
        assert_eq!(idx.byte_len(), 11);
        assert_eq!(idx.line_to_byte_offset(0), Some(0));
        assert_eq!(idx.line_to_byte_offset(1), Some(6));
        assert!(idx.line_to_byte_offset(2).is_none());
        assert_eq!(idx.byte_offset_to_line(0), Some(0));
        assert_eq!(idx.byte_offset_to_line(5), Some(0));
        assert_eq!(idx.byte_offset_to_line(6), Some(1));
        assert_eq!(idx.byte_offset_to_line(10), Some(1));
        assert!(idx.byte_offset_to_line(11).is_none());
    }

    #[test]
    fn trailing_newline_produces_empty_last_line() {
        // "Hi\n" → line 0 = 3 bytes ("Hi\n"), line 1 = 0 bytes ("")
        let idx = BTreeLineIndex::build(b"Hi\n").unwrap();
        assert_eq!(idx.line_count(), 2);
        assert_eq!(idx.line_to_byte_offset(0), Some(0));
        assert_eq!(idx.line_to_byte_offset(1), Some(3));
    }

    #[test]
    fn large_document_splits_correctly() {
        // 200 lines of "X\n" (2 bytes each) → forces multiple B-tree levels
        let content: Vec<u8> = b"X\n".repeat(200);
        let idx = BTreeLineIndex::build(&content).unwrap();
        assert_eq!(idx.line_count(), 201); // 200 newlines → 201 elements
        assert_eq!(idx.byte_len(), 400);
        // Line 0 starts at byte 0
        assert_eq!(idx.line_to_byte_offset(0), Some(0));
        // Line 100 starts at byte 200
        assert_eq!(idx.line_to_byte_offset(100), Some(200));
        assert_eq!(idx.byte_offset_to_line(0), Some(0));
        assert_eq!(idx.byte_offset_to_line(200), Some(100));
        assert_eq!(idx.byte_offset_to_line(399), Some(199));
    }
}
