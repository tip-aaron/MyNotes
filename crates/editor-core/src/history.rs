#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    pub actions: Vec<crate::enums::EditAction>,
    pub cursor_before: u64,
    pub cursor_after: u64,
}

#[derive(Debug)]
pub struct History {
    pub undo_stack: Vec<Transaction>,
    pub redo_stack: Vec<Transaction>,
}

impl History {
    /// Records an insertion, batching it with the previous insertion if they are contiguous.
    pub fn record_insert(
        &mut self,
        pos: u64,
        text: &str,
        cursor_before: u64,
        cursor_after: u64,
    ) -> Result<(), crate::enums::MathError> {
        // Any new action invalidates the redo stack
        self.redo_stack.clear();

        if let Some(last_tx) = self.undo_stack.last_mut()
            && let Some(crate::enums::EditAction::Insert {
                pos: last_pos,
                text: last_text,
            }) = last_tx.actions.last_mut()
            && (*last_pos)
                .checked_add(<usize as TryInto<u64>>::try_into(last_text.len())?)
                .ok_or(crate::enums::MathError::Overflow)?
                == pos
        {
            // Check if the new insert is exactly at the end of the last insert
            // Batch them together!
            last_text.push_str(text);
            last_tx.cursor_after = cursor_after;

            return Ok(());
        }

        // If we couldn't batch, push a new transaction
        self.undo_stack.push(Transaction {
            actions: vec![crate::enums::EditAction::Insert {
                pos,
                text: text.to_string(),
            }],
            cursor_before,
            cursor_after,
        });

        Ok(())
    }

    /// Records a deletion, batching consecutive backspaces.
    pub fn record_delete(
        &mut self,
        pos: u64,
        deleted_text: &str,
        cursor_before: u64,
        cursor_after: u64,
    ) -> Result<(), crate::enums::MathError> {
        self.redo_stack.clear();

        if let Some(last_tx) = self.undo_stack.last_mut()
            && let Some(crate::enums::EditAction::Delete {
                pos: last_pos,
                text: last_text,
            }) = last_tx.actions.last_mut()
        {
            // Backspace batching: The new delete happens exactly BEFORE the last delete
            if pos
                .checked_add(<usize as TryInto<u64>>::try_into(deleted_text.len())?)
                .ok_or(crate::enums::MathError::Overflow)?
                == *last_pos
            {
                // Prepend the text
                *last_text = format!("{}{}", deleted_text, last_text);
                *last_pos = pos; // Move the recorded start position back
                last_tx.cursor_after = cursor_after;

                return Ok(());
            } else if pos == *last_pos {
                // Append the text
                last_text.push_str(deleted_text);
                last_tx.cursor_after = cursor_after;

                return Ok(());
            }
        }

        self.undo_stack.push(Transaction {
            actions: vec![crate::enums::EditAction::Delete {
                pos,
                text: deleted_text.to_string(),
            }],
            cursor_before,
            cursor_after,
        });

        Ok(())
    }
}

impl History {
    pub fn undo(&mut self) -> Option<Transaction> {
        let tx = self.undo_stack.pop()?;

        self.redo_stack.push(tx.clone());

        Some(tx)
    }

    pub fn redo(&mut self) -> Option<Transaction> {
        let tx = self.redo_stack.pop()?;

        self.undo_stack.push(tx.clone());

        Some(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_batching() {
        let mut history = History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };

        // User types 'H'
        history.record_insert(0, "H", 0, 1).unwrap();
        // User types 'i' immediately after
        history.record_insert(1, "i", 1, 2).unwrap();

        assert_eq!(
            history.undo_stack.len(),
            1,
            "Should batch into a single transaction"
        );

        let tx = &history.undo_stack[0];
        assert_eq!(tx.cursor_before, 0);
        assert_eq!(tx.cursor_after, 2);

        match &tx.actions[0] {
            crate::enums::EditAction::Insert { pos, text } => {
                assert_eq!(*pos, 0);
                assert_eq!(text, "Hi");
            }
            _ => panic!("Expected Insert action"),
        }
    }

    #[test]
    fn test_backspace_batching() {
        let mut history = History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };

        // User deletes 'b' at pos 1 (cursor moves 2 -> 1)
        history.record_delete(1, "b", 2, 1).unwrap();
        // User deletes 'a' at pos 0 (cursor moves 1 -> 0)
        history.record_delete(0, "a", 1, 0).unwrap();

        assert_eq!(
            history.undo_stack.len(),
            1,
            "Should batch consecutive backspaces"
        );

        let tx = &history.undo_stack[0];
        assert_eq!(tx.cursor_before, 2); // Cursor before the WHOLE batched sequence
        assert_eq!(tx.cursor_after, 0); // Cursor after the WHOLE batched sequence

        match &tx.actions[0] {
            crate::enums::EditAction::Delete { pos, text } => {
                assert_eq!(*pos, 0);
                assert_eq!(
                    text, "ab",
                    "Text should be prepended to form the full deleted string"
                );
            }
            _ => panic!("Expected Delete action"),
        }
    }

    #[test]
    fn test_forward_delete_batching() {
        let mut history = History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };

        // User presses 'Delete' key on 'a' at pos 0
        history.record_delete(0, "a", 0, 0).unwrap();
        // User presses 'Delete' key again on 'b' which shifted into pos 0
        history.record_delete(0, "b", 0, 0).unwrap();

        assert_eq!(
            history.undo_stack.len(),
            1,
            "Should batch consecutive forward deletes"
        );

        let tx = &history.undo_stack[0];
        match &tx.actions[0] {
            crate::enums::EditAction::Delete { pos, text } => {
                assert_eq!(*pos, 0);
                assert_eq!(
                    text, "ab",
                    "Text should be appended to form the full deleted string"
                );
            }
            _ => panic!("Expected Delete action"),
        }
    }

    #[test]
    fn test_undo_redo_stack_movement() {
        let mut history = History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };

        history.record_insert(0, "A", 0, 1).unwrap();

        // Undo
        let undone = history.undo().unwrap();
        assert_eq!(history.undo_stack.len(), 0);
        assert_eq!(history.redo_stack.len(), 1);

        // Redo
        let redone = history.redo().unwrap();
        assert_eq!(undone, redone);
        assert_eq!(history.undo_stack.len(), 1);
        assert_eq!(history.redo_stack.len(), 0);
    }
}
