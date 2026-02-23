use std::ops::AddAssign;

#[derive(Debug)]
pub struct BTreeLineIndex {
    pub root: crate::line_index::node::Node,
    pub cache: std::cell::Cell<Option<crate::line_index::search_cache::SearchCache>>,
}

/*

====================
===== CREATION =====
====================

*/

impl BTreeLineIndex {
    fn build_leaves(
        bytes: &[u8],
    ) -> Result<Vec<crate::line_index::node::Node>, crate::enums::MathError> {
        let mut leaves = Vec::new();
        let mut current_line_lengths = Vec::with_capacity(crate::line_index::MAX_CHILDREN);
        let mut current_summary = crate::line_index::line_summary::LineSummary::default();
        let mut last_position = 0u64;

        // 1. PASS ONE: Scan the file and bulk-load the Leaves
        for line_position in memchr::memchr_iter(b'\n', bytes) {
            let next_line_position = <usize as TryInto<u64>>::try_into(line_position + 1)?;
            let len = next_line_position - last_position;

            current_line_lengths.push(len);
            current_summary.line_count.add_assign(1);
            current_summary.byte_len.add_assign(len);

            last_position = next_line_position;

            // When the leaf is perfectly full, pack it and start a new one
            if current_line_lengths.len() == crate::line_index::MAX_CHILDREN {
                leaves.push(crate::line_index::node::Node::Leaf(
                    crate::line_index::node::LeafNode {
                        summary: current_summary,
                        line_lengths: std::mem::replace(
                            &mut current_line_lengths,
                            Vec::with_capacity(crate::line_index::MAX_CHILDREN),
                        ),
                    },
                ));
                // Reset summary for the next leaf
                current_summary = crate::line_index::line_summary::LineSummary::default();
            }
        }

        let bytes_len = <usize as TryInto<u64>>::try_into(bytes.len())?;

        // Handle the trailing text after the last newline
        if last_position < bytes_len {
            let len = bytes_len - last_position;

            current_line_lengths.push(len);
            current_summary.line_count.add_assign(1);
            current_summary.byte_len.add_assign(len);
        }

        // Push any remaining lengths as the final leaf
        if !current_line_lengths.is_empty() {
            leaves.push(crate::line_index::node::Node::Leaf(
                crate::line_index::node::LeafNode {
                    summary: current_summary,
                    line_lengths: current_line_lengths,
                },
            ));
        }

        Ok(leaves)
    }

    fn build_tree(
        mut current_level: Vec<crate::line_index::node::Node>,
    ) -> Result<crate::line_index::node::Node, crate::enums::MathError> {
        while current_level.len() > 1 {
            let chunk_count = current_level
                .len()
                .div_ceil(crate::line_index::MAX_CHILDREN);
            let mut next_level = Vec::with_capacity(chunk_count);
            let mut iter = current_level.into_iter();

            for _ in 0..chunk_count {
                let chunk: Vec<crate::line_index::node::Node> = iter
                    .by_ref()
                    .take(crate::line_index::MAX_CHILDREN)
                    .collect();
                let internal_summary = chunk.iter().fold(
                    crate::line_index::line_summary::LineSummary::default(),
                    |mut acc, child| {
                        acc.add(child.summary());

                        acc
                    },
                );

                next_level.push(crate::line_index::node::Node::Internal(
                    crate::line_index::node::InternalNode {
                        summary: internal_summary,
                        children: chunk,
                    },
                ));
            }

            current_level = next_level;
        }

        current_level
            .pop()
            .ok_or(crate::enums::MathError::OutOfBounds(0))
    }

    #[allow(unused)]
    pub fn new_empty() -> Self {
        Self {
            root: crate::line_index::node::Node::Leaf(crate::line_index::node::LeafNode::default()),
            cache: std::cell::Cell::new(None),
        }
    }

    pub fn new(bytes: &[u8]) -> Result<Self, crate::enums::MathError> {
        if bytes.is_empty() {
            return Ok(Self {
                root: crate::line_index::node::Node::Leaf(
                    crate::line_index::node::LeafNode::default(),
                ),
                cache: std::cell::Cell::new(None),
            });
        }

        let leaves = Self::build_leaves(bytes)?;
        let tree = if leaves.is_empty() {
            crate::line_index::node::Node::Leaf(crate::line_index::node::LeafNode::default())
        } else {
            Self::build_tree(leaves)?
        };

        Ok(Self {
            root: tree,
            cache: std::cell::Cell::new(None),
        })
    }
}

/*

=====================
===== INSERTION =====
=====================

*/

impl BTreeLineIndex {
    pub fn insert(&mut self, byte_pos: u64, bytes: &[u8]) -> Result<(), crate::enums::MathError> {
        if bytes.is_empty() {
            return Ok(());
        }

        if let Some(new_sibling) = self.root.add_child(byte_pos, bytes)? {
            let mut new_children = Vec::with_capacity(2);
            let old_root = std::mem::replace(
                &mut self.root,
                crate::line_index::node::Node::Leaf(crate::line_index::node::LeafNode::default()),
            );
            let mut new_summary = *old_root.summary();

            new_summary.add(new_sibling.summary());
            new_children.push(old_root);
            new_children.push(new_sibling);

            self.root =
                crate::line_index::node::Node::Internal(crate::line_index::node::InternalNode {
                    summary: new_summary,
                    children: new_children,
                });
        }

        self.cache.set(None);

        Ok(())
    }
}

/*

======================
======= GETTER =======
======================

*/

impl BTreeLineIndex {
    pub fn get_line_length_at(&self, line_idx: usize) -> Option<u64> {
        self.root.get_line_length_at(line_idx)
    }

    pub fn line_idx_to_abs_idx(&self, line_idx: usize, bust_cache: bool) -> Option<u64> {
        if !bust_cache
            && let Some(cache) = self.cache.get()
            && cache.line_idx == line_idx
        {
            return Some(cache.byte_offset);
        }

        let result = self.root.line_idx_to_abs_idx(line_idx)?;

        self.cache
            .set(Some(crate::line_index::search_cache::SearchCache {
                line_idx,
                byte_offset: result,
            }));

        Some(result)
    }

    pub fn abs_idx_to_line_idx(&self, abs_idx: u64, bust_cache: bool) -> Option<usize> {
        if !bust_cache
            && let Some(cache) = self.cache.get()
            && cache.byte_offset == abs_idx
        {
            return Some(cache.line_idx);
        }

        let result = self.root.abs_idx_to_line_idx(abs_idx)?;

        self.cache
            .set(Some(crate::line_index::search_cache::SearchCache {
                line_idx: result,
                byte_offset: abs_idx,
            }));

        Some(result)
    }

    pub fn lines(
        &self,
        start_line: usize,
        end_line: usize,
    ) -> crate::line_index::line_iter::LineRangeIter<'_> {
        // A B-Tree of 1,000,000 lines is only ~6 levels deep.
        // Pre-allocating 8 is incredibly memory efficient.
        let mut stack = Vec::with_capacity(8);
        let mut current_abs_idx = 0u64;
        let mut target_line = start_line;

        self.root
            .lines(&mut target_line, &mut current_abs_idx, &mut stack);

        crate::line_index::line_iter::LineRangeIter {
            stack,
            current_line_idx: target_line,
            end_line_idx: end_line,
            current_abs_idx,
        }
    }
}

/*

========================
======= DELETION =======
========================

*/

impl BTreeLineIndex {
    pub fn remove(&mut self, abs_idx: u64, len: u64) -> Result<(), crate::enums::MathError> {
        if len == 0 {
            return Ok(());
        }

        let deletion_end = abs_idx
            .checked_add(len)
            .expect("CRASH 1: deletion_end overflowed");
        // 1. Find the lines
        let start_line = self
            .abs_idx_to_line_idx(abs_idx, true)
            .expect("CRASH 2: abs_idx_to_line_idx returned None for start_line");
        let end_line = self
            .abs_idx_to_line_idx(deletion_end, true)
            .expect("CRASH 3: abs_idx_to_line_idx returned None for end_line");
        // 2. Find the exact byte offsets for those lines
        let start_line_byte = self
            .line_idx_to_abs_idx(start_line, true)
            .expect("CRASH 4: line_idx_to_abs_idx returned None for start_line_byte");
        let end_line_byte = self
            .line_idx_to_abs_idx(end_line, true)
            .expect("CRASH 5: line_idx_to_abs_idx returned None for end_line_byte");
        let end_line_len = self
            .get_line_length_at(end_line)
            .expect("CRASH 6: get_line_length_at returned None");
        // 3. Prefix length
        let prefix_len = abs_idx
            .checked_sub(start_line_byte)
            .expect("CRASH 7: prefix_len underflowed");
        // 4. Suffix length
        let end_line_total_bytes = end_line_byte
            .checked_add(end_line_len)
            .expect("CRASH 8: end_line_total_bytes overflowed");
        let suffix_len = end_line_total_bytes
            .checked_sub(deletion_end)
            .expect("CRASH 9: suffix_len underflowed");
        let new_merged_len = prefix_len
            .checked_add(suffix_len)
            .expect("CRASH 10: new_merged_len overflowed");

        // 5. Apply the updates
        self.root.set_line_length(start_line, new_merged_len)?;

        if start_line < end_line {
            self.root.remove_line_range(
                start_line
                    .checked_add(1)
                    .ok_or(crate::enums::MathError::Overflow)?,
                end_line,
            )?;
        }

        self.cache.set(None);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CREATION TESTS ---

    #[test]
    fn test_new_empty() {
        let btree = BTreeLineIndex::new(b"").expect("Failed to create empty btree");

        // An empty tree should still technically have 1 line (line 0) with 0 length
        assert_eq!(btree.get_line_length_at(0), Some(0));
        assert_eq!(btree.line_idx_to_abs_idx(0, false), Some(0));
        assert_eq!(btree.abs_idx_to_line_idx(0, false), Some(0));
    }

    #[test]
    fn test_new_single_line() {
        let text = b"Hello, World!";
        let btree = BTreeLineIndex::new(text).expect("Failed to create btree");

        // Line 0 should be the exact length of the text
        assert_eq!(btree.get_line_length_at(0), Some(13));
        assert_eq!(btree.line_idx_to_abs_idx(0, false), Some(0));

        // Out of bounds checks
        assert_eq!(btree.get_line_length_at(1), None);
        assert_eq!(btree.line_idx_to_abs_idx(1, false), None);
    }

    #[test]
    fn test_new_multiple_lines() {
        // "Line1\n" (6 bytes)
        // "Line2\n" (6 bytes)
        // "End"     (3 bytes)
        // Total: 15 bytes, 3 lines
        let text = b"Line1\nLine2\nEnd";
        let btree = BTreeLineIndex::new(text).expect("Failed to create btree");

        // Check line lengths
        assert_eq!(btree.get_line_length_at(0), Some(6));
        assert_eq!(btree.get_line_length_at(1), Some(6));
        assert_eq!(btree.get_line_length_at(2), Some(3));

        // Check line to absolute index (byte offsets)
        assert_eq!(btree.line_idx_to_abs_idx(0, false), Some(0));
        assert_eq!(btree.line_idx_to_abs_idx(1, false), Some(6));
        assert_eq!(btree.line_idx_to_abs_idx(2, false), Some(12));

        // Check absolute index to line number
        assert_eq!(btree.abs_idx_to_line_idx(0, false), Some(0)); // Start of line 0
        assert_eq!(btree.abs_idx_to_line_idx(5, false), Some(0)); // '\n' of line 0
        assert_eq!(btree.abs_idx_to_line_idx(6, false), Some(1)); // Start of line 1
        assert_eq!(btree.abs_idx_to_line_idx(14, false), Some(2)); // 'd' in End
    }

    #[test]
    fn test_new_trailing_newline() {
        // "A\n" (2 bytes)
        // ""    (0 bytes - the empty line after the newline)
        let text = b"A\n";
        let btree = BTreeLineIndex::new(text).expect("Failed to create btree");

        assert_eq!(btree.get_line_length_at(0), Some(2));
        assert_eq!(btree.get_line_length_at(1), None); // Crucial edge case for text editors!
    }

    // --- CACHING TESTS ---
    #[test]
    fn test_cache_population_and_busting() {
        let btree = BTreeLineIndex::new(b"a\nb\nc\n").expect("Failed to create btree");

        // Cache should be empty initially
        assert!(btree.cache.get().is_none());

        // --- 1. Trigger normal cache population ---
        assert_eq!(btree.line_idx_to_abs_idx(1, false), Some(2));

        let cache_val = btree.cache.get().expect("Cache should be populated");
        assert_eq!(cache_val.line_idx, 1);
        assert_eq!(cache_val.byte_offset, 2);

        // --- 2. Poison the cache to prove `bust_cache = false` is working ---
        // We inject a fake byte_offset for line 1 to see if the getter blindly trusts it.
        btree
            .cache
            .set(Some(crate::line_index::search_cache::SearchCache {
                line_idx: 1,
                byte_offset: 999, // FAKE OFFSET
            }));

        // Because bust_cache is false, it should hit the cache and return our fake value
        assert_eq!(btree.line_idx_to_abs_idx(1, false), Some(999));

        // --- 3. Test `bust_cache = true` ---
        // This should ignore the fake 999, traverse the tree to find the real offset (2),
        // return it, AND overwrite the poisoned cache with the correct data.
        assert_eq!(btree.line_idx_to_abs_idx(1, true), Some(2));

        // --- 4. Verify the cache was repaired ---
        let fixed_cache = btree.cache.get().unwrap();
        assert_eq!(fixed_cache.line_idx, 1);
        assert_eq!(fixed_cache.byte_offset, 2); // The 999 should be gone!
    }

    // --- INSERTION TESTS ---

    #[test]
    fn test_insert_clears_cache() {
        let mut btree = BTreeLineIndex::new(b"hello").expect("Failed to create btree");

        // Populate cache
        btree.line_idx_to_abs_idx(0, false);
        assert!(btree.cache.get().is_some());

        // Insert should invalidate the cache
        btree.insert(5, b" world").expect("Failed to insert");
        assert!(btree.cache.get().is_none());
    }

    // Note: To fully test `insert`, you will need to verify that your `Node::add_child`
    // correctly updates internal `line_lengths` and `LineSummary` sizes.

    // --- Helper to easily check line lengths ---
    fn assert_line_len(btree: &BTreeLineIndex, line: usize, expected: u64) {
        assert_eq!(
            btree.get_line_length_at(line),
            Some(expected),
            "Line {line} length mismatch"
        );
    }

    #[test]
    fn test_remove_zero_len() {
        let mut btree = BTreeLineIndex::new(b"123\n456\n").unwrap();

        // Deleting 0 bytes should do absolutely nothing
        btree.remove(2, 0).expect("Remove failed");

        assert_line_len(&btree, 0, 4);
        assert_line_len(&btree, 1, 4);
    }

    #[test]
    fn test_remove_within_single_line() {
        let mut btree = BTreeLineIndex::new(b"Hello\nWorld\n").unwrap();

        // "Hello\n" is 6 bytes.
        // We delete 2 bytes starting at index 1 (removes "el").
        // Result should be "Hlo\n" (4 bytes).
        btree.remove(1, 2).unwrap();

        assert_line_len(&btree, 0, 4);
        assert_line_len(&btree, 1, 6); // Line 1 is untouched
    }

    #[test]
    fn test_remove_merge_two_lines() {
        let mut btree = BTreeLineIndex::new(b"A\nB\n").unwrap();

        // Line 0 is "A\n" (2 bytes). Line 1 is "B\n" (2 bytes).
        // We delete 1 byte at index 1 (which is the '\n').
        // The text becomes "AB\n". Lines 0 and 1 merge!
        btree.remove(1, 1).unwrap();

        assert_line_len(&btree, 0, 3);
        assert_eq!(btree.get_line_length_at(1), None); // Line 1 should be gone!
    }

    #[test]
    fn test_remove_multi_line_span() {
        let mut btree = BTreeLineIndex::new(b"Line1\nLine2\nLine3\n").unwrap();

        // Lines are 6 bytes each.
        // Index 4 is the '1' in "Line1\n".
        // We delete 8 bytes. This removes "1\nLine2\n".
        // The surviving text is "Line" (4 bytes) + "Line3\n" (6bytes) = 10 bytes.
        btree.remove(4, 8).unwrap();

        assert_line_len(&btree, 0, 10);
        assert_eq!(btree.get_line_length_at(1), None); // Lines 1 and 2 merged into 0
    }

    #[test]
    fn test_remove_stress_test() {
        // Build a tree with 1,000 lines, 10 bytes each (10,000 bytes total)
        let mut text = Vec::with_capacity(10000);
        for _ in 0..1000 {
            text.extend_from_slice(b"123456789\n");
        }
        let mut btree = BTreeLineIndex::new(&text).unwrap();

        // Delete exactly 500 lines worth of text (5,000 bytes)
        // starting from the middle of line 200 (index 2005)
        btree.remove(2005, 5000).unwrap();

        // Lines 0 to 199 should be completely untouched
        assert_line_len(&btree, 0, 10);
        assert_line_len(&btree, 199, 10);

        // Line 200 is the merged line.
        // It keeps 5 bytes from line 200, and 5 bytes from line 700. Total = 10 bytes.
        assert_line_len(&btree, 200, 10);

        // We deleted 500 lines. The total line count should now be 500.
        // Therefore, line index 499 is the last valid line.
        assert_line_len(&btree, 499, 10);
        assert_eq!(btree.get_line_length_at(500), None);
    }
}
