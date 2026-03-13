//! Reference panel for block-local related context.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! The panel currently contains two sections:
//! - point links attached to the block,
//! - friend relations used as additional context.
//!
//! Note: the naming is intentionally broader than the current behavior because
//! point links are expected to converge here as part of the reference-panel
//! merge.
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

use crate::app::{AppState, DocumentMode, EditMessage, LinkModeMessage, Message, StructureMessage};
use crate::component::{
    friend_row::{FriendRow, friend_perspective_input_id},
    reference_list_row::ReferenceListRow,
    text_button::TextButton,
};
use crate::store::{BlockId, BlockPanelBarState, LinkKind, PointLink};
use crate::theme;
use iced::widget::{column, container, markdown, operation::focus, row, text};
use iced::{Element, Length, Padding, Task};
use lucide_icons::iced as icons;

/// Message types for reference panel interactions.
#[derive(Debug, Clone)]
pub enum ReferencePanelMessage {
    /// Toggle the reference panel visibility for the given block.
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
    /// Friend block clicked in panel - highlights the friend in the document tree.
    ///
    /// Note: Currently triggered on click rather than hover due to iced's closure
    /// type system limitations with `mouse_area`. Future implementation may use
    /// subscriptions for true hover detection.
    HoverFriend(BlockId),
    /// Friend click exited - clears the highlight (reserved for future hover implementation).
    #[allow(dead_code)]
    UnhoverFriend,
}

/// Handle reference panel messages.
pub fn handle(state: &mut AppState, msg: ReferencePanelMessage) -> Task<Message> {
    match msg {
        | ReferencePanelMessage::Toggle(block_id) => {
            let current_state = state.store.block_panel_state(&block_id).copied();
            if state.focus().is_some_and(|s| s.block_id == block_id) {
                match current_state {
                    | Some(BlockPanelBarState::Friends) => {
                        state.store.set_block_panel_state(&block_id, None);
                        // Clear hover state when closing the friends panel
                        state.ui_mut().reference_panel.hovered_friend_block = None;
                    }
                    | _ => {
                        state
                            .store
                            .set_block_panel_state(&block_id, Some(BlockPanelBarState::Friends));
                    }
                }
            } else {
                state.store.set_block_panel_state(&block_id, Some(BlockPanelBarState::Friends));
            }
            state.persist_with_context("after toggling friends panel");
            Task::none()
        }
        | ReferencePanelMessage::StartFriendPicker(_block_id) => {
            state.set_overflow_open(false);
            state.ui_mut().document_mode = DocumentMode::PickFriend;
            Task::none()
        }
        | ReferencePanelMessage::StartEditingFriendPerspective { target, friend_id } => {
            let current_perspective = state
                .store
                .friend_blocks_for(&target)
                .iter()
                .find(|f| f.block_id == friend_id)
                .and_then(|f| f.perspective.clone())
                .unwrap_or_default();
            state.ui_mut().reference_panel.editing_friend_perspective = Some((target, friend_id));
            state.ui_mut().reference_panel.editing_friend_perspective_input =
                Some(current_perspective);
            // Focus the text input
            focus(friend_perspective_input_id())
        }
        | ReferencePanelMessage::CancelEditingFriendPerspective => {
            // Clear editing state regardless of what's being edited
            state.ui_mut().reference_panel.editing_friend_perspective = None;
            state.ui_mut().reference_panel.editing_friend_perspective_input = None;
            if state.ui().document_mode == DocumentMode::PickFriend {
                state.ui_mut().document_mode = DocumentMode::Normal;
            }
            Task::none()
        }
        | ReferencePanelMessage::UpdateFriendPerspectiveInput(text) => {
            state.ui_mut().reference_panel.editing_friend_perspective_input = Some(text);
            Task::none()
        }
        | ReferencePanelMessage::ClearFriendPerspective { target, friend_id } => {
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
            state.ui_mut().reference_panel.editing_friend_perspective = None;
            state.ui_mut().reference_panel.editing_friend_perspective_input = None;
            Task::none()
        }
        | ReferencePanelMessage::AcceptFriendPerspective { target, friend_id } => {
            // Get current input value
            let perspective = state.ui().reference_panel.editing_friend_perspective_input.clone();
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
            state.ui_mut().reference_panel.editing_friend_perspective = None;
            state.ui_mut().reference_panel.editing_friend_perspective_input = None;
            Task::none()
        }
        | ReferencePanelMessage::ToggleParentLineageTelescope { target, friend_id } => {
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
        | ReferencePanelMessage::ToggleChildrenTelescope { target, friend_id } => {
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
        | ReferencePanelMessage::HoverFriend(friend_id) => {
            state.ui_mut().reference_panel.hovered_friend_block = Some(friend_id);
            Task::none()
        }
        | ReferencePanelMessage::UnhoverFriend => {
            state.ui_mut().reference_panel.hovered_friend_block = None;
            Task::none()
        }
    }
}

/// Render the friends panel for `target_block_id`.
///
/// Note: the target is explicit so document-level panel hosts can decide which
/// block owns the panel without requiring this view to read global focus.
pub fn view<'a>(state: &'a AppState, target_block_id: BlockId) -> Element<'a, Message> {
    let is_picker_mode = matches!(
        state.store.block_panel_state(&target_block_id),
        Some(BlockPanelBarState::Friends)
    );

    let links = state
        .store
        .point_content(&target_block_id)
        .map(|content| content.links.as_slice())
        .unwrap_or(&[]);
    let expanded_link_index =
        state.ui().reference_panel.expanded_links.get(&target_block_id).copied();
    let expanded_markdown_preview = state.expanded_markdown_preview(&target_block_id);
    let friends = state.store.friend_blocks_for(&target_block_id);

    let link_header = row![].spacing(theme::PANEL_BUTTON_GAP).push(
        TextButton::action(rust_i18n::t!("action_add_link").to_string(), theme::FRIEND_POINT_SIZE)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::LinkMode(LinkModeMessage::Enter(target_block_id))),
    );

    // Header with "+" button to start friend picker
    let mut friend_header = row![].spacing(theme::PANEL_BUTTON_GAP);
    friend_header = friend_header.push(
        TextButton::action(rust_i18n::t!("ui_add").to_string(), theme::FRIEND_POINT_SIZE)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::ReferencePanel(ReferencePanelMessage::StartFriendPicker(
                target_block_id,
            ))),
    );

    let message_text = if is_picker_mode {
        Some(rust_i18n::t!("doc_friend_picker_hint").to_string())
    } else if friends.is_empty() {
        Some(rust_i18n::t!("doc_friend_empty_hint").to_string())
    } else {
        None
    };
    // Show message based on state
    if let Some(message_text) = message_text {
        friend_header = friend_header.push(
            container(
                text(message_text)
                    .style(theme::spine_text)
                    .font(theme::INTER)
                    .size(theme::FRIEND_POINT_SIZE)
                    .align_y(iced::alignment::Alignment::Center),
            )
            .align_y(iced::alignment::Alignment::Center)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .width(Length::Fill)
            .padding(Padding::ZERO.left(theme::FRIEND_ROW_GAP)),
        )
    }

    let mut panel =
        column![].spacing(theme::PANEL_INNER_GAP).push(container(link_header).width(Length::Fill));

    for (index, link) in links.iter().enumerate() {
        panel = panel.push(view_link_row(
            target_block_id,
            index,
            link,
            expanded_link_index,
            expanded_markdown_preview,
            state.is_dark_mode(),
        ));
    }

    panel = panel.push(container(friend_header).width(Length::Fill));

    for friend in friends {
        let point_text = state.store.point(&friend.block_id).unwrap_or_default();
        let perspective_label = friend.perspective.as_deref().unwrap_or("").trim();
        let friend_id = friend.block_id;
        let target = target_block_id;

        let is_editing_this =
            state.ui().reference_panel.editing_friend_perspective == Some((target, friend_id));
        let placeholder = rust_i18n::t!("doc_friend_perspective_placeholder").to_string();
        let relation_label = rust_i18n::t!("doc_friend_as").to_string();
        let parent_toggle_tooltip = rust_i18n::t!("doc_friend_telescope_parent").to_string();
        let children_toggle_tooltip = rust_i18n::t!("doc_friend_telescope_children").to_string();
        let remove_label = rust_i18n::t!("ui_remove").to_string();
        let current_input =
            state.ui().reference_panel.editing_friend_perspective_input.clone().unwrap_or_default();

        panel = panel.push(
            FriendRow {
                point_text,
                perspective_label: perspective_label.to_string(),
                is_editing: is_editing_this,
                current_input,
                parent_lineage_telescope: friend.parent_lineage_telescope,
                children_telescope: friend.children_telescope,
                perspective_placeholder: placeholder,
                relation_label,
                parent_toggle_tooltip,
                children_toggle_tooltip,
                remove_label,
                on_press_point: Message::ReferencePanel(ReferencePanelMessage::HoverFriend(
                    friend_id,
                )),
                on_start_editing: Message::ReferencePanel(
                    ReferencePanelMessage::StartEditingFriendPerspective { target, friend_id },
                ),
                on_clear_perspective: Message::ReferencePanel(
                    ReferencePanelMessage::ClearFriendPerspective { target, friend_id },
                ),
                on_accept_perspective: Message::ReferencePanel(
                    ReferencePanelMessage::AcceptFriendPerspective { target, friend_id },
                ),
                on_submit_input: Message::Structure(StructureMessage::SetFriendPerspective {
                    target,
                    friend_id,
                    perspective: Some(
                        state
                            .ui()
                            .reference_panel
                            .editing_friend_perspective_input
                            .clone()
                            .unwrap_or_default(),
                    ),
                }),
                on_toggle_parent_lineage: Message::ReferencePanel(
                    ReferencePanelMessage::ToggleParentLineageTelescope { target, friend_id },
                ),
                on_toggle_children: Message::ReferencePanel(
                    ReferencePanelMessage::ToggleChildrenTelescope { target, friend_id },
                ),
                on_remove_friend: Message::Structure(StructureMessage::RemoveFriendBlock {
                    target,
                    friend_id,
                }),
                on_update_input: |s| {
                    Message::ReferencePanel(ReferencePanelMessage::UpdateFriendPerspectiveInput(s))
                },
            }
            .view(),
        );
    }

    container(panel)
        .padding(iced::Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}

/// Render one point-link row using the same shell as friend rows.
fn view_link_row<'a>(
    target_block_id: BlockId, index: usize, link: &'a PointLink,
    expanded_link_index: Option<usize>, expanded_markdown_preview: Option<&'a [markdown::Item]>,
    is_dark_mode: bool,
) -> Element<'a, Message> {
    let primary = ReferenceListRow::summary_button(
        row![]
            .spacing(theme::FRIEND_ROW_GAP)
            .align_y(iced::alignment::Vertical::Center)
            .push(link_kind_icon(link.kind))
            .push(
                text(link.display_text().to_owned())
                    .font(theme::INTER)
                    .size(theme::FRIEND_POINT_SIZE),
            ),
        Message::LinkChipToggle(target_block_id, index),
    );

    let detail = container(
        text(link_kind_label(link.kind))
            .style(theme::spine_text)
            .font(theme::INTER)
            .size(theme::FRIEND_PERSPECTIVE_SIZE),
    )
    .width(Length::Fill)
    .into();

    let controls =
        TextButton::destructive(rust_i18n::t!("ui_remove").to_string(), theme::FRIEND_POINT_SIZE)
            .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
            .padding(Padding::ZERO)
            .on_press(Message::Edit(EditMessage::RemoveLink { block_id: target_block_id, index }))
            .into();

    let row = ReferenceListRow { primary, relation_label: None, detail, controls }.view();

    if expanded_link_index == Some(index) {
        return column![
            row,
            view_link_preview(target_block_id, link, expanded_markdown_preview, is_dark_mode)
        ]
        .spacing(theme::INLINE_GAP)
        .into();
    }

    row
}

/// Render the expanded inline preview for a link row.
fn view_link_preview<'a>(
    target_block_id: BlockId, link: &'a PointLink,
    expanded_markdown_preview: Option<&'a [markdown::Item]>, is_dark_mode: bool,
) -> Element<'a, Message> {
    match link.kind {
        | LinkKind::Image => {
            iced::widget::image(iced::widget::image::Handle::from_path(&link.href))
                .width(Length::Fill)
                .into()
        }
        | LinkKind::Markdown => {
            if let Some(markdown_preview) = expanded_markdown_preview {
                let markdown_widget: Element<'a, Message> = markdown::view(
                    markdown_preview,
                    theme::markdown_preview_settings(is_dark_mode),
                )
                .map(move |uri| Message::MarkdownPreviewLinkClicked(target_block_id, uri))
                .into();
                container(markdown_widget).padding(theme::LINK_CHIP_PAD).width(Length::Fill).into()
            } else {
                iced::widget::Space::new().height(Length::Shrink).into()
            }
        }
        | LinkKind::Path => iced::widget::Space::new().height(Length::Shrink).into(),
    }
}

/// Render the icon corresponding to a link kind.
fn link_kind_icon(kind: LinkKind) -> Element<'static, Message> {
    match kind {
        | LinkKind::Image => icons::icon_image().size(theme::LINK_CHIP_ICON_SIZE).into(),
        | LinkKind::Markdown => icons::icon_file_text().size(theme::LINK_CHIP_ICON_SIZE).into(),
        | LinkKind::Path => icons::icon_link().size(theme::LINK_CHIP_ICON_SIZE).into(),
    }
}

/// Return the localized display label for a point-link kind.
fn link_kind_label(kind: LinkKind) -> String {
    match kind {
        | LinkKind::Image => rust_i18n::t!("link_kind_image").to_string(),
        | LinkKind::Markdown => rust_i18n::t!("link_kind_markdown").to_string(),
        | LinkKind::Path => rust_i18n::t!("link_kind_path").to_string(),
    }
}
