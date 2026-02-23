use std::ops::AddAssign;

#[derive(Debug)]
pub struct LineRangeIter<'node> {
    /// Stack tracks: (Node Reference, Index of next child/line to visit)
    pub stack: Vec<(&'node crate::line_index::node::Node, usize)>,
    pub current_line_idx: usize,
    pub end_line_idx: usize,
    pub current_abs_idx: u64,
}

impl Iterator for LineRangeIter<'_> {
    type Item = (usize, std::ops::Range<u64>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_line_idx >= self.end_line_idx || self.stack.is_empty() {
            return None;
        }

        let line_len = loop {
            let (node, idx) = *self.stack.last()?;

            match node {
                crate::line_index::node::Node::Leaf(leaf_node)
                    if idx < leaf_node.line_lengths.len() =>
                {
                    self.stack.last_mut().unwrap().1 += 1;
                    break leaf_node.line_lengths[idx];
                }
                crate::line_index::node::Node::Internal(internal_node)
                    if idx < internal_node.children.len() =>
                {
                    self.stack.push((&internal_node.children[idx], 0));
                }
                _ => {
                    self.stack.pop();

                    if let Some(parent) = self.stack.last_mut() {
                        parent.1.add_assign(1);
                    }
                }
            }
        };

        let start = self.current_abs_idx;

        self.current_abs_idx.add_assign(line_len);
        self.current_line_idx.add_assign(1);

        Some((self.current_line_idx - 1, start..self.current_abs_idx))
    }
}
