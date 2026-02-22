use super::action_bar::{
    ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, RowContext, StatusChipVm,
    ViewportBucket, action_to_message, build_action_bar_vm, project_for_viewport,
    shortcut_to_action,
};
use super::diff::{WordChange, word_diff};
use super::{
    AppState, EditMessage, ExpandMessage, ExpandState, Message, MountFileMessage, OverlayMessage,
    ReduceMessage, ReduceState, ShortcutMessage, StructureMessage,
};
use crate::store::BlockId;
use crate::store::{ExpansionDraftRecord, ReductionDraftRecord};
use crate::theme;
use iced::widget::{button, column, container, row, rule, text, text_editor, tooltip};
use iced::{Element, Fill, Length, Padding};
use lucide_icons::iced as icons;

fn action_icon<'a>(id: ActionId) -> Element<'a, Message> {
    let icon = match id {
        | ActionId::Expand => icons::icon_maximize_2(),
        | ActionId::Reduce => icons::icon_minimize_2(),
        | ActionId::Cancel => icons::icon_circle_x(),
        | ActionId::AddChild => icons::icon_corner_down_right(),
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

        let Some(editor_content) = self.state.editors.get(block_id) else {
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
        let is_collapsed = self.state.collapsed.contains(block_id);
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
            container(text("\u{2022}").size(12).style(theme::spine_text))
                .width(Length::Fixed(theme::MARKER_WIDTH))
                .align_x(iced::alignment::Horizontal::Center)
                .padding(Padding::ZERO.top(theme::MARKER_TOP))
                .into()
        };

        let action_buttons: Element<'a, Message> =
            self.render_action_buttons(block_id, &action_bar);

        let row_content = row![]
            .spacing(theme::ROW_GAP)
            .width(Fill)
            .align_y(iced::Alignment::Start)
            .push(spine)
            .push(marker)
            .push({
                let mut editor = text_editor(editor_content)
                    .placeholder("point")
                    .style(theme::point_editor)
                    .on_action(move |action| {
                        Message::Edit(EditMessage::PointEdited {
                            block_id: block_id_for_edit,
                            action,
                        })
                    })
                    .key_binding(move |key_press| editor_key_binding(block_id_for_edit, key_press))
                    .height(Length::Shrink);
                if let Some(wid) = self.state.editors.widget_id(block_id) {
                    editor = editor.id(wid.clone());
                }
                editor
            })
            .push(action_buttons);

        let mut block = column![].spacing(theme::BLOCK_INNER_GAP).push(row_content);
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

        let is_active = self.state.focused_block_id == Some(*block_id)
            || self.state.active_block_id == Some(*block_id);
        if is_active { container(block).style(theme::active_block).into() } else { block.into() }
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
                    .push(container(text("Rewrite")).width(Length::Fill))
                    .push(container(diff_content).width(Length::Fill))
                    .push(
                        row![]
                            .spacing(theme::PANEL_BUTTON_GAP)
                            .push(
                                button(text("Apply rewrite").font(theme::INTER).size(13))
                                    .style(theme::action_button)
                                    .on_press(Message::Expand(ExpandMessage::ApplyRewrite(
                                        *block_id,
                                    ))),
                            )
                            .push(
                                button(text("Dismiss rewrite").font(theme::INTER).size(13))
                                    .style(theme::destructive_button)
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
                    .push(container(text("Child suggestions")).width(Length::Fill))
                    .push(
                        button(text("Accept all").font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .on_press(Message::Expand(ExpandMessage::AcceptAllChildren(*block_id))),
                    )
                    .push(
                        button(text("Discard all").font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .on_press(Message::Expand(ExpandMessage::Discard(*block_id))),
                    ),
            );

            for (index, child) in draft.children.iter().enumerate() {
                panel = panel.push(
                    row![]
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(container(text(child.as_str())).width(Length::Fill))
                        .push(
                            button(text("Keep").font(theme::INTER).size(13))
                                .style(theme::action_button)
                                .on_press(Message::Expand(ExpandMessage::AcceptChild {
                                    block_id: *block_id,
                                    child_index: index,
                                })),
                        )
                        .push(
                            button(text("Drop").font(theme::INTER).size(13))
                                .style(theme::destructive_button)
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

        container(
            column![]
                .spacing(theme::PANEL_INNER_GAP)
                .push(container(text("Reduce")).width(Length::Fill))
                .push(container(diff_content).width(Length::Fill))
                .push(
                    row![]
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(
                            button(text("Apply reduction").font(theme::INTER).size(13))
                                .style(theme::action_button)
                                .on_press(Message::Reduce(ReduceMessage::Apply(*block_id))),
                        )
                        .push(
                            button(text("Dismiss reduction").font(theme::INTER).size(13))
                                .style(theme::destructive_button)
                                .on_press(Message::Reduce(ReduceMessage::Reject(*block_id))),
                        ),
                ),
        )
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
            draft_suggestion_count: expansion_draft.map(|d| d.children.len()).unwrap_or(0),
            has_expand_error: self
                .state
                .expand_states
                .get(*block_id)
                .is_some_and(|s| matches!(s, ExpandState::Error { .. })),
            has_reduce_error: self
                .state
                .reduce_states
                .get(*block_id)
                .is_some_and(|s| matches!(s, ReduceState::Error { .. })),
            is_expanding: self.state.is_expanding(block_id),
            is_reducing: self.state.is_reducing(block_id),
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
            | Some(StatusChipVm::Loading { op: ActionId::Expand }) => "Expanding...".to_string(),
            | Some(StatusChipVm::Loading { op: ActionId::Reduce }) => "Reducing...".to_string(),
            | Some(StatusChipVm::Loading { .. }) => "Working...".to_string(),
            | Some(StatusChipVm::Error { message, .. }) => message.clone(),
            | Some(StatusChipVm::DraftActive { suggestion_count }) if *suggestion_count > 0 => {
                "Draft ready".to_string()
            }
            | Some(StatusChipVm::DraftActive { .. }) => "Draft".to_string(),
            | None => String::new(),
        };

        container(text(label).size(12).font(theme::INTER).style(theme::status_text))
            .padding(Padding::from([theme::CHIP_PAD_V, theme::CHIP_PAD_H]))
            .width(Length::Shrink)
            .into()
    }

    fn render_action_buttons(&self, block_id: &BlockId, vm: &ActionBarVm) -> Element<'a, Message> {
        let mut actions_row = row![].spacing(theme::ACTION_GAP);

        for descriptor in vm.visible_actions() {
            actions_row = actions_row.push(self.render_action_button(block_id, &descriptor));
        }

        if !vm.overflow.is_empty() {
            let is_open = self.state.overflow_open_for.as_ref() == Some(block_id);
            let (icon, label) =
                if is_open { (icons::icon_x(), "Close") } else { (icons::icon_ellipsis(), "More") };
            let btn = button(centered_icon(icon.size(16).into()))
                .style(theme::action_button)
                .padding(0)
                .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::Overlay(OverlayMessage::ToggleOverflow(*block_id)));

            actions_row = actions_row.push(
                tooltip(btn, text(label).size(12).font(theme::INTER), tooltip::Position::Bottom)
                    .style(theme::tooltip)
                    .padding(theme::TOOLTIP_PAD)
                    .gap(theme::TOOLTIP_GAP),
            );
        }

        let mut layout = column![].spacing(theme::BLOCK_INNER_GAP).push(actions_row);
        if self.state.overflow_open_for.as_ref() == Some(block_id) {
            let mut overflow = row![].spacing(theme::ACTION_GAP);
            for descriptor in &vm.overflow {
                overflow = overflow.push(self.render_action_button(block_id, descriptor));
            }
            layout = layout
                .push(container(overflow).padding(Padding::from([theme::OVERFLOW_PAD_V, 0.0])));
        }

        layout.into()
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
