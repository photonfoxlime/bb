//! Immutable document and tree renderer from `AppState` to Iced elements.
//!
//! Rendering semantics:
//! - mount and fold state are represented through disclosure marker behavior,
//! - action bars are projected per-row via `action_bar` view-model pipeline,
//! - rewrite/reduce drafts render inline word-level diff panels.
//!
//! # Friend blocks UI
//!
//! Friend blocks are shown per block that has at least one friend:
//! - A "Friends" panel is rendered below the block row (same pattern as
//!   expansion/reduction draft panels), listing each friend's point text and
//!   optional perspective, with a remove button per friend.

use super::{
    AppState, EditMessage, ErrorBanner, ErrorMessage, ExpandMessage, Message, MountFileMessage,
    OverlayMessage, ReduceMessage, ShortcutMessage, StructureMessage,
    action_bar::{
        ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, RowContext, StatusChipVm,
        ViewportBucket, action_to_message, build_action_bar_vm, project_for_viewport,
        shortcut_to_action,
    },
    diff::{WordChange, word_diff},
    instruction_panel,
    settings::SettingsMessage,
};
use crate::{
    store::{BlockId, ExpansionDraftRecord, FriendBlock, ReductionDraftRecord},
    theme,
};
use rust_i18n::t;
use iced::{
    Element, Fill, Length, Padding,
    widget::{
        button, column, container, row, rule, scrollable, space, stack, text, text_editor, tooltip,
    },
};
use lucide_icons::iced as icons;

/// Stateless view that borrows `AppState` to render the document.
///
/// All rendering methods return iced `Element`s; no mutation of state occurs.
pub(super) struct DocumentView<'a> {
    state: &'a AppState,
}

impl<'a> DocumentView<'a> {
    pub(super) fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub(super) fn view(&self) -> Element<'a, Message> {
        let Self { state } = self;
        let mut layout = column![].spacing(theme::LAYOUT_GAP);

        // Settings gear button – rendered as a floating overlay at the bottom-right.
        let gear_button = button(
            lucide_icons::iced::icon_settings()
                .size(16)
                .line_height(iced::widget::text::LineHeight::Relative(1.0)),
        )
        .on_press(Message::Settings(SettingsMessage::Open))
        .style(theme::action_button)
        .padding(theme::BUTTON_PAD);

        if let Some(error_banner) = ErrorBanner::from_state(state) {
            let mut banner_content = column![
                row![
                    text(error_banner.title()),
                    button(text(t!("ui_dismiss").to_string())).on_press(Message::Error(ErrorMessage::DismissAt(
                        error_banner.latest.index
                    ))),
                ]
                .spacing(8)
            ]
            .spacing(4);
            for entry in &error_banner.previous_entries {
                banner_content = banner_content.push(
                    row![
                        text(t!("error_earlier", message = entry.message.as_str()).to_string()),
                        button(text(t!("ui_dismiss").to_string()))
                            .on_press(Message::Error(ErrorMessage::DismissAt(entry.index))),
                    ]
                    .spacing(8),
                );
            }
            if error_banner.hidden_previous_count > 0 {
                banner_content = banner_content.push(text(t!(
                    "error_older_count",
                    count = error_banner.hidden_previous_count
                ).to_string()));
            }
            layout = layout.push(
                container(banner_content).style(theme::error_banner).padding(theme::BANNER_PAD),
            );
        }

        let tree = TreeView::new(state).render_roots();
        let content = container(tree).padding(theme::CANVAS_PAD).max_width(theme::CANVAS_MAX_WIDTH);
        layout = layout.push(
            scrollable(
                container(content)
                    .width(Fill)
                    .center_x(Fill)
                    .padding(iced::Padding::ZERO.top(theme::CANVAS_TOP)),
            )
            .height(Fill),
        );

        let main_content = container(layout).style(theme::canvas).width(Fill).height(Fill);

        let floating_gear = container(
            container(gear_button)
                .width(Fill)
                .align_y(iced::alignment::Vertical::Top)
                .align_x(iced::alignment::Horizontal::Right)
                .padding(iced::Padding::new(16.0).right(theme::CANVAS_PAD).bottom(16.0)),
        )
        .width(Fill)
        .height(Fill);

        stack![main_content, floating_gear].width(Fill).height(Fill).into()
    }
}

/// Stateless view that borrows `AppState` to render the block tree.
///
/// All rendering methods return iced `Element`s; no mutation of state occurs.
pub(super) struct TreeView<'a> {
    state: &'a AppState,
}

impl<'a> TreeView<'a> {
    pub(super) fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub(super) fn render_roots(&self) -> Element<'a, Message> {
        self.render_line(self.state.store.roots())
    }

    fn render_line(&self, ids: &'a [BlockId]) -> Element<'a, Message> {
        let mut col = column![].spacing(theme::BLOCK_GAP);
        for id in ids {
            if self.state.store.node(id).is_none() {
                continue;
            }
            col = col.push(self.render_block(id));
        }
        col.into()
    }

    fn render_block(&self, block_id: &BlockId) -> Element<'a, Message> {
        let Some(node) = self.state.store.node(block_id) else {
            return container(text("")).into();
        };

        let is_expanded_mount = self.state.store.mount_table().entry(*block_id).is_some();
        let unexpanded_mount_path = node.mount_path();

        let Some(editor_content) = self.state.editor_buffers.get(block_id) else {
            let fallback_text = self.state.store.point(block_id).unwrap_or_default();
            tracing::error!(block_id = ?block_id, "missing editor content for rendered block");
            return container(text(fallback_text).style(theme::spine_text)).into();
        };

        let block_id_for_edit = *block_id;
        let row_context = self.action_row_context(block_id, editor_content.text());
        let action_bar =
            project_for_viewport(build_action_bar_vm(&row_context), self.viewport_bucket());

        let spine = container(rule::vertical(1).style(theme::spine_rule))
            .width(Length::Fixed(theme::SPINE_WIDTH))
            .align_x(iced::alignment::Horizontal::Center);
        let has_children = !self.state.store.children(block_id).is_empty();
        let is_collapsed = self.state.store.is_collapsed(block_id);
        let is_foldable = has_children || is_expanded_mount || unexpanded_mount_path.is_some();

        let marker: Element<'a, Message> = if is_foldable {
            let icon = if is_collapsed || unexpanded_mount_path.is_some() {
                ActionId::ExpandBranch
            } else {
                ActionId::CollapseBranch
            };
            let msg = if unexpanded_mount_path.is_some() {
                Message::MountFile(MountFileMessage::ExpandMount(*block_id))
            } else if is_expanded_mount {
                Message::MountFile(MountFileMessage::CollapseMount(*block_id))
            } else {
                Message::Structure(StructureMessage::ToggleFold(*block_id))
            };
            button(centered_icon(action_icon(icon)))
                .style(theme::action_button)
                .padding(0)
                .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(msg)
                .into()
        } else {
            let ring_icon: Element<'a, Message> = icons::icon_circle()
                .size(theme::LEAF_RING_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into();
            button(centered_icon(ring_icon))
                .style(theme::action_button)
                .padding(0)
                .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .into()
        };

        let is_focused = self.state.focused_block_id == Some(*block_id);
        let friends_panel_open = is_focused
            && self
                .state
                .panel_bar_state
                .as_ref()
                .is_some_and(|state| *state == super::PanelBarState::Friends);

        // Only render action bar when block is focused
        let action_buttons: Element<'a, Message> = if is_focused {
            self.render_action_buttons(block_id, &action_bar)
        } else {
            container(iced::widget::Space::new())
                .padding(
                    Padding::ZERO
                        .top(theme::ROW_CONTROL_VERTICAL_PAD)
                        .bottom(theme::ROW_CONTROL_VERTICAL_PAD),
                )
                .into()
        };

        let row_content = row![]
            .spacing(theme::ROW_GAP)
            .width(Fill)
            .align_y(iced::Alignment::Start)
            .push(spine)
            .push(
                container(marker).padding(
                    Padding::ZERO
                        .top(theme::ROW_CONTROL_VERTICAL_PAD)
                        .bottom(theme::ROW_CONTROL_VERTICAL_PAD),
                ),
            )
            .push({
                let mut editor = text_editor(editor_content)
                    .placeholder(t!("doc_placeholder_point").to_string())
                    .style(theme::point_editor)
                    .on_action(move |action| {
                        Message::Edit(EditMessage::PointEdited {
                            block_id: block_id_for_edit,
                            action,
                        })
                    })
                    .key_binding(move |key_press| editor_key_binding(block_id_for_edit, key_press))
                    .height(Length::Shrink);
                if let Some(wid) = self.state.editor_buffers.widget_id(block_id) {
                    editor = editor.id(wid.clone());
                }
                editor
            });

        // Build the bottom row: inline panel bar on left, action bar on right
        let left_col = self.render_overlay_panel_bar(block_id, is_focused, friends_panel_open);

        // Right side: action bar
        let right_col = action_buttons;

        let bottom_row = row![]
            .spacing(theme::ROW_GAP)
            .width(Fill)
            .push(container(left_col).width(Length::Fill))
            .push(right_col);

        let mut block = column![].spacing(theme::BLOCK_INNER_GAP).push(row_content);
        block = block.push(bottom_row);
        if action_bar.status_chip.is_some() {
            block = block.push(
                container(self.render_status_chip(&action_bar))
                    .padding(Padding::ZERO.left(theme::INDENT)),
            );
        }
        if let Some(draft) = self.state.store.expansion_draft(block_id) {
            block = block.push(self.render_expansion_panel(block_id, draft));
        }
        if let Some(draft) = self.state.store.reduction_draft(block_id) {
            block = block.push(self.render_reduction_panel(block_id, draft));
        }

        // Unexpanded mount: show path label below the block.
        if let Some(mount_path) = unexpanded_mount_path {
            block = block.push(
                container(self.render_mount_indicator(block_id, mount_path))
                    .padding(Padding::ZERO.left(theme::INDENT)),
            );
        }

        // Render children only when not folded.
        if !is_collapsed {
            let children = self.state.store.children(block_id);
            if !children.is_empty() {
                block = block.push(
                    container(self.render_line(children))
                        .padding(Padding::ZERO.left(theme::INDENT)),
                );
            }
        }

        let is_focused = self.state.focused_block_id == Some(*block_id);
        let is_picker_target = self.state.friend_picker_for == Some(*block_id);

        // When in friend picker mode, make blocks clickable to select as friend
        if self.state.friend_picker_for.is_some() && !is_picker_target {
            let target = self.state.friend_picker_for.unwrap();
            button(container(block).style(theme::friend_picker_hover))
                .on_press(Message::Structure(StructureMessage::AddFriendBlock {
                    target,
                    friend_id: *block_id,
                }))
                .padding(0)
                .style(theme::action_button)
                .into()
        } else if is_focused {
            container(block).style(theme::focused_block).into()
        } else {
            block.into()
        }
    }

    fn render_expansion_panel(
        &self, block_id: &BlockId, draft: &'a ExpansionDraftRecord,
    ) -> Element<'a, Message> {
        let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

        if let Some(rewrite) = &draft.rewrite {
            let old_text = self.state.store.point(block_id).unwrap_or_default();
            let diff_content = self.render_diff_content(&old_text, rewrite);

            panel = panel.push(
                column![]
                    .spacing(theme::PANEL_INNER_GAP)
                    .push(container(text(t!("doc_rewrite").to_string())).width(Length::Fill))
                    .push(container(diff_content).width(Length::Fill))
                    .push(
                        row![]
                            .width(Length::Fill)
                            .spacing(theme::PANEL_BUTTON_GAP)
                            .push(space::horizontal())
                            .push(
                                button(text(t!("doc_apply_rewrite").to_string()).font(theme::INTER).size(13))
                                    .style(theme::action_button)
                                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                    .on_press(Message::Expand(ExpandMessage::ApplyRewrite(
                                        *block_id,
                                    ))),
                            )
                            .push(
                                button(text(t!("doc_dismiss_rewrite").to_string()).font(theme::INTER).size(13))
                                    .style(theme::destructive_button)
                                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                    .on_press(Message::Expand(ExpandMessage::RejectRewrite(
                                        *block_id,
                                    ))),
                            ),
                    ),
            );
        }

        if !draft.children.is_empty() {
            panel = panel.push(
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(container(text(t!("doc_child_suggestions").to_string())).width(Length::Fill))
                    .push(
                        button(text(t!("doc_accept_all").to_string()).font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Expand(ExpandMessage::AcceptAllChildren(*block_id))),
                    )
                    .push(
                        button(text(t!("doc_discard_all").to_string()).font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Expand(ExpandMessage::DiscardAllChildren(
                                *block_id,
                            ))),
                    ),
            );

            for (index, child) in draft.children.iter().enumerate() {
                panel = panel.push(
                    row![]
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(container(text(child.as_str())).width(Length::Fill))
                        .push(
                            button(text(t!("doc_keep").to_string()).font(theme::INTER).size(13))
                                .style(theme::action_button)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Expand(ExpandMessage::AcceptChild {
                                    block_id: *block_id,
                                    child_index: index,
                                })),
                        )
                        .push(
                            button(text(t!("doc_drop").to_string()).font(theme::INTER).size(13))
                                .style(theme::destructive_button)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Expand(ExpandMessage::RejectChild {
                                    block_id: *block_id,
                                    child_index: index,
                                })),
                        ),
                );
            }
        }

        container(panel)
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .style(theme::draft_panel)
            .into()
    }

    fn render_reduction_panel(
        &self, block_id: &BlockId, draft: &'a ReductionDraftRecord,
    ) -> Element<'a, Message> {
        let old_text = self.state.store.point(block_id).unwrap_or_default();
        let diff_content = self.render_diff_content(&old_text, &draft.reduction);

        let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

        panel = panel
            .push(container(text(t!("doc_reduce").to_string())).width(Length::Fill))
            .push(container(diff_content).width(Length::Fill))
            .push(
                row![]
                    .width(Length::Fill)
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(space::horizontal())
                    .push(
                        button(text(t!("doc_apply_reduction").to_string()).font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::Apply(*block_id))),
                    )
                    .push(
                        button(text(t!("doc_dismiss_reduction").to_string()).font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::Reject(*block_id))),
                    ),
            );

        let valid_children: Vec<(usize, &BlockId)> = draft
            .redundant_children
            .iter()
            .enumerate()
            .filter(|(_, id)| self.state.store.node(id).is_some())
            .collect();

        if !valid_children.is_empty() {
            panel = panel.push(
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(container(text(t!("doc_redundant_children").to_string())).width(Length::Fill))
                    .push(
                        button(text(t!("doc_delete_all").to_string()).font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::AcceptAllDeletions(
                                *block_id,
                            ))),
                    )
                    .push(
                        button(text(t!("doc_keep_all").to_string()).font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::RejectAllDeletions(
                                *block_id,
                            ))),
                    ),
            );

            for (index, child_id) in &valid_children {
                let child_text = self.state.store.point(child_id).unwrap_or_default();
                panel = panel.push(
                    row![]
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(container(text(child_text)).width(Length::Fill))
                        .push(
                            button(text(t!("doc_delete").to_string()).font(theme::INTER).size(13))
                                .style(theme::destructive_button)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Reduce(ReduceMessage::AcceptChildDeletion {
                                    block_id: *block_id,
                                    child_index: *index,
                                })),
                        )
                        .push(
                            button(text(t!("doc_keep").to_string()).font(theme::INTER).size(13))
                                .style(theme::action_button)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Reduce(ReduceMessage::RejectChildDeletion {
                                    block_id: *block_id,
                                    child_index: *index,
                                })),
                        ),
                );
            }
        }

        container(panel)
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .style(theme::draft_panel)
            .into()
    }

    /// Renders the friend blocks panel: list of friend point text, optional perspective, remove button.
    fn render_friends_panel(
        &self, block_id: &BlockId, friends: &'a [FriendBlock],
    ) -> Element<'a, Message> {
        let is_picker_mode = self.state.friend_picker_for == Some(*block_id);

        // Header with "+" button to start friend picker
        let header = row![]
            .spacing(theme::PANEL_BUTTON_GAP)
            .push(container(text(t!("ui_friends").to_string()).font(theme::INTER).size(13)).width(Length::Fill))
            .push(
                button(text("+").font(theme::INTER).size(13))
                    .style(theme::action_button)
                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .on_press(Message::Overlay(OverlayMessage::StartFriendPicker(*block_id))),
            );

        let mut panel =
            column![].spacing(theme::PANEL_INNER_GAP).push(container(header).width(Length::Fill));

        // Show message based on state
        if is_picker_mode {
            // In picker mode - show instruction
            panel = panel.push(
                container(
                    text(t!("doc_friend_picker_hint").to_string())
                        .font(theme::INTER)
                        .size(12),
                )
                .width(Length::Fill),
            );
        } else if friends.is_empty() {
            // Empty state - show instruction to start picker
            panel = panel.push(
                container(
                    text(t!("doc_friend_empty_hint").to_string())
                        .font(theme::INTER)
                        .size(12)
                        .style(theme::spine_text),
                )
                .width(Length::Fill),
            );
        }

        for friend in friends {
            let point_text = self.state.store.point(&friend.block_id).unwrap_or_default();
            let perspective_label = friend.perspective.as_deref().unwrap_or("").trim();
            let friend_id = friend.block_id;
            let target = *block_id;
            let line = if perspective_label.is_empty() {
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(
                        container(text(point_text).font(theme::INTER).size(13)).width(Length::Fill),
                    )
                    .push(
                        button(text(t!("ui_remove").to_string()).font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Structure(StructureMessage::RemoveFriendBlock {
                                target,
                                friend_id,
                            })),
                    )
            } else {
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(
                        column![]
                            .spacing(0)
                            .push(text(point_text).font(theme::INTER).size(13))
                            .push(
                                text(t!("doc_perspective", label = perspective_label).to_string())
                                    .font(theme::INTER)
                                    .size(12)
                                    .style(theme::spine_text),
                            )
                            .width(Length::Fill),
                    )
                    .push(
                        button(text(t!("ui_remove").to_string()).font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Structure(StructureMessage::RemoveFriendBlock {
                                target,
                                friend_id,
                            })),
                    )
            };
            panel = panel.push(line);
        }
        container(panel)
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .style(theme::draft_panel)
            .into()
    }

    fn action_row_context(&self, block_id: &BlockId, point_text: String) -> RowContext {
        let expansion_draft = self.state.store.expansion_draft(block_id);
        let reduction_draft = self.state.store.reduction_draft(block_id);
        let node = self.state.store.node(block_id);
        RowContext {
            block_id: *block_id,
            point_text,
            has_draft: expansion_draft.is_some() || reduction_draft.is_some(),
            draft_suggestion_count: expansion_draft.map(|d| d.children.len()).unwrap_or(0)
                + reduction_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
            has_expand_error: self.state.llm_requests.has_expand_error(*block_id),
            has_reduce_error: self.state.llm_requests.has_reduce_error(*block_id),
            is_expanding: self.state.llm_requests.is_expanding(*block_id),
            is_reducing: self.state.llm_requests.is_reducing(*block_id),
            is_mounted: self.state.store.mount_table().entry(*block_id).is_some(),
            is_unexpanded_mount: node.is_some_and(|n| n.mount_path().is_some()),
            has_children: !self.state.store.children(block_id).is_empty(),
        }
    }

    fn render_diff_content(&self, old_text: &str, new_text: &str) -> Element<'a, Message> {
        let changes = word_diff(old_text, new_text);
        let mut diff_content = column![].spacing(theme::DIFF_LINE_GAP);

        let mut old_line = row![].spacing(0);
        for change in &changes {
            match change {
                | WordChange::Unchanged(s) => {
                    old_line = old_line.push(text(s.clone()).style(theme::diff_context));
                }
                | WordChange::Deleted(s) => {
                    old_line = old_line.push(
                        container(text(s.clone()))
                            .style(theme::diff_deletion)
                            .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
                    );
                }
                | WordChange::Added(_) => {}
            }
        }
        diff_content = diff_content.push(old_line);

        let mut new_line = row![].spacing(0);
        for change in &changes {
            match change {
                | WordChange::Unchanged(s) => {
                    new_line = new_line.push(text(s.clone()).style(theme::diff_context));
                }
                | WordChange::Deleted(_) => {}
                | WordChange::Added(s) => {
                    new_line = new_line.push(
                        container(text(s.clone()))
                            .style(theme::diff_addition)
                            .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
                    );
                }
            }
        }
        diff_content = diff_content.push(new_line);

        container(diff_content).width(Length::Fill).into()
    }

    fn viewport_bucket(&self) -> ViewportBucket {
        ViewportBucket::Wide
    }

    fn render_status_chip(&self, vm: &ActionBarVm) -> Element<'a, Message> {
        let label = match &vm.status_chip {
            | Some(StatusChipVm::Loading { op: ActionId::Expand }) => t!("doc_status_expanding").to_string(),
            | Some(StatusChipVm::Loading { op: ActionId::Reduce }) => t!("doc_status_reducing").to_string(),
            | Some(StatusChipVm::Loading { .. }) => t!("doc_status_working").to_string(),
            | Some(StatusChipVm::Error { message, .. }) => message.clone(),
            | Some(StatusChipVm::DraftActive { suggestion_count }) if *suggestion_count > 0 => {
                t!("doc_status_draft_ready").to_string()
            }
            | Some(StatusChipVm::DraftActive { .. }) => t!("doc_status_draft").to_string(),
            | None => String::new(),
        };

        container(text(label).size(12).font(theme::INTER).style(theme::status_text))
            .padding(Padding::from([theme::CHIP_PAD_V, theme::CHIP_PAD_H]))
            .width(Length::Shrink)
            .into()
    }

    /// Renders the overlay panel bar containing toggle buttons for overlay panels.
    ///
    /// This component lives below each block's editor and provides toggles for panels
    /// that can be shown inline (as opposed to draft panels which appear below).
    ///
    /// The toggle buttons reflect panel-open state independently:
    /// - `Friends` is highlighted only when `PanelBarState::Friends` is open.
    /// - `Instruction` is highlighted only when `PanelBarState::Instruction` is open.
    fn render_overlay_panel_bar(
        &self, block_id: &BlockId, is_focused: bool, friends_panel_open: bool,
    ) -> Element<'a, Message> {
        if !is_focused {
            return column![].into();
        }

        use crate::app::PanelBarState;
        let instruction_panel_open =
            matches!(self.state.panel_bar_state, Some(PanelBarState::Instruction));

        let mut button_row = row![].spacing(theme::PANEL_BUTTON_GAP);
        button_row = button_row.push(
            button(text(t!("ui_friends").to_string()).font(theme::INTER).size(13))
                .style(move |theme, status| {
                    theme::panel_toggle_button(theme, status, friends_panel_open)
                })
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::Overlay(OverlayMessage::ToggleFriendsPanel(*block_id))),
        );
        button_row = button_row.push(
            button(text(t!("ui_instruction").to_string()).font(theme::INTER).size(13))
                .style(move |theme, status| {
                    theme::panel_toggle_button(theme, status, instruction_panel_open)
                })
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(instruction_panel::InstructionPanelMessage::Toggle)
                        .into(),
                ),
        );

        let mut col =
            column![].push(container(button_row).padding(Padding::ZERO.right(theme::INDENT)));

        match self.state.panel_bar_state {
            | Some(PanelBarState::Friends) => {
                let friends = self.state.store.friend_blocks_for(block_id);
                col = col.push(
                    container(self.render_friends_panel(block_id, friends)).width(Length::Fill),
                );
            }
            | Some(PanelBarState::Instruction) => {
                col = col.push(container(instruction_panel::view(&self.state)).width(Length::Fill));
            }
            | None => {}
        }

        col.into()
    }

    fn render_action_buttons(&self, block_id: &BlockId, vm: &ActionBarVm) -> Element<'a, Message> {
        let is_overflow_open = self.state.overflow_open_for.as_ref() == Some(block_id);
        let mut actions_row = row![].spacing(theme::ACTION_GAP);

        // Always show primary actions
        for descriptor in vm.visible_actions() {
            actions_row = actions_row.push(self.render_action_button(block_id, &descriptor));
        }

        // Show "More" button when closed, or "Close" button at end when open
        if !vm.overflow.is_empty() {
            if is_overflow_open {
                // When open, show overflow actions first, then close button at the end
                for descriptor in &vm.overflow {
                    actions_row = actions_row.push(self.render_action_button(block_id, descriptor));
                }
                let btn = button(centered_icon(icons::icon_x().size(16).into()))
                    .style(theme::action_button)
                    .padding(0)
                    .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .on_press(Message::Overlay(OverlayMessage::ToggleOverflow(*block_id)));

                actions_row = actions_row.push(
                    tooltip(
                        btn,
                        text(t!("ui_close").to_string()).size(12).font(theme::INTER),
                        tooltip::Position::Bottom,
                    )
                    .style(theme::tooltip)
                    .padding(theme::TOOLTIP_PAD)
                    .gap(theme::TOOLTIP_GAP),
                );
            } else {
                // When closed, show "More" button
                let btn = button(centered_icon(icons::icon_ellipsis().size(16).into()))
                    .style(theme::action_button)
                    .padding(0)
                    .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .on_press(Message::Overlay(OverlayMessage::ToggleOverflow(*block_id)));

                actions_row = actions_row.push(
                    tooltip(
                        btn,
                        text(t!("ui_more").to_string()).size(12).font(theme::INTER),
                        tooltip::Position::Bottom,
                    )
                    .style(theme::tooltip)
                    .padding(theme::TOOLTIP_PAD)
                    .gap(theme::TOOLTIP_GAP),
                );
            }
        }

        actions_row.into()
    }

    fn render_action_button(
        &self, block_id: &BlockId, descriptor: &ActionDescriptor,
    ) -> Element<'a, Message> {
        let style = if descriptor.destructive {
            theme::destructive_button as fn(&iced::Theme, button::Status) -> button::Style
        } else {
            theme::action_button
        };
        let icon = centered_icon(action_icon(descriptor.id));
        let base = button(icon)
            .style(style)
            .padding(0)
            .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE));
        let btn = if descriptor.availability == ActionAvailability::Enabled {
            if let Some(message) = action_to_message(self.state, block_id, descriptor) {
                base.on_press(message)
            } else {
                base
            }
        } else {
            base
        };
        tooltip(btn, text(descriptor.label).size(12).font(theme::INTER), tooltip::Position::Bottom)
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP)
            .into()
    }

    /// Render a mount indicator showing the file path.
    ///
    /// Displayed below the node's own content for unexpanded mounts,
    /// indicating that children live in an external file.
    /// The chevron marker handles load/unload; this only shows the path.
    fn render_mount_indicator(
        &self, _block_id: &BlockId, mount_path: &'a std::path::Path,
    ) -> Element<'a, Message> {
        text(mount_path.display().to_string())
            .font(theme::INTER)
            .size(13)
            .style(theme::spine_text)
            .into()
    }
}

fn editor_key_binding(
    block_id: BlockId, key_press: text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    if let Some(action_id) = shortcut_to_action(key_press.key.clone(), key_press.modifiers) {
        return Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
            block_id,
            action_id,
        })));
    }

    text_editor::Binding::from_key_press(key_press)
}

fn action_icon<'a>(id: ActionId) -> Element<'a, Message> {
    let icon = match id {
        | ActionId::Expand => icons::icon_maximize_2(),
        | ActionId::Reduce => icons::icon_minimize_2(),
        | ActionId::Cancel => icons::icon_circle_x(),
        | ActionId::AddChild => icons::icon_corner_down_right(),
        | ActionId::AddParent => icons::icon_corner_up_left(),
        | ActionId::AcceptAll => icons::icon_check_check(),
        | ActionId::Retry => icons::icon_refresh_cw(),
        | ActionId::DismissDraft => icons::icon_x(),
        | ActionId::CollapseBranch => icons::icon_chevron_down(),
        | ActionId::ExpandBranch => icons::icon_chevron_right(),
        | ActionId::AddSibling => icons::icon_plus(),
        | ActionId::DuplicateBlock => icons::icon_copy(),
        | ActionId::ArchiveBlock => icons::icon_archive(),
        | ActionId::SaveToFile => icons::icon_hard_drive_download(),
        | ActionId::LoadFromFile => icons::icon_hard_drive_upload(),
    };
    icon.size(16).line_height(iced::widget::text::LineHeight::Relative(1.0)).into()
}

fn centered_icon<'a>(icon: Element<'a, Message>) -> Element<'a, Message> {
    container(icon)
        .padding(theme::BUTTON_PAD)
        .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}
