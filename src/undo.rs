//! Generic linear undo/redo history.

/// Linear undo/redo stack.
///
/// The current (live) state is NOT stored in the stack. When the user
/// undoes, the current state is pushed as a redo entry and the top undo
/// entry becomes live. New mutations discard the redo future.
///
/// In `AppState`, this history is fed with pre-mutation snapshots of
/// `BlockStore`; text-edit coalescing boundaries are handled in app logic.
#[derive(Clone)]
pub struct UndoHistory<T> {
    undo_stack: Vec<T>,
    redo_stack: Vec<T>,
    capacity: usize,
}

impl<T> UndoHistory<T> {
    /// Create an empty history that retains at most `capacity` undo entries.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { undo_stack: Vec::new(), redo_stack: Vec::new(), capacity }
    }

    /// Record a snapshot as the previous state before a mutation.
    ///
    /// Clears the redo stack (the user forked a new timeline).
    /// If the undo stack is at capacity, the oldest entry is dropped.
    pub fn push(&mut self, snapshot: T) {
        if self.undo_stack.len() >= self.capacity {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    /// Undo: pop the top undo entry and return it, pushing `current` onto the
    /// redo stack. Returns `None` when there is nothing to undo.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo_stack.pop()?;
        self.redo_stack.push(current);
        Some(previous)
    }

    /// Redo: pop the top redo entry and return it, pushing `current` onto the
    /// undo stack. Returns `None` when there is nothing to redo.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push(current);
        Some(next)
    }

    /// Whether an undo operation can be performed.
    ///
    /// Returns `true` when at least one snapshot exists in the undo stack.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether a redo operation can be performed.
    ///
    /// Returns `true` when at least one snapshot exists in the redo stack.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_empty() {
        let mut history = UndoHistory::<i32>::with_capacity(10);
        assert_eq!(history.undo(0), None);
    }

    #[test]
    fn redo_empty() {
        let mut history = UndoHistory::<i32>::with_capacity(10);
        assert_eq!(history.redo(0), None);
    }

    #[test]
    fn push_then_undo() {
        let mut history = UndoHistory::with_capacity(10);
        history.push(42);
        let result = history.undo(100);
        assert_eq!(result, Some(42));
    }

    #[test]
    fn push_then_undo_then_redo() {
        let mut history = UndoHistory::with_capacity(10);
        history.push(42);
        history.undo(100);
        let result = history.redo(42);
        assert_eq!(result, Some(100));
    }

    #[test]
    fn multiple_push_then_undo() {
        let mut history = UndoHistory::with_capacity(10);
        history.push(1);
        history.push(2);
        history.push(3);

        assert_eq!(history.undo(3), Some(3));
        assert_eq!(history.undo(2), Some(2));
        assert_eq!(history.undo(1), Some(1));
        assert_eq!(history.undo(0), None);
    }

    #[test]
    fn redo_after_new_push_is_discarded() {
        let mut history = UndoHistory::with_capacity(10);
        history.push(42);
        history.undo(100);
        history.push(200);
        assert_eq!(history.redo(200), None);
    }

    #[test]
    fn capacity_overflow() {
        let mut history = UndoHistory::with_capacity(2);
        history.push(1);
        history.push(2);
        history.push(3);
        assert_eq!(history.undo(3), Some(3));
        assert_eq!(history.undo(2), Some(2));
        assert_eq!(history.undo(1), None);
    }

    #[test]
    fn undo_then_redo_preserves_current() {
        let mut history = UndoHistory::with_capacity(10);
        history.push(42);
        history.undo(100);
        let result = history.redo(42);
        assert_eq!(result, Some(100));
        let result = history.undo(100);
        assert_eq!(result, Some(42));
    }

    #[test]
    fn multiple_undo_redo_cycles() {
        let mut history = UndoHistory::with_capacity(10);
        history.push(1);
        history.push(2);

        assert_eq!(history.undo(2), Some(2));
        assert_eq!(history.undo(1), Some(1));

        assert_eq!(history.redo(1), Some(1));
        assert_eq!(history.redo(2), Some(2));

        assert_eq!(history.undo(2), Some(2));
        assert_eq!(history.undo(1), Some(1));
        assert_eq!(history.undo(3), None);
    }

    #[test]
    fn capacity_one() {
        let mut history = UndoHistory::with_capacity(1);
        history.push(42);
        history.push(100);
        assert_eq!(history.undo(0), Some(100));
        assert_eq!(history.undo(1), None);
    }

    #[test]
    fn can_undo_and_redo_flags_track_stack_state() {
        let mut history = UndoHistory::with_capacity(10);

        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history.push(42);
        assert!(history.can_undo());
        assert!(!history.can_redo());

        let _ = history.undo(100);
        assert!(!history.can_undo());
        assert!(history.can_redo());

        let _ = history.redo(42);
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }
}
