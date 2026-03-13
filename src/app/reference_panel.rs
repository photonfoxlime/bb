//! Reference panel for block-local related context.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! The panel contains one merged list of block-local references:
//! - point links attached to the block,
//! - friend relations used as additional context.
//!
//! ## Inline Perspective Editor
//!
//! Friend rows and point-link rows can both expose an editable "perspective"
//! field. This is a user-authored framing string that describes how the source
//! block should interpret that reference. For example, a reference might be
//! viewed from "historical lens", "skeptical counterpoint", or "supporting evidence"
//! perspective.
//!
//! The perspective is rendered as a secondary line below the reference summary.
//! When empty, a localized placeholder invites the user to "add perspective...".
//! Clicking the perspective area toggles an inline text input field. On blur
//! (or Enter key), the panel saves the new perspective directly to the owning
//! friend relation or point link.
//!
//! Design rationale:
//! - Inline editing avoids navigating to a separate modal/dialog, keeping context visible.
//! - Immediate save on blur provides instant feedback without requiring explicit save actions.
//! - Empty state with placeholder makes the affordance discoverable without cluttering the UI.

use crate::app::state::ReferencePerspectiveEditState;
use crate::app::{AppState, DocumentMode, EditMessage, LinkModeMessage, Message, StructureMessage};
use crate::component::{
    icon_button::IconButton, reference_list_row::ReferenceListRow,
    reference_perspective::reference_perspective_input_id, reference_row::ReferenceRow,
    text_button::TextButton,
};
use crate::store::{BlockId, BlockPanelBarState, LinkKind, PointLink};
use crate::text::truncate_for_display;
use crate::theme;
use iced::widget::{column, container, markdown, operation::focus, row, text, tooltip};
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
    /// Start inline editing the perspective for a specific point link.
    StartEditingLinkPerspective { target: BlockId, link_index: usize },
    /// Cancel inline editing of the active reference perspective.
    CancelEditingPerspective,
    /// Update the input buffer while editing a reference perspective.
    UpdatePerspectiveInput(String),
    /// Clear/remove the perspective for a friend.
    ClearFriendPerspective { target: BlockId, friend_id: BlockId },
    /// Accept the perspective and exit editing mode.
    AcceptFriendPerspective { target: BlockId, friend_id: BlockId },
    /// Clear/remove the perspective for a point link.
    ClearLinkPerspective { target: BlockId, link_index: usize },
    /// Accept the perspective and exit editing mode for a point link.
    AcceptLinkPerspective { target: BlockId, link_index: usize },
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
                        state.ui_mut().reference_panel.editing_perspective = None;
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
            state.ui_mut().reference_panel.editing_perspective =
                Some(ReferencePerspectiveEditState::Friend {
                    target,
                    friend_id,
                    input: current_perspective,
                });
            // Focus the text input
            focus(reference_perspective_input_id())
        }
        | ReferencePanelMessage::StartEditingLinkPerspective { target, link_index } => {
            let current_perspective = state
                .store
                .point_content(&target)
                .and_then(|content| content.links.get(link_index))
                .and_then(|link| link.perspective.clone())
                .unwrap_or_default();
            state.ui_mut().reference_panel.editing_perspective =
                Some(ReferencePerspectiveEditState::Link {
                    target,
                    link_index,
                    input: current_perspective,
                });
            focus(reference_perspective_input_id())
        }
        | ReferencePanelMessage::CancelEditingPerspective => {
            // Clear editing state regardless of what's being edited
            state.ui_mut().reference_panel.editing_perspective = None;
            if state.ui().document_mode == DocumentMode::PickFriend {
                state.ui_mut().document_mode = DocumentMode::Normal;
            }
            Task::none()
        }
        | ReferencePanelMessage::UpdatePerspectiveInput(text) => {
            if let Some(editing) = state.ui_mut().reference_panel.editing_perspective.as_mut() {
                editing.set_input(text);
            }
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
            state.ui_mut().reference_panel.editing_perspective = None;
            Task::none()
        }
        | ReferencePanelMessage::AcceptFriendPerspective { target, friend_id } => {
            let Some(perspective) = active_friend_perspective_input(state, target, friend_id)
            else {
                return Task::none();
            };
            // Save to store
            state.mutate_with_undo_and_persist("after setting friend perspective", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let friend = friends.iter_mut().find(|f| f.block_id == friend_id);
                if let Some(friend) = friend {
                    friend.perspective = Some(perspective.clone());
                    state.store.set_friend_blocks_for(&target, friends);
                    tracing::info!(target = ?target, friend_id = ?friend_id, "set friend perspective");
                    true
                } else {
                    false
                }
            });
            // Exit editing state
            state.ui_mut().reference_panel.editing_perspective = None;
            Task::none()
        }
        | ReferencePanelMessage::ClearLinkPerspective { target, link_index } => {
            state.mutate_with_undo_and_persist("after clearing link perspective", |state| {
                let changed = state.store.set_link_perspective(&target, link_index, None);
                if changed {
                    tracing::info!(target = ?target, link_index, "cleared link perspective");
                }
                changed
            });
            state.ui_mut().reference_panel.editing_perspective = None;
            Task::none()
        }
        | ReferencePanelMessage::AcceptLinkPerspective { target, link_index } => {
            let Some(perspective) = active_link_perspective_input(state, target, link_index) else {
                return Task::none();
            };
            state.mutate_with_undo_and_persist("after setting link perspective", |state| {
                let changed = state.store.set_link_perspective(
                    &target,
                    link_index,
                    Some(perspective.clone()),
                );
                if changed {
                    tracing::info!(target = ?target, link_index, "set link perspective");
                }
                changed
            });
            state.ui_mut().reference_panel.editing_perspective = None;
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

/// Render the reference panel for `target_block_id`.
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

    let mut action_bar = row![].spacing(theme::PANEL_BUTTON_GAP);
    action_bar = action_bar.push(
        TextButton::action(
            rust_i18n::t!("action_add_friend").to_string(),
            theme::FRIEND_POINT_SIZE,
        )
        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
        .on_press(Message::ReferencePanel(ReferencePanelMessage::StartFriendPicker(
            target_block_id,
        ))),
    );
    action_bar = action_bar.push(
        TextButton::action(rust_i18n::t!("action_add_link").to_string(), theme::FRIEND_POINT_SIZE)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::LinkMode(LinkModeMessage::Enter(target_block_id))),
    );

    let message_text = if is_picker_mode {
        Some(rust_i18n::t!("doc_friend_picker_hint").to_string())
    } else if friends.is_empty() && links.is_empty() {
        Some(rust_i18n::t!("doc_reference_empty_hint").to_string())
    } else {
        None
    };

    let mut panel =
        column![].spacing(theme::PANEL_INNER_GAP).push(container(action_bar).width(Length::Fill));

    if let Some(message_text) = message_text {
        panel = panel.push(
            text(message_text)
                .style(theme::spine_text)
                .font(theme::INTER)
                .size(theme::FRIEND_POINT_SIZE),
        );
    }

    for (index, link) in links.iter().enumerate() {
        panel = panel.push(view_link_row(
            state,
            target_block_id,
            index,
            link,
            expanded_link_index,
            expanded_markdown_preview,
            state.is_dark_mode(),
        ));
    }

    for friend in friends {
        let point_text = state.store.point(&friend.block_id).unwrap_or_default();
        let perspective_label = friend.perspective.as_deref().unwrap_or("").trim();
        let friend_id = friend.block_id;
        let target = target_block_id;

        let is_editing_this = matches!(
            state.ui().reference_panel.editing_perspective,
            Some(ReferencePerspectiveEditState::Friend {
                target: editing_target,
                friend_id: editing_friend_id,
                ..
            }) if editing_target == target && editing_friend_id == friend_id
        );
        let placeholder = rust_i18n::t!("doc_reference_perspective_placeholder").to_string();
        let relation_label = rust_i18n::t!("doc_reference_as").to_string();
        let parent_toggle_tooltip = rust_i18n::t!("doc_friend_telescope_parent").to_string();
        let children_toggle_tooltip = rust_i18n::t!("doc_friend_telescope_children").to_string();
        let remove_label = rust_i18n::t!("ui_remove").to_string();
        let current_input = active_editor_input(state);
        let controls = view_friend_controls(
            friend.parent_lineage_telescope,
            friend.children_telescope,
            &parent_toggle_tooltip,
            &children_toggle_tooltip,
            &remove_label,
            Message::ReferencePanel(ReferencePanelMessage::ToggleParentLineageTelescope {
                target,
                friend_id,
            }),
            Message::ReferencePanel(ReferencePanelMessage::ToggleChildrenTelescope {
                target,
                friend_id,
            }),
            Message::Structure(StructureMessage::RemoveFriendBlock { target, friend_id }),
        );

        panel = panel.push(
            ReferenceRow {
                primary: ReferenceRow::text_summary_button(
                    truncate_for_display(&point_text, theme::FRIEND_POINT_TRUNCATE),
                    Message::ReferencePanel(ReferencePanelMessage::HoverFriend(friend_id)),
                ),
                perspective_label: perspective_label.to_string(),
                is_editing: is_editing_this,
                current_input,
                perspective_placeholder: placeholder,
                relation_label,
                controls,
                on_start_editing: Message::ReferencePanel(
                    ReferencePanelMessage::StartEditingFriendPerspective { target, friend_id },
                ),
                on_clear_perspective: Message::ReferencePanel(
                    ReferencePanelMessage::ClearFriendPerspective { target, friend_id },
                ),
                on_accept_perspective: Message::ReferencePanel(
                    ReferencePanelMessage::AcceptFriendPerspective { target, friend_id },
                ),
                on_submit_input: Message::ReferencePanel(
                    ReferencePanelMessage::AcceptFriendPerspective { target, friend_id },
                ),
                on_update_input: |s| {
                    Message::ReferencePanel(ReferencePanelMessage::UpdatePerspectiveInput(s))
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
    state: &'a AppState, target_block_id: BlockId, index: usize, link: &'a PointLink,
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

    let is_editing_this = matches!(
        state.ui().reference_panel.editing_perspective,
        Some(ReferencePerspectiveEditState::Link {
            target,
            link_index,
            ..
        }) if target == target_block_id && link_index == index
    );
    let controls =
        TextButton::destructive(rust_i18n::t!("ui_remove").to_string(), theme::FRIEND_POINT_SIZE)
            .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
            .padding(Padding::ZERO)
            .on_press(Message::Edit(EditMessage::RemoveLink { block_id: target_block_id, index }))
            .into();

    let row = ReferenceRow {
        primary,
        perspective_label: link.perspective.clone().unwrap_or_default(),
        is_editing: is_editing_this,
        current_input: active_editor_input(state),
        perspective_placeholder: rust_i18n::t!("doc_reference_perspective_placeholder").to_string(),
        relation_label: rust_i18n::t!("doc_reference_as").to_string(),
        controls,
        on_start_editing: Message::ReferencePanel(
            ReferencePanelMessage::StartEditingLinkPerspective {
                target: target_block_id,
                link_index: index,
            },
        ),
        on_clear_perspective: Message::ReferencePanel(
            ReferencePanelMessage::ClearLinkPerspective {
                target: target_block_id,
                link_index: index,
            },
        ),
        on_accept_perspective: Message::ReferencePanel(
            ReferencePanelMessage::AcceptLinkPerspective {
                target: target_block_id,
                link_index: index,
            },
        ),
        on_submit_input: Message::ReferencePanel(ReferencePanelMessage::AcceptLinkPerspective {
            target: target_block_id,
            link_index: index,
        }),
        on_update_input: |s| {
            Message::ReferencePanel(ReferencePanelMessage::UpdatePerspectiveInput(s))
        },
    }
    .view();

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

/// Return the current input buffer of the active reference perspective editor.
fn active_editor_input(state: &AppState) -> String {
    state
        .ui()
        .reference_panel
        .editing_perspective
        .as_ref()
        .map(|editing| editing.input().to_string())
        .unwrap_or_default()
}

/// Return the active friend perspective input if it matches the given row.
fn active_friend_perspective_input(
    state: &AppState, target: BlockId, friend_id: BlockId,
) -> Option<String> {
    match state.ui().reference_panel.editing_perspective.as_ref() {
        | Some(ReferencePerspectiveEditState::Friend {
            target: editing_target,
            friend_id: editing_friend_id,
            input,
        }) if *editing_target == target && *editing_friend_id == friend_id => Some(input.clone()),
        | _ => None,
    }
}

/// Return the active link perspective input if it matches the given row.
fn active_link_perspective_input(
    state: &AppState, target: BlockId, link_index: usize,
) -> Option<String> {
    match state.ui().reference_panel.editing_perspective.as_ref() {
        | Some(ReferencePerspectiveEditState::Link {
            target: editing_target,
            link_index: editing_link_index,
            input,
        }) if *editing_target == target && *editing_link_index == link_index => Some(input.clone()),
        | _ => None,
    }
}

/// Render friend-only telescope controls plus the shared remove action.
fn view_friend_controls(
    parent_lineage_telescope: bool, children_telescope: bool, parent_toggle_tooltip: &str,
    children_toggle_tooltip: &str, remove_label: &str, on_toggle_parent_lineage: Message,
    on_toggle_children: Message, on_remove_friend: Message,
) -> Element<'static, Message> {
    row![]
        .spacing(theme::FRIEND_TOGGLE_GAP)
        .padding(Padding::ZERO.left(theme::TOOLTIP_PAD))
        .align_y(iced::alignment::Vertical::Center)
        .push(
            tooltip(
                IconButton::toggle_with_size(
                    icons::icon_corner_up_left().size(theme::FRIEND_TOGGLE_ICON_SIZE).into(),
                    parent_lineage_telescope,
                    theme::FRIEND_TOGGLE_SIZE,
                    0.0,
                )
                .on_press(on_toggle_parent_lineage),
                text(parent_toggle_tooltip.to_owned())
                    .size(theme::SMALL_TEXT_SIZE)
                    .font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP),
        )
        .push(
            tooltip(
                IconButton::toggle_with_size(
                    icons::icon_corner_down_right().size(theme::FRIEND_TOGGLE_ICON_SIZE).into(),
                    children_telescope,
                    theme::FRIEND_TOGGLE_SIZE,
                    0.0,
                )
                .on_press(on_toggle_children),
                text(children_toggle_tooltip.to_owned())
                    .size(theme::SMALL_TEXT_SIZE)
                    .font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP),
        )
        .push(
            TextButton::destructive(remove_label.to_string(), theme::FRIEND_POINT_SIZE)
                .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                .padding(Padding::ZERO)
                .on_press(on_remove_friend),
        )
        .into()
}
