//! Structure handler: tree manipulation operations.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Operations that mutate the block tree topology: adding children, parents,
//! siblings, duplicating subtrees, archiving, folding, and managing friend
//! blocks used as additional LLM context.

use super::{AppState, DocumentMode, Message};
use crate::store::{BlockId, FriendBlock};
use iced::Task;

/// Messages for tree structure mutations.
#[derive(Debug, Clone)]
pub enum StructureMessage {
    AddChild(BlockId),
    AddParent(BlockId),
    AddSibling(BlockId),
    DuplicateBlock(BlockId),
    ArchiveBlock(BlockId),
    ToggleFold(BlockId),
    /// Add the given block as a friend of the target (for LLM context).
    AddFriendBlock {
        target: BlockId,
        friend_id: BlockId,
    },
    /// Remove a friend from the target's friend list.
    RemoveFriendBlock {
        target: BlockId,
        friend_id: BlockId,
    },
    /// Set the perspective for a friend block.
    SetFriendPerspective {
        target: BlockId,
        friend_id: BlockId,
        perspective: Option<String>,
    },
}

/// Process one structure message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: StructureMessage) -> Task<Message> {
    // Clear friend hover state on any structure action
    state.ui_mut().hovered_friend_block = None;

    match message {
        | StructureMessage::AddChild(block_id) => {
            state.set_overflow_open(false);
            state.mutate_with_undo_and_persist("after adding child", |state| {
                    if let Some(child_id) = state.store.append_child(&block_id, String::new()) {
                        tracing::info!(parent_block_id = ?block_id, child_block_id = ?child_id, "added child block");
                        state.editor_buffers.set_text(&child_id, "");
                        return true;
                    }
                    false
                });
            Task::none()
        }
        | StructureMessage::AddParent(block_id) => {
            state.mutate_with_undo_and_persist("after adding parent", |state| {
                if let Some(parent_id) = state.store.insert_parent(&block_id, String::new()) {
                    tracing::info!(
                        block_id = ?block_id,
                        parent_block_id = ?parent_id,
                        "added parent block"
                    );
                    state.editor_buffers.set_text(&parent_id, "");
                    state.set_overflow_open(false);
                    return true;
                }
                false
            });
            Task::none()
        }
        | StructureMessage::AddSibling(block_id) => {
            state.mutate_with_undo_and_persist("after adding sibling", |state| {
                    if let Some(sibling_id) = state.store.append_sibling(&block_id, String::new()) {
                        tracing::info!(block_id = ?block_id, sibling_block_id = ?sibling_id, "added sibling block");
                        state.editor_buffers.set_text(&sibling_id, "");
                        state.set_overflow_open(false);
                        return true;
                    }
                    false
                });
            Task::none()
        }
        | StructureMessage::DuplicateBlock(block_id) => {
            state.mutate_with_undo_and_persist("after duplicating subtree", |state| {
                    if let Some(duplicate_id) = state.store.duplicate_subtree_after(&block_id) {
                        tracing::info!(block_id = ?block_id, duplicate_block_id = ?duplicate_id, "duplicated block subtree");
                        state.editor_buffers.ensure_subtree(&state.store, &duplicate_id);
                        state.set_overflow_open(false);
                        return true;
                    }
                    false
                });
            Task::none()
        }
        | StructureMessage::ArchiveBlock(block_id) => {
            state.snapshot_for_undo();
            if let Some(removed_ids) = state.store.remove_block_subtree(&block_id) {
                tracing::info!(block_id = ?block_id, removed = removed_ids.len(), "archived block subtree");
                state.editor_buffers.remove_blocks(&removed_ids);
                for id in &removed_ids {
                    state.llm_requests.remove_block(*id);
                }
                if removed_ids.iter().any(|id| state.focus().is_some_and(|s| s.block_id == *id)) {
                    state.clear_focus();
                }
                for root_id in state.store.roots() {
                    state.editor_buffers.ensure_block(&state.store, root_id);
                }
                state.set_overflow_open(false);
                state.persist_with_context("after archiving subtree");
            }
            Task::none()
        }
        | StructureMessage::ToggleFold(block_id) => {
            state.store.toggle_collapsed(&block_id);
            state.persist_with_context("after toggling fold");
            Task::none()
        }
        | StructureMessage::AddFriendBlock { target, friend_id } => {
            state.set_overflow_open(false);
            // Need to check document_mode before mutation since it happens inside the closure
            let was_pick_friend = state.ui().document_mode == DocumentMode::PickFriend;
            state.mutate_with_undo_and_persist("after adding friend block", |state| {
                if friend_id == target {
                    return false;
                }
                if state.store.node(&target).is_none() || state.store.node(&friend_id).is_none() {
                    return false;
                }
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                if friends.iter().any(|f| f.block_id == friend_id) {
                    return false;
                }
                friends.push(FriendBlock {
                    block_id: friend_id,
                    perspective: None,
                    parent_lineage_telescope: false,
                    children_telescope: false,
                });
                state.store.set_friend_blocks_for(&target, friends);
                tracing::info!(target = ?target, friend_id = ?friend_id, "added friend block");
                true
            });
            // Exit PickFriend mode after adding a friend
            if was_pick_friend {
                state.ui_mut().document_mode = DocumentMode::Normal;
            }
            Task::none()
        }
        | StructureMessage::RemoveFriendBlock { target, friend_id } => {
            state.mutate_with_undo_and_persist("after removing friend block", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let prev = friends.len();
                friends.retain(|f| f.block_id != friend_id);
                if friends.len() == prev {
                    return false;
                }
                if friends.is_empty() {
                    state.store.set_friend_blocks_for(&target, vec![]);
                } else {
                    state.store.set_friend_blocks_for(&target, friends);
                }
                tracing::info!(target = ?target, friend_id = ?friend_id, "removed friend block");
                true
            });
            Task::none()
        }
        | StructureMessage::SetFriendPerspective { target, friend_id, perspective } => {
            state.mutate_with_undo_and_persist("after setting friend perspective", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let friend = friends.iter_mut().find(|f| f.block_id == friend_id);
                if let Some(friend) = friend {
                    friend.perspective = perspective;
                    state.store.set_friend_blocks_for(&target, friends);
                    tracing::info!(target = ?target, friend_id = ?friend_id, "set friend perspective");
                    true
                } else {
                    false
                }
            });
            Task::none()
        }
    }
}
