//! Overlay handler: transient UI state for overflow menus and friend pickers.
//!
//! These messages toggle ephemeral overlays that float above the main document
//! view. None of them mutate the block tree or trigger persistence.

use super::{AppState, Message, PanelBarState};
use crate::store::BlockId;
use iced::Task;

/// Messages for overlay and popup management.
#[derive(Debug, Clone)]
pub enum OverlayMessage {
    ToggleOverflow(BlockId),
    CloseOverflow,
    /// Toggle friends panel visibility for the given block.
    ToggleFriendsPanel(BlockId),
    /// Start picking a friend for the given target block.
    StartFriendPicker(BlockId),
    /// Cancel friend picker mode.
    CancelFriendPicker,
    /// Start inline editing the perspective for a specific friend.
    StartEditingFriendPerspective {
        target: BlockId,
        friend_id: BlockId,
    },
    /// Cancel inline editing of friend perspective.
    CancelEditingFriendPerspective,
    /// Update the input buffer while editing friend perspective.
    UpdateFriendPerspectiveInput(String),
    /// Commit the perspective input and save to store.
    CommitFriendPerspective,
}

/// Process one overlay message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: OverlayMessage) -> Task<Message> {
    match message {
        | OverlayMessage::ToggleOverflow(block_id) => {
            if state.overflow_open_for == Some(block_id) {
                state.overflow_open_for = None;
            } else {
                state.overflow_open_for = Some(block_id);
            }
            Task::none()
        }
        | OverlayMessage::CloseOverflow => {
            state.overflow_open_for = None;
            state.panel_bar_state = None;
            state.focused_block_id = None;
            Task::none()
        }
        | OverlayMessage::ToggleFriendsPanel(block_id) => {
            // Only toggle if this is the focused block
            if state.focused_block_id == Some(block_id) {
                match &state.panel_bar_state {
                    | Some(PanelBarState::Friends) => {
                        state.panel_bar_state = None;
                    }
                    | _ => {
                        state.panel_bar_state = Some(PanelBarState::Friends);
                    }
                }
            } else {
                state.panel_bar_state = Some(PanelBarState::Friends);
            }
            Task::none()
        }
        | OverlayMessage::StartFriendPicker(block_id) => {
            state.friend_picker_for = Some(block_id);
            state.overflow_open_for = None;
            Task::none()
        }
        | OverlayMessage::CancelFriendPicker => {
            state.friend_picker_for = None;
            Task::none()
        }
        | OverlayMessage::StartEditingFriendPerspective { target, friend_id } => {
            // Initialize input buffer with current perspective value
            let current_perspective = state.store.friend_blocks_for(&target)
                .iter()
                .find(|f| f.block_id == friend_id)
                .and_then(|f| f.perspective.clone())
                .unwrap_or_default();
            state.editing_friend_perspective = Some((target, friend_id));
            state.editing_friend_perspective_input = Some(current_perspective);
            Task::none()
        }
        | OverlayMessage::CancelEditingFriendPerspective => {
            state.editing_friend_perspective = None;
            state.editing_friend_perspective_input = None;
            Task::none()
        }
        | OverlayMessage::UpdateFriendPerspectiveInput(text) => {
            state.editing_friend_perspective_input = Some(text);
            Task::none()
        }
        | OverlayMessage::CommitFriendPerspective => {
            // Handled in document.rs rendering - the view constructs the message directly
            Task::none()
        }
    }
}
