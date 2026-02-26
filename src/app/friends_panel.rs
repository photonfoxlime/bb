//! Friend blocks panel for displaying user-selected related blocks.
//!
//! Friend blocks are shown per block that has at least one friend:
//! - A "Friends" panel is rendered below the block row (same pattern as
//!   expansion/reduction draft panels), listing each friend's point text and
//!   optional perspective, with a remove button per friend.
//!
//! ## Inline Perspective Editor
//!
//! Each friend in the panel has an editable "perspective" field. This is a
//! user-authored framing string that describes how the source block should
//! interpret that friend block. For example, a friend might be viewed from
//! "historical lens", "skeptical counterpoint", or "supporting evidence" perspective.
//!
//! The perspective is rendered as a secondary line below the friend's point text.
//! When empty, a localized placeholder invites the user to "add perspective...".
//! Clicking the perspective area toggles an inline text input field. On blur
//! (or Enter key), the new perspective is saved via `StructureMessage::SetFriendPerspective`.
//!
//! Design rationale:
//! - Inline editing avoids navigating to a separate modal/dialog, keeping context visible.
//! - Immediate save on blur provides instant feedback without requiring explicit save actions.
//! - Empty state with placeholder makes the affordance discoverable without cluttering the UI.

use crate::app::{AppState, DocumentMode, Message, StructureMessage};
use crate::store::{BlockId, PanelBarState};
use crate::theme;

use iced::Element;
use iced::widget::{button, column, container, row, text, text_input};
use iced::Length;
use iced::Task;

/// Message types for friends panel interactions.
#[derive(Debug, Clone)]
pub enum FriendPanelMessage {
    /// Toggle friends panel visibility for the given block.
    Toggle(BlockId),
    /// Start picking a friend for the given target block.
    StartFriendPicker(BlockId),
    /// Cancel friend picker mode.
    CancelFriendPicker,
    /// Start inline editing the perspective for a specific friend.
    StartEditingFriendPerspective {
        target: BlockId,
        friend_id: BlockId,
    },
    /// Cancel inline editing of friend perspective (uses state to find target).
    CancelEditingFriendPerspective,
    /// Update the input buffer while editing friend perspective.
    UpdateFriendPerspectiveInput(String),
    /// Commit the perspective input and save to store.
    CommitFriendPerspective,
}

/// Handle friends panel messages.
pub fn handle(
    state: &mut AppState, msg: FriendPanelMessage,
) -> Task<Message> {
    match msg {
        | FriendPanelMessage::Toggle(block_id) => {
            let current_state = state.store.panel_state(&block_id).copied();
            if state.focused_block_id == Some(block_id) {
                match current_state {
                    | Some(PanelBarState::Friends) => {
                        state.store.set_panel_state(&block_id, None);
                    }
                    | _ => {
                        state.store.set_panel_state(&block_id, Some(PanelBarState::Friends));
                    }
                }
            } else {
                state.store.set_panel_state(&block_id, Some(PanelBarState::Friends));
            }
            state.persist_with_context("after toggling friends panel");
            Task::none()
        }
        | FriendPanelMessage::StartFriendPicker(_block_id) => {
            state.overflow_open_for = None;
            state.document_mode = DocumentMode::PickFriend;
            Task::none()
        }
        | FriendPanelMessage::CancelFriendPicker => {
            state.document_mode = DocumentMode::Normal;
            Task::none()
        }
        | FriendPanelMessage::StartEditingFriendPerspective { target, friend_id } => {
            let current_perspective = state
                .store
                .friend_blocks_for(&target)
                .iter()
                .find(|f| f.block_id == friend_id)
                .and_then(|f| f.perspective.clone())
                .unwrap_or_default();
            state.editing_friend_perspective = Some((target, friend_id));
            state.editing_friend_perspective_input = Some(current_perspective);
            Task::none()
        }
        | FriendPanelMessage::CancelEditingFriendPerspective => {
            // Clear editing state regardless of what's being edited
            state.editing_friend_perspective = None;
            state.editing_friend_perspective_input = None;
            if state.document_mode == DocumentMode::PickFriend {
                state.document_mode = DocumentMode::Normal;
            }
            Task::none()
        }
        | FriendPanelMessage::UpdateFriendPerspectiveInput(text) => {
            state.editing_friend_perspective_input = Some(text);
            Task::none()
        }
        | FriendPanelMessage::CommitFriendPerspective => {
            // Handled in view - the view constructs the message directly
            Task::none()
        }
    }
}

/// Render the friends panel for the focused block.
pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let block_id = match state.focused_block_id {
        Some(id) => id,
        None => return column![].into(),
    };

    let is_picker_mode =
        matches!(state.store.panel_state(&block_id), Some(PanelBarState::Friends));

    let friends = state.store.friend_blocks_for(&block_id);

    // Header with "+" button to start friend picker
    let header = row![]
        .spacing(theme::PANEL_BUTTON_GAP)
        .push(
            container(text(rust_i18n::t!("ui_friends").to_string()).font(theme::INTER).size(13))
                .width(Length::Fill),
        )
        .push(
            button(text("+").font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::FriendPanel(FriendPanelMessage::StartFriendPicker(block_id))),
        );

    let mut panel =
        column![].spacing(theme::PANEL_INNER_GAP).push(container(header).width(Length::Fill));

    // Show message based on state
    if is_picker_mode {
        panel = panel.push(
            container(
                text(rust_i18n::t!("doc_friend_picker_hint").to_string()).font(theme::INTER).size(12),
            )
            .width(Length::Fill),
        );
    } else if friends.is_empty() {
        panel = panel.push(
            container(
                text(rust_i18n::t!("doc_friend_empty_hint").to_string())
                    .font(theme::INTER)
                    .size(12)
                    .style(theme::spine_text),
            )
            .width(Length::Fill),
        );
    }

    for friend in friends {
        let point_text = state.store.point(&friend.block_id).unwrap_or_default();
        let perspective_label = friend.perspective.as_deref().unwrap_or("").trim();
        let friend_id = friend.block_id;
        let target = block_id;

        let is_editing_this =
            state.editing_friend_perspective == Some((target, friend_id));
        let input_value = state.editing_friend_perspective_input.as_deref().unwrap_or("");
        let placeholder = rust_i18n::t!("doc_friend_perspective_placeholder").to_string();

        let perspective_column: Element<'a, Message> = if is_editing_this {
            text_input(&placeholder, input_value)
                .font(theme::INTER)
                .size(12)
                .on_input(|s| Message::FriendPanel(FriendPanelMessage::UpdateFriendPerspectiveInput(s)))
                .on_submit(Message::Structure(StructureMessage::SetFriendPerspective {
                    target,
                    friend_id,
                    perspective: Some(input_value.to_string()),
                }))
                .into()
        } else if perspective_label.is_empty() {
            button(
                text(rust_i18n::t!("doc_friend_perspective_placeholder").to_string())
                    .font(theme::INTER)
                    .size(12)
                    .style(theme::spine_text),
            )
            .style(theme::action_button)
            .on_press(Message::FriendPanel(FriendPanelMessage::StartEditingFriendPerspective {
                target,
                friend_id,
            }))
            .into()
        } else {
            button(
                text(rust_i18n::t!("doc_perspective", label = perspective_label).to_string())
                    .font(theme::INTER)
                    .size(12)
                    .style(theme::spine_text),
            )
            .style(theme::action_button)
            .on_press(Message::FriendPanel(FriendPanelMessage::StartEditingFriendPerspective {
                target,
                friend_id,
            }))
            .into()
        };

        let line = row![]
            .spacing(theme::PANEL_BUTTON_GAP)
            .push(
                column![]
                    .spacing(0)
                    .push(text(point_text).font(theme::INTER).size(13))
                    .push(perspective_column)
                    .width(Length::Fill),
            )
            .push(
                button(text(rust_i18n::t!("ui_remove").to_string()).font(theme::INTER).size(13))
                    .style(theme::destructive_button)
                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .on_press(Message::Structure(StructureMessage::RemoveFriendBlock {
                        target,
                        friend_id,
                    })),
            );
        panel = panel.push(line);
    }

    container(panel)
        .padding(iced::Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}
