//! Generic linear undo/redo history.

/// Linear undo/redo stack.
///
/// The current (live) state is NOT stored in the stack. When the user
/// undoes, the current state is pushed as a redo entry and the top undo
/// entry becomes live. New mutations discard the redo future.
#[derive(Clone)]
pub struct UndoHistory<T> {
    undo_stack: Vec<T>,
    redo_stack: Vec<T>,
    capacity: usize,
}

impl<T> UndoHistory<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self { undo_stack: Vec::new(), redo_stack: Vec::new(), capacity }
    }

    pub fn push(&mut self, snapshot: T) {
        if self.undo_stack.len() >= self.capacity {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo_stack.pop()?;
        self.redo_stack.push(current);
        Some(previous)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push(current);
        Some(next)
    }
}
