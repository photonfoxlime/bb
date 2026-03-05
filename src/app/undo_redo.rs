//! Undo/redo message handling and snapshot type.
//!
//! The undo snapshot contains the store and navigation stack; editor buffers
//! are rebuilt on restore since `text_editor::Content` is not cheaply cloneable.

use super::{AppState, Message};
use crate::store::BlockStore;
use iced::Task;

/// Snapshot of undoable application state.
///
/// Contains the store and navigation stack. Editor buffers are
/// rebuilt from the store on restore since `text_editor::Content` is
/// not cheaply cloneable with full cursor state.
///
/// # Design Decisions
///
/// ## Navigation Stack Inclusion
///
/// The navigation stack is part of the undo snapshot to maintain consistency
/// between document structure and view state. Without this, undoing a structural
/// change (e.g., deleting a block) could leave the user viewing a non-existent
/// block or an outdated view.
///
/// ## Editor Buffers Exclusion
///
/// Editor buffers (text editor content with cursor state) are intentionally
/// excluded from the snapshot. They are rebuilt from the store on restore
/// because:
/// - Full cursor state is expensive to clone
/// - Text content is derived from `BlockStore::points`
/// - Cursor position reset is acceptable UX for undo operations
#[derive(Clone)]
pub struct UndoSnapshot {
    pub store: BlockStore,
    pub navigation: super::navigation::NavigationStack,
}

/// Messages for global undo/redo operations.
#[derive(Debug, Clone)]
pub enum UndoRedoMessage {
    Undo,
    Redo,
}

/// Handle undo/redo messages.
pub fn handle(state: &mut AppState, message: UndoRedoMessage) -> Task<Message> {
    match message {
        | UndoRedoMessage::Undo => {
            let current =
                UndoSnapshot { store: state.store.clone(), navigation: state.navigation.clone() };
            if let Some(previous) = state.undo_history.undo(current) {
                tracing::info!("undo applied");
                state.restore_snapshot(previous);
            }
            Task::none()
        }
        | UndoRedoMessage::Redo => {
            let current =
                UndoSnapshot { store: state.store.clone(), navigation: state.navigation.clone() };
            if let Some(next) = state.undo_history.redo(current) {
                tracing::info!("redo applied");
                state.restore_snapshot(next);
            }
            Task::none()
        }
    }
}
