//! Friend blocks panel for displaying user-selected related blocks.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
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
use iced::widget::{Id, button, column, container, operation::focus, row, text, text_input};
use iced::{Element, Length, Task};
use lucide_icons::iced as icons;

/// Message types for friends panel interactions.
#[derive(Debug, Clone)]
pub enum FriendPanelMessage {
    /// Toggle friends panel visibility for the given block.
    Toggle(BlockId),
    /// Start picking a friend for the given target block.
    StartFriendPicker(BlockId),
    /// Start inline editing the perspective for a specific friend.
    StartEditingFriendPerspective { target: BlockId, friend_id: BlockId },
    /// Cancel inline editing of friend perspective (uses state to find target).
    CancelEditingFriendPerspective,
    /// Update the input buffer while editing friend perspective.
    UpdateFriendPerspectiveInput(String),
    /// Clear/remove the perspective for a friend.
    ClearFriendPerspective { target: BlockId, friend_id: BlockId },
    /// Accept the perspective and exit editing mode.
    AcceptFriendPerspective { target: BlockId, friend_id: BlockId },
    /// Toggle whether parent lineage telescope is enabled for a friend in LLM context.
    ToggleParentLineageTelescope { target: BlockId, friend_id: BlockId },
    /// Toggle whether children telescope is enabled for a friend in LLM context.
    ToggleChildrenTelescope { target: BlockId, friend_id: BlockId },
}

/// Handle friends panel messages.
pub fn handle(state: &mut AppState, msg: FriendPanelMessage) -> Task<Message> {
    match msg {
        | FriendPanelMessage::Toggle(block_id) => {
            let current_state = state.store.panel_state(&block_id).copied();
            if state.focus().is_some_and(|s| s.block_id == block_id) {
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
            state.set_overflow_open(false);
            state.document_mode = DocumentMode::PickFriend;
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
            // Focus the text input
            let input_id = Id::new("friend-perspective-input");
            focus(input_id)
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
        | FriendPanelMessage::ClearFriendPerspective { target, friend_id } => {
            // Clear the perspective in the store
            state.mutate_with_undo_and_persist("after clearing friend perspective", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let friend = friends.iter_mut().find(|f| f.block_id == friend_id);
                if let Some(friend) = friend {
                    friend.perspective = None;
                    state.store.set_friend_blocks_for(&target, friends);
                    tracing::info!(target = ?target, friend_id = ?friend_id, "cleared friend perspective");
                    true
                } else {
                    false
                }
            });
            // Also clear the editing state
            state.editing_friend_perspective = None;
            state.editing_friend_perspective_input = None;
            Task::none()
        }
        | FriendPanelMessage::AcceptFriendPerspective { target, friend_id } => {
            // Get current input value
            let perspective = state.editing_friend_perspective_input.clone();
            // Save to store
            state.mutate_with_undo_and_persist("after setting friend perspective", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let friend = friends.iter_mut().find(|f| f.block_id == friend_id);
                if let Some(friend) = friend {
                    friend.perspective = perspective.clone();
                    state.store.set_friend_blocks_for(&target, friends);
                    tracing::info!(target = ?target, friend_id = ?friend_id, "set friend perspective");
                    true
                } else {
                    false
                }
            });
            // Exit editing state
            state.editing_friend_perspective = None;
            state.editing_friend_perspective_input = None;
            Task::none()
        }
        | FriendPanelMessage::ToggleParentLineageTelescope { target, friend_id } => {
            state.mutate_with_undo_and_persist("after toggling friend parent lineage visibility", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let friend = friends.iter_mut().find(|f| f.block_id == friend_id);
                if let Some(friend) = friend {
                    let new_value = !friend.parent_lineage_telescope;
                    friend.parent_lineage_telescope = new_value;
                    state.store.set_friend_blocks_for(&target, friends);
                    tracing::info!(target = ?target, friend_id = ?friend_id, parent_lineage_telescope = new_value, "toggled friend parent lineage visibility");
                    true
                } else {
                    false
                }
            });
            Task::none()
        }
        | FriendPanelMessage::ToggleChildrenTelescope { target, friend_id } => {
            state.mutate_with_undo_and_persist("after toggling friend children visibility", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let friend = friends.iter_mut().find(|f| f.block_id == friend_id);
                if let Some(friend) = friend {
                    let new_value = !friend.children_telescope;
                    friend.children_telescope = new_value;
                    state.store.set_friend_blocks_for(&target, friends);
                    tracing::info!(target = ?target, friend_id = ?friend_id, children_telescope = new_value, "toggled friend children visibility");
                    true
                } else {
                    false
                }
            });
            Task::none()
        }
    }
}

/// Render the friends panel for the focused block.
pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let block_id = match state.focus().map(|s| s.block_id) {
        | Some(id) => id,
        | None => return column![].into(),
    };

    let is_picker_mode = matches!(state.store.panel_state(&block_id), Some(PanelBarState::Friends));

    let friends = state.store.friend_blocks_for(&block_id);

    // Header with "+" button to start friend picker
    let mut header = row![].spacing(theme::PANEL_BUTTON_GAP);
    header = header.push(
        button(
            text(rust_i18n::t!("ui_add").to_string())
                .font(theme::INTER)
                .size(theme::FRIEND_POINT_SIZE)
                .align_y(iced::alignment::Alignment::Center),
        )
        .style(theme::action_button)
        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
        .on_press(Message::FriendPanel(FriendPanelMessage::StartFriendPicker(block_id))),
    );

    // Show message based on state
    if is_picker_mode {
        header = header.push(
            container(
                text(rust_i18n::t!("doc_friend_picker_hint").to_string())
                    .style(theme::spine_text)
                    .font(theme::INTER)
                    .size(theme::FRIEND_POINT_SIZE),
            )
            .align_y(iced::alignment::Alignment::Center)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .width(Length::Fill),
        );
    } else if friends.is_empty() {
        header = header.push(
            container(
                text(rust_i18n::t!("doc_friend_empty_hint").to_string())
                    .style(theme::spine_text)
                    .font(theme::INTER)
                    .size(theme::FRIEND_POINT_SIZE),
            )
            .align_y(iced::alignment::Alignment::Center)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .width(Length::Fill),
        );
    }

    let mut panel =
        column![].spacing(theme::PANEL_INNER_GAP).push(container(header).width(Length::Fill));

    for friend in friends {
        let point_text = state.store.point(&friend.block_id).unwrap_or_default();
        let perspective_label = friend.perspective.as_deref().unwrap_or("").trim();
        let friend_id = friend.block_id;
        let target = block_id;

        let is_editing_this = state.editing_friend_perspective == Some((target, friend_id));
        let placeholder = rust_i18n::t!("doc_friend_perspective_placeholder").to_string();

        // Layout: "[start of point text] as [perspective]"
        let truncated_point = if point_text.len() > theme::FRIEND_POINT_TRUNCATE {
            format!("{}...", &point_text[..theme::FRIEND_POINT_TRUNCATE])
        } else {
            point_text.clone()
        };

        let content: Element<'a, Message> = if is_editing_this {
            // Inline editing for perspective with accept/cancel buttons
            let current_input = state.editing_friend_perspective_input.as_deref().unwrap_or("");
            // Create a unique ID for this text input
            let input_id = Id::new("friend-perspective-input");
            let input_field = text_input(&placeholder, current_input)
                .id(input_id)
                .font(theme::INTER)
                .size(theme::FRIEND_PERSPECTIVE_SIZE)
                .padding(0)
                .width(Length::Fill)
                .on_input(|s| {
                    Message::FriendPanel(FriendPanelMessage::UpdateFriendPerspectiveInput(s))
                })
                .on_submit(Message::Structure(StructureMessage::SetFriendPerspective {
                    target,
                    friend_id,
                    perspective: Some(
                        state.editing_friend_perspective_input.clone().unwrap_or_default(),
                    ),
                }));

            let accept_btn = button(icons::icon_check().size(theme::FRIEND_PERSPECTIVE_ICON_SIZE))
                .padding(2)
                .style(theme::action_button)
                .width(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                .on_press(Message::FriendPanel(FriendPanelMessage::AcceptFriendPerspective {
                    target,
                    friend_id,
                }));

            let cancel_btn = button(icons::icon_x().size(theme::FRIEND_PERSPECTIVE_ICON_SIZE))
                .padding(2)
                .style(theme::destructive_button)
                .width(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                .on_press(Message::FriendPanel(FriendPanelMessage::ClearFriendPerspective {
                    target,
                    friend_id,
                }));

            row![].spacing(4).push(input_field).push(accept_btn).push(cancel_btn).into()
        } else if perspective_label.is_empty() {
            button(
                text(rust_i18n::t!("doc_friend_perspective_placeholder").to_string())
                    .font(theme::INTER)
                    .size(theme::FRIEND_PERSPECTIVE_SIZE)
                    .style(theme::spine_text),
            )
            .style(theme::action_button)
            .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
            .width(Length::Fill)
            .padding(0)
            .on_press(Message::FriendPanel(FriendPanelMessage::StartEditingFriendPerspective {
                target,
                friend_id,
            }))
            .into()
        } else {
            button(
                text(perspective_label)
                    .font(theme::INTER)
                    .size(theme::FRIEND_PERSPECTIVE_SIZE)
                    .style(theme::spine_text),
            )
            .style(theme::action_button)
            .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
            .width(Length::Fill)
            .padding(0)
            .on_press(Message::FriendPanel(FriendPanelMessage::StartEditingFriendPerspective {
                target,
                friend_id,
            }))
            .into()
        };

        let line = row![]
            .spacing(theme::PANEL_BUTTON_GAP)
            .align_y(iced::alignment::Vertical::Top)
            .push(
                row![]
                    .spacing(theme::FRIEND_ROW_GAP)
                    .align_y(iced::alignment::Vertical::Top)
                    .push(text(truncated_point).font(theme::INTER).size(theme::FRIEND_POINT_SIZE))
                    .push(iced::widget::Space::new().width(Length::Fixed(theme::FRIEND_AS_GAP)))
                    .push(
                        text(rust_i18n::t!("doc_friend_as").to_string())
                            .style(theme::spine_text)
                            .font(theme::INTER)
                            .size(theme::FRIEND_POINT_SIZE),
                    )
                    .push(iced::widget::Space::new().width(Length::Fixed(theme::FRIEND_AS_GAP)))
                    .push(container(content))
                    .width(Length::Fill)
                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE)),
            )
            // Visibility toggles: lineage and children
            .push(
                row![]
                    .spacing(theme::FRIEND_TOGGLE_GAP)
                    .align_y(iced::alignment::Vertical::Center)
                    .push(
                        button(
                            icons::icon_corner_up_left().size(theme::FRIEND_TOGGLE_ICON_SIZE).center(),
                        )
                        .style(theme::toggle_button(friend.parent_lineage_telescope))
                        .height(Length::Fixed(theme::FRIEND_TOGGLE_SIZE))
                        .width(Length::Fixed(theme::FRIEND_TOGGLE_SIZE))
                        .padding(0)
                        .on_press(Message::FriendPanel(
                            FriendPanelMessage::ToggleParentLineageTelescope { target, friend_id },
                        )),
                    )
                    .push(
                        button(
                            icons::icon_corner_down_right().size(theme::FRIEND_TOGGLE_ICON_SIZE).center(),
                        )
                        .style(theme::toggle_button(friend.children_telescope))
                        .height(Length::Fixed(theme::FRIEND_TOGGLE_SIZE))
                        .width(Length::Fixed(theme::FRIEND_TOGGLE_SIZE))
                        .padding(0)
                        .on_press(Message::FriendPanel(
                            FriendPanelMessage::ToggleChildrenTelescope { target, friend_id },
                        )),
                    ),
            )
            .push(
                button(
                    text(rust_i18n::t!("ui_remove").to_string())
                        .font(theme::INTER)
                        .size(theme::FRIEND_POINT_SIZE),
                )
                .style(theme::destructive_button)
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::Structure(
                    StructureMessage::RemoveFriendBlock { target, friend_id },
                )),
            );
        panel = panel.push(line);
    }

    container(panel)
        .padding(iced::Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}
