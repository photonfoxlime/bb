//! Overlay handler: transient UI state for overflow menus.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! These messages toggle ephemeral overlays that float above the main document
//! view. None of them mutate the block tree or trigger persistence.

use super::{AppState, Message};
use crate::store::BlockId;
use iced::Task;

/// Messages for overlay and popup management.
#[derive(Debug, Clone)]
pub enum OverlayMessage {
    ToggleOverflow(BlockId),
    /// Toggle the mount header path-operations overflow menu.
    ToggleMountActionsOverflow(BlockId),
    /// Toggle the bottom-right keyboard shortcut help banner.
    ToggleShortcutHelp,
}

/// Process one overlay message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: OverlayMessage) -> Task<Message> {
    // Clear any reference-panel friend highlight when interacting with overlays.
    state.ui_mut().reference_panel.highlighted_friend_block = None;

    match message {
        | OverlayMessage::ToggleOverflow(block_id) => {
            let is_currently_open =
                state.focus().is_some_and(|s| s.block_id == block_id && s.overflow_open);
            if is_currently_open {
                state.set_overflow_open(false);
            } else {
                state.set_focus(block_id);
                state.set_overflow_open(true);
            }
            Task::none()
        }
        | OverlayMessage::ToggleMountActionsOverflow(block_id) => {
            if state.ui().mount_action_overflow_block == Some(block_id) {
                state.ui_mut().mount_action_overflow_block = None;
            } else {
                state.ui_mut().mount_action_overflow_block = Some(block_id);
            }
            Task::none()
        }
        | OverlayMessage::ToggleShortcutHelp => {
            state.ui_mut().show_shortcut_help = !state.ui().show_shortcut_help;
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_shortcut_help_flips_banner_visibility() {
        let (mut state, _) = AppState::test_state();

        assert!(!state.ui().show_shortcut_help);

        let _ = handle(&mut state, OverlayMessage::ToggleShortcutHelp);
        assert!(state.ui().show_shortcut_help);

        let _ = handle(&mut state, OverlayMessage::ToggleShortcutHelp);
        assert!(!state.ui().show_shortcut_help);
    }
}
