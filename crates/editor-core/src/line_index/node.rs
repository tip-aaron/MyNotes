// These node operations are complete API surface reserved for incremental index
// updates (insert/delete without a full rebuild).  They are exercised by the
// tests below but not yet called from library code.
#![allow(dead_code)]

use std::ops::{AddAssign, Sub, SubAssign};

/// Contains all LeafNodes with a total summary of its children's summaries
#[derive(Debug)]
pub struct InternalNode {
    pub summary: crate::line_index::line_summary::LineSummary,
    pub children: Vec<Node>,
}

/// Contains the data of a line
#[derive(Debug)]
pub struct LeafNode {
    pub summary: crate::line_index::line_summary::LineSummary,
    pub line_lengths: Vec<u64>,
}

#[derive(Debug)]
pub enum Node {
    /// Contains all LeafNodes with a total summary of its children's summaries
    Internal(InternalNode),
    /// Contains the data of a line
    Leaf(LeafNode),
}

pub(super) const MAX_CHILDREN: usize = 16;

impl Node {
    /// Returns a copy
    /// of this node's `LineSummary`
    #[inline]
    pub fn summary(&self) -> &crate::line_index::line_summary::LineSummary {
        match self {
            Node::Internal(internal_node) => &internal_node.summary,
            Node::Leaf(leaf_node) => &leaf_node.summary,
        }
    }

    #[inline]
    pub fn summary_mut(&mut self) -> &mut crate::line_index::line_summary::LineSummary {
        match self {
            Node::Internal(internal_node) => &mut internal_node.summary,
            Node::Leaf(leaf_node) => &mut leaf_node.summary,
        }
    }
}

/*

=====================
===== INSERTION =====
=====================

 */

impl Node {
    #[inline]
    pub fn add_child(
        &mut self,
        abs_byte_offset: u64,
        bytes: &[u8],
    ) -> Result<Option<Node>, crate::enums::MathError> {
        match self {
            Node::Leaf(leaf_node) => leaf_node
                .add_child(abs_byte_offset, bytes)
                .map(|opt_node| opt_node.map(Node::Leaf)),
            Node::Internal(internal_node) => internal_node
                .add_child(abs_byte_offset, bytes)
                .map(|opt_node| opt_node.map(Node::Internal)),
        }
    }
}

impl LeafNode {
    /// Appends a default 0 if line_lengths is currently empty
    #[inline]
    fn default_if_empty(&mut self) {
        if self.line_lengths.is_empty() {
            self.line_lengths.push(0);
        }
    }

    pub fn add_child(
        &mut self,
        mut abs_byte_offset: u64,
        bytes: &[u8],
    ) -> Result<Option<LeafNode>, crate::enums::MathError> {
        self.default_if_empty();

        let bytes_len = <usize as TryInto<u64>>::try_into(bytes.len())?;
        let target_idx = self
            .line_lengths
            .iter()
            .position(|&line_length| {
                if abs_byte_offset < line_length {
                    return true;
                }

                abs_byte_offset.sub_assign(line_length);

                false
            })
            .unwrap_or(self.line_lengths.len().sub(1));
        let old_line_len = self.line_lengths[target_idx];
        let line_prefix_len = abs_byte_offset;
        let line_suffix_len = old_line_len.sub(abs_byte_offset);
        let mut new_lines = Vec::new();
        let mut last_line_idx = 0u64;

        // `line_idx` is the exact byte index where a `\n` was found.
        for line_idx in memchr::Memchr::new(b'\n', bytes) {
            // Calculate the length of the line.
            // `line_idx + 1` ensures we include the `\n` itself in the line's total length.
            let line_idx_ahead = <usize as TryInto<u64>>::try_into(line_idx)?
                .checked_add(1)
                .ok_or(crate::enums::MathError::Overflow)?;

            // Subtracting `last_line_idx` gives us the distance from the start of this line to the `\n`.
            new_lines.push(
                line_idx_ahead
                    .checked_sub(last_line_idx)
                    .ok_or(crate::enums::MathError::Overflow)?,
            );

            // Advance our starting cursor to the character immediately following this `\n`,
            // setting it up for the next iteration of the loop.
            last_line_idx = line_idx_ahead;
        }

        // If there are no new_lines `\n`, that means we can just
        // add the current line's length since we'd just be
        // adding to it.
        if new_lines.is_empty() {
            self.line_lengths[target_idx].add_assign(bytes_len);

            self.summary.byte_len = self
                .summary
                .byte_len
                .checked_add(bytes_len)
                .ok_or(crate::enums::MathError::Overflow)?;

            return Ok(self.split_if_needed());
        }

        // Check if there are trailing texts after `\n` that doesn't have an ending `\n`
        // For example: "Hello\nWorld", value below would be 5 for "World"
        let remaining_text_len = bytes_len
            .checked_sub(last_line_idx)
            .ok_or(crate::enums::MathError::Overflow)?;

        self.line_lengths[target_idx] = line_prefix_len
            .checked_add(new_lines[0])
            .ok_or(crate::enums::MathError::Overflow)?;

        let middle_lines = &new_lines.get(1..).unwrap_or(&[]);
        let final_new_line_len = remaining_text_len + line_suffix_len;
        // 2. Chain the iterators together. This creates a single lazy Iterator
        // yielding middle_lines followed by final_new_line_len, completely
        // avoiding the intermediate Vec allocation.
        let to_insert = middle_lines
            .iter()
            .copied()
            .chain(std::iter::once(final_new_line_len));

        self.line_lengths
            .splice(target_idx + 1..=target_idx, to_insert);

        self.summary.line_count = self.line_lengths.len();
        self.summary.byte_len = self
            .summary
            .byte_len
            .checked_add(bytes_len)
            .ok_or(crate::enums::MathError::Overflow)?;

        Ok(self.split_if_needed())
    }

    pub fn split_if_needed(&mut self) -> Option<LeafNode> {
        let line_len = self.line_lengths.len();

        if line_len <= MAX_CHILDREN {
            return None;
        }

        let mid = line_len / 2;
        let right_lengths = self.line_lengths.split_off(mid);
        let left_summary = crate::line_index::line_summary::LineSummary {
            line_count: self.line_lengths.len(),
            byte_len: self.line_lengths.iter().sum(),
        };
        self.summary = left_summary;
        let right_summary = crate::line_index::line_summary::LineSummary {
            line_count: right_lengths.len(),
            byte_len: right_lengths.iter().sum(),
        };

        Some(LeafNode {
            summary: right_summary,
            line_lengths: right_lengths,
        })
    }
}

impl InternalNode {
    pub fn add_child(
        &mut self,
        mut abs_byte_offset: u64,
        bytes: &[u8],
    ) -> Result<Option<InternalNode>, crate::enums::MathError> {
        for (idx, child) in self.children.iter_mut().enumerate() {
            let child_byte_len = child.summary().byte_len;

            if abs_byte_offset <= child_byte_len
                && let Some(new_node) = child.add_child(abs_byte_offset, bytes)?
            {
                self.children.insert(idx + 1, new_node);
                break;
            }

            abs_byte_offset.sub_assign(child_byte_len);
        }

        self.summary
            .byte_len
            .add_assign(<usize as TryInto<u64>>::try_into(bytes.len())?);
        self.summary.line_count = self.children.iter().map(|c| c.summary().line_count).sum();

        Ok(self.split_if_needed())
    }

    pub fn split_if_needed(&mut self) -> Option<InternalNode> {
        let children_len = self.children.len();

        if children_len <= MAX_CHILDREN {
            return None;
        }

        let mid = children_len / 2;
        let right_children = self.children.split_off(mid);
        let left_sum = crate::line_index::line_summary::LineSummary {
            line_count: self.children.iter().map(|c| c.summary().line_count).sum(),
            byte_len: self.children.iter().map(|c| c.summary().byte_len).sum(),
        };
        self.summary = left_sum;
        let right_sum = crate::line_index::line_summary::LineSummary {
            line_count: right_children.iter().map(|c| c.summary().line_count).sum(),
            byte_len: right_children.iter().map(|c| c.summary().byte_len).sum(),
        };

        Some(InternalNode {
            summary: right_sum,
            children: right_children,
        })
    }
}

/*

======================
======= SETTER =======
======================

 */

impl Node {
    /// Recursively finds the target line, updates its length, and fixes byte_len summaries.
    /// Returns the difference in bytes to bubble up the tree.
    #[inline]
    pub fn set_line_length(
        &mut self,
        target_line_idx: usize,
        new_len: u64,
    ) -> Result<i64, crate::enums::MathError> {
        match self {
            Node::Leaf(leaf_node) => leaf_node.set_line_length(target_line_idx, new_len),
            Node::Internal(internal_node) => {
                internal_node.set_line_length(target_line_idx, new_len)
            }
        }
    }
}

impl LeafNode {
    pub fn set_line_length(
        &mut self,
        target_line_idx: usize,
        new_len: u64,
    ) -> Result<i64, crate::enums::MathError> {
        if target_line_idx >= self.line_lengths.len() {
            return Err(crate::enums::MathError::OutOfBounds(
                self.line_lengths.len(),
            ));
        }

        let diff = <u64 as TryInto<i64>>::try_into(new_len)?
            .checked_sub(<u64 as TryInto<i64>>::try_into(
                self.line_lengths[target_line_idx],
            )?)
            .ok_or(crate::enums::MathError::Overflow)?;
        self.line_lengths[target_line_idx] = new_len;
        self.summary.byte_len = self
            .summary
            .byte_len
            .checked_add_signed(diff)
            .ok_or(crate::enums::MathError::Overflow)?;

        Ok(diff)
    }
}

impl InternalNode {
    pub fn set_line_length(
        &mut self,
        mut target_line_idx: usize,
        new_len: u64,
    ) -> Result<i64, crate::enums::MathError> {
        let mut diff = 0;

        for child in self.children.iter_mut() {
            let child_lines = child.summary().line_count;

            if target_line_idx < child_lines {
                diff = child.set_line_length(target_line_idx, new_len)?;

                break;
            }

            target_line_idx.sub_assign(child_lines);
        }

        self.summary.byte_len = self
            .summary
            .byte_len
            .checked_add_signed(diff)
            .ok_or(crate::enums::MathError::Overflow)?;

        Ok(diff)
    }
}

/*

========================
======= DELETION =======
========================

 */

impl Node {
    #[inline]
    /// Removes a range of lines (inclusive) and culls empty nodes.
    /// Returns the total bytes removed to bubble up the summaries.
    pub fn remove_line_range(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<u64, crate::enums::MathError> {
        match self {
            Node::Leaf(leaf_node) => leaf_node.remove_line_range(start, end),
            Node::Internal(internal_node) => internal_node.remove_line_range(start, end),
        }
    }
}

impl LeafNode {
    pub fn remove_line_range(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<u64, crate::enums::MathError> {
        let remove_start: usize;
        let remove_end: usize;

        {
            let line_len = self.line_lengths.len();
            remove_start = start.min(line_len);
            remove_end = (end + 1).min(line_len);
        }

        if remove_start >= remove_end {
            return Ok(0);
        }

        let removed_bytes_count = self.line_lengths.drain(remove_start..remove_end).sum();
        self.summary.line_count = self.line_lengths.len();

        self.summary.byte_len.sub_assign(removed_bytes_count);

        Ok(removed_bytes_count)
    }
}

impl InternalNode {
    pub fn remove_line_range(
        &mut self,
        mut start: usize,
        mut end: usize,
    ) -> Result<u64, crate::enums::MathError> {
        let mut idx = 0usize;
        let mut bytes_removed = 0;

        while idx < self.children.len() && start <= end {
            let child_line_count = self.children[idx].summary().line_count;

            if start >= child_line_count {
                start.sub_assign(child_line_count);
                end.sub_assign(child_line_count);
                idx.add_assign(1);

                continue;
            }

            let child_line_start = start;
            let child_line_end = end.min(child_line_count - 1);

            // Recurse into the child
            bytes_removed.add_assign(
                self.children[idx].remove_line_range(child_line_start, child_line_end)?,
            );

            if self.children[idx].summary().line_count == 0 {
                self.children.remove(idx);
            } else {
                idx.add_assign(1);
            }

            if end < child_line_count {
                break;
            }

            end.sub_assign(child_line_count);
            start = 0;
        }

        self.summary.line_count = self.children.iter().map(|c| c.summary().line_count).sum();

        self.summary.byte_len.sub_assign(bytes_removed);

        Ok(bytes_removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Adjust these imports based on your actual crate structure
    use crate::enums::MathError;
    use crate::line_index::line_summary::LineSummary;

    // --- Helper Functions ---

    fn create_empty_leaf() -> LeafNode {
        LeafNode {
            summary: LineSummary {
                line_count: 0,
                byte_len: 0,
            },
            line_lengths: Vec::new(),
        }
    }

    fn create_empty_internal() -> InternalNode {
        InternalNode {
            summary: LineSummary {
                line_count: 0,
                byte_len: 0,
            },
            children: Vec::new(),
        }
    }

    // =====================
    // ===== INSERTION =====
    // =====================

    #[test]
    fn test_leaf_add_child_no_newlines() {
        let mut leaf = create_empty_leaf();

        // Add "Hello" (5 bytes)
        let split = leaf.add_child(0, b"Hello").unwrap();

        assert!(split.is_none());
        assert_eq!(leaf.summary.line_count, 0);
        assert_eq!(leaf.summary.byte_len, 5);
        assert_eq!(leaf.line_lengths, vec![5]);
    }

    #[test]
    fn test_leaf_add_child_with_newlines() {
        let mut leaf = create_empty_leaf();

        // Add "Hello\nWorld\nRust" (16 bytes)
        let split = leaf.add_child(0, b"Hello\nWorld\nRust").unwrap();

        assert!(split.is_none());
        // "Hello\n" = 6, "World\n" = 6, "Rust" = 4
        assert_eq!(leaf.summary.byte_len, 16);
        assert_eq!(leaf.summary.line_count, 3);
        assert_eq!(leaf.line_lengths, vec![6, 6, 4]);
    }

    #[test]
    fn test_leaf_split_if_needed() {
        let mut leaf = create_empty_leaf();
        // Force a split by adding more lines than MAX_CHILDREN (16)
        // Adding 18 lines of "A\n" (2 bytes each)
        let bytes = b"A\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\n";
        let split_result = leaf.add_child(0, bytes).unwrap();

        assert!(split_result.is_some());

        let right_node = split_result.unwrap();

        // Original node keeps the left half (9 lines)
        assert_eq!(leaf.line_lengths.len(), 9);
        assert_eq!(leaf.summary.line_count, 9);
        assert_eq!(leaf.summary.byte_len, 18);
        // New right node gets the right half (10 lines, since 18 '\n' creates 19 elements including the empty remainder)
        assert_eq!(right_node.line_lengths.len(), 10);
        assert_eq!(right_node.summary.line_count, 10);
        assert_eq!(right_node.summary.byte_len, 18);
    }

    // ======================
    // ======= SETTER =======
    // ======================

    #[test]
    fn test_leaf_set_line_length() {
        let mut leaf = create_empty_leaf();
        leaf.add_child(0, b"Line1\nLine2\nLine3").unwrap();

        // line_lengths should be [6, 6, 5] (total 17)
        assert_eq!(leaf.summary.line_count, 3);
        assert_eq!(leaf.summary.byte_len, 17);

        // Change "Line2\n" (6 bytes) to 10 bytes (+4 difference)
        let diff = leaf.set_line_length(1, 10).unwrap();

        assert_eq!(diff, 4);
        assert_eq!(leaf.line_lengths[1], 10);
        assert_eq!(leaf.summary.byte_len, 21); // 17 + 4
    }

    #[test]
    fn test_leaf_set_line_length_out_of_bounds() {
        let mut leaf = create_empty_leaf();

        leaf.add_child(0, b"Line1").unwrap(); // 1 line

        // targeting index 5, but only has 1 line
        let result = leaf.set_line_length(5, 10);

        assert!(matches!(result, Err(MathError::OutOfBounds(_)) | Err(_)));
    }

    #[test]
    fn test_internal_set_line_length() {
        let mut leaf1 = create_empty_leaf();

        leaf1.add_child(0, b"A\nB\n").unwrap(); // 2 lines: [2, 2]

        assert_eq!(leaf1.summary.line_count, 3);
        assert_eq!(leaf1.summary.byte_len, 4);

        let mut leaf2 = create_empty_leaf();

        leaf2.add_child(0, b"C\nD\nE\n").unwrap(); // 3 lines: [2, 2, 2]

        assert_eq!(leaf2.summary.line_count, 4);
        assert_eq!(leaf2.summary.byte_len, 6);

        let mut internal = create_empty_internal();

        internal.children.push(Node::Leaf(leaf1));
        internal.children.push(Node::Leaf(leaf2));

        // Cascade the summaries manually since we used Vec::push instead of InternalNode::add_child
        internal.summary.line_count = internal
            .children
            .iter()
            .map(|c| c.summary().line_count)
            .sum();
        internal.summary.byte_len = internal.children.iter().map(|c| c.summary().byte_len).sum();

        // 3 lines + 4 lines = 7 lines total!
        assert_eq!(internal.summary.line_count, 7);
        assert_eq!(internal.summary.byte_len, 10);

        // Target index 3 skips leaf1 (which has indices 0, 1, 2)
        // and lands on leaf2 at index 0 (which is "C\n" with a length of 2).
        // We change length 2 to 5 (diff = +3).
        let diff = internal.set_line_length(3, 5).unwrap();

        assert_eq!(diff, 3);
        assert_eq!(internal.summary.byte_len, 13); // 10 + 3

        if let Node::Leaf(l) = &internal.children[1] {
            // Assert on index 0 of leaf2!
            assert_eq!(l.line_lengths[0], 5);
        } else {
            panic!("Expected LeafNode");
        }
    }

    // ========================
    // ======= DELETION =======
    // ========================

    #[test]
    fn test_leaf_remove_line_range() {
        let mut leaf = create_empty_leaf();

        leaf.add_child(0, b"A\nB\nC\nD\nE").unwrap();
        // Lengths: [2, 2, 2, 2, 1] -> Total 9 bytes
        assert_eq!(leaf.summary.byte_len, 9);
        assert_eq!(leaf.summary.line_count, 5);

        // Remove lines 1 to 3 inclusive ("B\n", "C\n", "D\n") -> indices 1..=3
        let removed_bytes = leaf.remove_line_range(1, 3).unwrap();

        assert_eq!(removed_bytes, 6); // 2 + 2 + 2 bytes removed
        assert_eq!(leaf.line_lengths, vec![2, 1]); // "A\n" and "E" left
        assert_eq!(leaf.summary.line_count, 2);
        assert_eq!(leaf.summary.byte_len, 3);
    }

    #[test]
    fn test_internal_remove_line_range() {
        let mut leaf1 = create_empty_leaf();

        leaf1.add_child(0, b"1\n2\n").unwrap(); // [2, 2]
        assert_eq!(leaf1.summary.byte_len, 4);
        assert_eq!(leaf1.summary.line_count, 3);

        let mut leaf2 = create_empty_leaf();

        leaf2.add_child(0, b"3\n4\n").unwrap(); // [2, 2]
        assert_eq!(leaf2.summary.byte_len, 4);
        assert_eq!(leaf2.summary.line_count, 3);

        let mut internal = create_empty_internal();
        internal.children.push(Node::Leaf(leaf1));
        internal.children.push(Node::Leaf(leaf2));

        // Cascade the summaries manually since we used Vec::push instead of InternalNode::add_child
        internal.summary.line_count = internal
            .children
            .iter()
            .map(|c| c.summary().line_count)
            .sum();
        internal.summary.byte_len = internal.children.iter().map(|c| c.summary().byte_len).sum();
        // Remove lines 1 through 3.
        // Line 1 is from leaf 1, line 3 is from leaf 2
        let removed_bytes = internal.remove_line_range(1, 3).unwrap();

        println!("{:#?}", internal);

        assert_eq!(internal.children[0].summary().line_count, 1);
        assert_eq!(removed_bytes, 4); // 2 bytes from leaf1, 2 bytes from leaf2
        assert_eq!(internal.summary.line_count, 3);
        assert_eq!(internal.summary.byte_len, 4);
        assert_eq!(internal.children.len(), 2); // Neither node became entirely empty

        if let Node::Leaf(l) = &internal.children[0] {
            assert_eq!(l.line_lengths.len(), 1);
        }
    }

    #[test]
    fn test_internal_remove_culls_empty_nodes() {
        let mut leaf1 = create_empty_leaf();

        leaf1.add_child(0, b"1\n").unwrap();
        assert_eq!(leaf1.summary.byte_len, 2);
        assert_eq!(leaf1.summary.line_count, 2);

        let mut leaf2 = create_empty_leaf();

        leaf2.add_child(0, b"2\n").unwrap();
        assert_eq!(leaf1.summary.byte_len, 2);
        assert_eq!(leaf1.summary.line_count, 2);

        let mut internal = create_empty_internal();

        internal.children.push(Node::Leaf(leaf1));
        internal.children.push(Node::Leaf(leaf2));
        internal.summary.line_count = internal
            .children
            .iter()
            .map(|c| c.summary().line_count)
            .sum();
        internal.summary.byte_len = internal.children.iter().map(|c| c.summary().byte_len).sum();

        // Remove only line 0 & 1 (empty line). This empties leaf1 entirely.
        internal.remove_line_range(0, 1).unwrap();
        assert_eq!(internal.children.len(), 1);
        assert_eq!(internal.summary.line_count, 2);

        if let Node::Leaf(l) = &internal.children[0] {
            assert_eq!(l.line_lengths.len(), 2);
            assert_eq!(l.summary.byte_len, 2);
            assert_eq!(l.line_lengths, vec![2, 0]); // The remaining "2\n"
        }
    }
}
