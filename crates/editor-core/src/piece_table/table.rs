use std::ops::{AddAssign, SubAssign};

#[derive(Debug)]
pub struct PieceTable {
    /// Original unchanged piece_table (shared, zero-copy).
    pub original: io::mmap::MmapFile,
    /// Append-only buffer storing piece_table to be inserted.
    pub buf: Vec<u8>,
    /// Ordered list of pieces describing the visible document.
    pub pieces: Vec<crate::piece_table::piece::Piece>,

    pub undo_stack: Vec<crate::enums::Edit>,
    pub redo_stack: Vec<crate::enums::Edit>,
}

pub trait SliceOfWithStartEnd {
    fn slice_of(
        &self,
        piece: &crate::piece_table::piece::Piece,
        start: u64,
        end: u64,
    ) -> Result<&[u8], crate::enums::MathError>;
}

pub trait SliceOf {
    fn slice_of(
        &self,
        piece: &crate::piece_table::piece::Piece,
    ) -> Result<&[u8], crate::enums::MathError>;
}

/*

====================================
========= CREATION METHOD ==========
====================================

*/

impl PieceTable {
    pub fn new(mmap_file: io::mmap::MmapFile) -> Result<Self, crate::enums::MathError> {
        let mut pieces = Vec::new();

        if !mmap_file.is_empty() {
            pieces.push(crate::piece_table::piece::Piece {
                buf_kind: crate::enums::BufferKind::Original,
                range: 0..<usize as TryInto<u64>>::try_into(mmap_file.len())?,
            });
        }

        Ok(Self {
            original: mmap_file,
            buf: Vec::new(),
            pieces,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        })
    }
}

/*

====================================
========= INLINE METHODS  ==========
====================================

*/

impl PieceTable {
    /// Total document length in bytes
    #[inline]
    pub fn len(&self) -> u64 {
        self.pieces.iter().map(super::piece::Piece::len).sum()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty() || self.len() == 0
    }

    #[inline]
    pub fn locate(&self, mut pos: u64) -> (usize, u64) {
        for (idx, piece) in self.pieces.iter().enumerate() {
            let piece_len = piece.len();

            if pos <= piece_len {
                return (idx, pos);
            }

            pos.sub_assign(piece_len);
        }

        (self.pieces.len(), 0)
    }
}

impl SliceOfWithStartEnd for PieceTable {
    #[inline]
    fn slice_of(
        &self,
        piece: &crate::piece_table::piece::Piece,
        start: u64,
        end: u64,
    ) -> Result<&[u8], crate::enums::MathError> {
        let s = <u64 as TryInto<usize>>::try_into(start)?;
        let e = <u64 as TryInto<usize>>::try_into(end)?;

        match piece.buf_kind {
            crate::enums::BufferKind::Original => Ok(self.original.get_bytes_clamped(s, e)),
            crate::enums::BufferKind::Add => Ok(&self.buf[s..e]),
        }
    }
}

impl SliceOf for PieceTable {
    #[inline]
    fn slice_of(
        &self,
        piece: &crate::piece_table::piece::Piece,
    ) -> Result<&[u8], crate::enums::MathError> {
        let start = <u64 as TryInto<usize>>::try_into(piece.range.start)?;
        let end = <u64 as TryInto<usize>>::try_into(piece.range.end)?;

        match piece.buf_kind {
            crate::enums::BufferKind::Original => Ok(self.original.get_bytes_clamped(start, end)),
            crate::enums::BufferKind::Add => Ok(&self.buf[start..end]),
        }
    }
}

/*

=====================================
========= INSERT / DELETE  ==========
=====================================

*/

impl PieceTable {
    fn merge_or_continue(
        &mut self,
        idx: usize,
        offset: u64,
        buf_kind: crate::enums::BufferKind,
        range: std::ops::Range<u64>,
    ) -> bool {
        let pieces_len = self.pieces.len();
        let prev_idx = if idx == pieces_len || offset == 0 {
            idx.checked_sub(1)
        } else if offset == self.pieces[idx].len() {
            Some(idx)
        } else {
            None
        };

        if let Some(prev) = prev_idx.and_then(|i| self.pieces.get_mut(i))
            && prev.buf_kind == buf_kind
            && prev.range.end == range.start
        {
            prev.range.end = range.end;

            return false;
        }

        true
    }

    fn insert_no_history(
        &mut self,
        pos: u64,
        range: std::ops::Range<u64>,
        buf_kind: crate::enums::BufferKind,
    ) -> Result<(), crate::enums::MathError> {
        let (idx, offset) = self.locate(pos);

        if !self.merge_or_continue(idx, offset, buf_kind, range.clone()) {
            return Ok(());
        }

        let new_piece = crate::piece_table::piece::Piece {
            buf_kind,
            range: range.clone(),
        };

        if idx == self.pieces.len() {
            self.pieces.push(new_piece);

            return Ok(());
        }

        if offset == 0 {
            self.pieces.insert(idx, new_piece);

            return Ok(());
        }

        let piece = self.pieces[idx].clone();

        if offset == piece.len() {
            self.pieces.insert(idx + 1, new_piece);

            return Ok(());
        }

        let start_plus_offset = piece
            .range
            .start
            .checked_add(offset)
            .ok_or(crate::enums::MathError::Overflow)?;

        if start_plus_offset > piece.range.end {
            return Err(crate::enums::MathError::Overflow);
        }

        self.pieces.splice(
            idx..=idx,
            [
                crate::piece_table::piece::Piece {
                    buf_kind: piece.buf_kind,
                    range: piece.range.start..start_plus_offset,
                },
                new_piece,
                crate::piece_table::piece::Piece {
                    buf_kind: piece.buf_kind,
                    range: start_plus_offset..piece.range.end,
                },
            ],
        );

        Ok(())
    }

    pub fn insert(&mut self, pos: u64, bytes: &[u8]) -> Result<(), crate::enums::MathError> {
        if bytes.is_empty() {
            return Ok(());
        }

        let start = <usize as TryInto<u64>>::try_into(self.buf.len())?;
        let bytes_len = bytes.len();
        let end = start
            .checked_add(<usize as TryInto<u64>>::try_into(bytes_len)?)
            .ok_or(crate::enums::MathError::Overflow)?;

        self.buf.extend_from_slice(bytes);
        self.insert_no_history(pos, start..end, crate::enums::BufferKind::Add)?;
        self.undo_stack.push(crate::enums::Edit::Insert {
            pos,
            range: start..end,
        });
        self.redo_stack.clear();

        Ok(())
    }

    fn delete_no_history(
        &mut self,
        pos: u64,
        mut len: u64,
    ) -> Result<Vec<crate::piece_table::piece::Piece>, crate::enums::MathError> {
        let (mut idx, mut offset) = self.locate(pos);
        let mut removed = Vec::new();
        let mut pieces_len = self.pieces.len();

        while len > 0 && idx < pieces_len {
            let piece = self.pieces[idx].clone();
            #[allow(clippy::similar_names)]
            let piece_len = self.pieces[idx].len();
            let delete_start = offset;
            let delete_end = (offset + len).min(piece_len);
            let remove_len = delete_end - delete_start;

            if delete_start == 0 && delete_end == piece_len {
                // Full delete: just drop the piece
                removed.push(self.pieces[idx].clone());
                self.pieces.remove(idx);

                pieces_len.sub_assign(1);
            } else if delete_start == 0 {
                // Delete start: shrink the piece from the left
                removed.push(crate::piece_table::piece::Piece {
                    buf_kind: piece.buf_kind,
                    range: piece.range.start..piece.range.start + remove_len,
                });

                self.pieces
                    .get_mut(idx)
                    .expect("idx is already being checked")
                    .range
                    .start
                    .add_assign(remove_len);

                // Don't increment idx here because the current piece shifted left
            } else if delete_end == piece_len {
                // Delete end: shrink the piece from the right
                let new_start = piece
                    .range
                    .end
                    .checked_sub(piece.range.end - remove_len)
                    .ok_or(crate::enums::MathError::Overflow)?;

                removed.push(crate::piece_table::piece::Piece {
                    buf_kind: piece.buf_kind,
                    range: new_start..piece.range.end,
                });
                self.pieces
                    .get_mut(idx)
                    .expect("idx is already being checked")
                    .range
                    .end
                    .sub_assign(remove_len);
                idx.add_assign(1);
            } else {
                let new_start_removed = piece
                    .range
                    .start
                    .checked_add(delete_start)
                    .ok_or(crate::enums::MathError::Overflow)?;

                removed.push(crate::piece_table::piece::Piece {
                    buf_kind: piece.buf_kind,
                    range: new_start_removed..delete_end,
                });
                self.pieces.splice(
                    idx..=idx,
                    [
                        crate::piece_table::piece::Piece {
                            buf_kind: piece.buf_kind,
                            range: piece.range.start..delete_start,
                        },
                        crate::piece_table::piece::Piece {
                            buf_kind: piece.buf_kind,
                            range: delete_end..piece.range.end,
                        },
                    ],
                );

                pieces_len.sub_assign(1);
                idx.add_assign(1); // Move past the 'left' piece we just kept
            }

            len.sub_assign(remove_len);
            offset = 0;
        }

        Ok(removed)
    }

    pub fn delete(&mut self, pos: u64, len: u64) -> Result<(), crate::enums::MathError> {
        if len == 0 {
            return Ok(());
        }

        let removed = self.delete_no_history(pos, len)?;

        self.undo_stack
            .push(crate::enums::Edit::Delete { pos, len, removed });
        self.redo_stack.clear();

        Ok(())
    }
}

/*

====================================
=========== UNDO / REDO ============
====================================

*/

impl PieceTable {
    pub fn undo(&mut self) -> Result<(), crate::enums::MathError> {
        let Some(cmd) = self.undo_stack.pop() else {
            return Ok(());
        };

        match &cmd {
            crate::enums::Edit::Insert { pos, range, .. } => {
                self.delete_no_history(*pos, range.end - range.start)?;
                self.redo_stack.push(cmd);
            }
            crate::enums::Edit::Delete { pos, removed, .. } => {
                let mut delete_position = *pos;

                for piece in removed {
                    self.insert_no_history(delete_position, piece.range.clone(), piece.buf_kind)?;
                    delete_position.add_assign(piece.len());
                }

                self.redo_stack.push(cmd);
            }
        }

        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), crate::enums::MathError> {
        let Some(cmd) = self.redo_stack.pop() else {
            return Ok(());
        };

        match &cmd {
            crate::enums::Edit::Insert { pos, range, .. } => {
                self.insert_no_history(*pos, range.clone(), crate::enums::BufferKind::Add)?;
                self.undo_stack.push(cmd);
            }
            crate::enums::Edit::Delete { pos, len, .. } => {
                let removed = self.delete_no_history(*pos, *len)?;

                self.undo_stack.push(crate::enums::Edit::Delete {
                    pos: *pos,
                    len: *len,
                    removed,
                });
            }
        }

        Ok(())
    }
}

/*

====================================
========== MISCELLANEOUS ===========
====================================

*/

impl PieceTable {
    pub fn get_bytes_at(
        &self,
        mut pos: u64,
        mut len: u64,
    ) -> Result<Vec<u8>, crate::enums::MathError> {
        let mut res = Vec::with_capacity(<u64 as TryInto<usize>>::try_into(len)?);

        for piece in &self.pieces {
            let piece_len = piece.len();

            if pos >= piece_len {
                pos.sub_assign(piece_len);

                continue;
            }

            let start = piece.range.start + pos;
            let take = (piece_len - pos).min(len);

            res.extend_from_slice(SliceOfWithStartEnd::slice_of(
                self,
                piece,
                start,
                start + take,
            )?);

            len.sub_assign(take);

            if len == 0 {
                break;
            }

            pos = 0;
        }

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    fn pt_from_str(s: &str) -> crate::piece_table::table::PieceTable {
        let mut temp_file = tempfile::NamedTempFile::new().expect("could not create temp file");

        write!(temp_file, "{s}").expect("could not write");

        let path = temp_file.into_temp_path();

        crate::piece_table::table::PieceTable::new(io::mmap::MmapFile::open(path).unwrap()).unwrap()
    }

    #[test]
    fn new_len_matches_original() {
        let pt = pt_from_str("hello");

        assert_eq!(pt.len(), 5);
    }

    #[test]
    fn insert_middle() {
        let mut pt = pt_from_str("helo");

        pt.insert(3, b"l").unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"hello");
    }

    #[test]
    fn insert_start_end() {
        let mut pt = pt_from_str("world");

        pt.insert(0, b"hello ").unwrap();
        pt.insert(pt.len(), b"!").unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"hello world!");
    }

    #[test]
    fn delete_middle() {
        let mut pt = pt_from_str("hello cruel world");

        pt.delete(5, 6).unwrap();

        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"hello world");
    }

    #[test]
    fn undo_redo_insert() {
        let mut pt = pt_from_str("abc");

        pt.insert(1, b"X").unwrap();
        pt.undo().unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"abc");
        pt.redo().unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"aXbc");
    }

    #[test]
    fn undo_redo_delete() {
        let mut pt = pt_from_str("abcdef");

        pt.delete(2, 2).unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"abef");
        pt.undo().unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"abcdef");
        pt.redo().unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"abef");
    }

    #[test]
    fn test_undo_redo_multiple_inserts() {
        let mut pt = pt_from_str(""); // Start with an empty document

        // 1. Insert "Hello" (length 5)
        // to_add_buf now contains: "Hello"
        pt.insert(0, b"Hello").unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"Hello");
        // 2. Insert "World" (length 5)
        // to_add_buf now contains: "HelloWorld"
        pt.insert(5, b"World").unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"HelloWorld");
        // 3. Undo "World"
        pt.undo().unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"Hello");
        // 4. Undo "Hello"
        pt.undo().unwrap();
        assert_eq!(pt.get_bytes_at(0, pt.len()).unwrap(), b"");
        // 5. Redo the first action ("Hello")
        // BUG REVEALED:
        // Original code took `to_add_buf.len() - len`.
        // to_add_buf is 10 bytes ("HelloWorld"). len is 5.
        // It grabs bytes 5..10, which is "World", and inserts it at pos 0!
        // The fixed code uses `range: 0..5` and correctly grabs "Hello".
        pt.redo().unwrap();
        assert_eq!(
            pt.get_bytes_at(0, pt.len()).unwrap(),
            b"Hello",
            "Failed to redo 'Hello' correctly"
        );
        // 6. Redo the second action ("World")
        pt.redo().unwrap();
        assert_eq!(
            pt.get_bytes_at(0, pt.len()).unwrap(),
            b"HelloWorld",
            "Failed to redo 'World' correctly"
        );
    }
}
