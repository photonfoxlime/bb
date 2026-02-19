use super::action_bar::{
    ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, RowContext, StatusChipVm,
    ViewportBucket, action_to_message, build_action_bar_vm, project_for_viewport,
};
use super::{AppState, ExpandState, ExpansionDraft, Message, SummaryState};
use crate::graph::{BlockId, BlockNode};
use crate::theme;
use iced::widget::{button, column, container, row, rule, text, text_editor, tooltip};
use iced::{Element, Fill, Length};
use lucide_icons::iced as icons;

fn action_icon<'a>(id: ActionId) -> Element<'a, Message> {
    let icon = match id {
        | ActionId::Expand => icons::icon_maximize_2(),
        | ActionId::Reduce => icons::icon_minimize_2(),
        | ActionId::AddChild => icons::icon_corner_down_right(),
        | ActionId::AcceptAll => icons::icon_check_check(),
        | ActionId::Retry => icons::icon_refresh_cw(),
        | ActionId::DismissDraft => icons::icon_x(),
        | ActionId::CollapseBranch => icons::icon_chevron_down(),
        | ActionId::ExpandBranch => icons::icon_chevron_right(),
        | ActionId::AddSibling => icons::icon_plus(),
        | ActionId::OpenAsFocus => icons::icon_arrow_right(),
        | ActionId::DuplicateBlock => icons::icon_copy(),
        | ActionId::ArchiveBlock => icons::icon_archive(),
        | ActionId::Overflow => text("?"),
    };
    icon.size(16).into()
}

pub(super) struct TreeView<'a> {
    state: &'a AppState,
}

impl<'a> TreeView<'a> {
    pub(super) fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub(super) fn render_roots(&self) -> Element<'a, Message> {
        self.render_line(self.state.graph.roots())
    }

    fn render_line(&self, ids: &'a [BlockId]) -> Element<'a, Message> {
        let mut col = column![].spacing(10);
        for id in ids {
            let Some(node) = self.state.graph.node(id) else {
                continue;
            };
            col = col.push(self.render_block(id, node));
        }
        col.into()
    }

    fn render_block(&self, block_id: &BlockId, node: &'a BlockNode) -> Element<'a, Message> {
        let editor_content =
            self.state.editors.get(block_id).expect("editor content is populated from graph");

        let block_id_for_edit = block_id.clone();
        let row_context = self.action_row_context(block_id, editor_content.text(), node);
        let action_bar =
            project_for_viewport(build_action_bar_vm(&row_context), self.viewport_bucket());

        let spine = container(rule::vertical(1).style(theme::spine_rule))
            .width(Length::Fixed(4.0))
            .align_x(iced::alignment::Horizontal::Center);
        let marker = container(text("•").size(12).style(theme::spine_text))
            .width(Length::Fixed(12.0))
            .align_x(iced::alignment::Horizontal::Center)
            .padding(iced::Padding::ZERO.top(3.0));

        let row_content = row![]
            .spacing(6)
            .width(Fill)
            .align_y(iced::Alignment::Start)
            .push(spine)
            .push(marker)
            .push(
                text_editor(editor_content)
                    .placeholder("point")
                    .style(theme::point_editor)
                    .on_action(move |action| {
                        Message::PointEdited(block_id_for_edit.clone(), action)
                    })
                    .height(Length::Shrink),
            )
            .push(self.render_action_buttons(block_id, &action_bar));

        let mut block = column![].spacing(4).push(row_content);
        if action_bar.status_chip.is_some() {
            block = block.push(
                container(self.render_status_chip(&action_bar))
                    .padding(iced::Padding::ZERO.left(16.0)),
            );
        }
        if let Some(draft) = self.state.expansion_drafts.get(block_id) {
            block = block.push(self.render_expansion_panel(block_id, draft));
        }

        if !node.children.is_empty() {
            block = block.push(
                container(self.render_line(&node.children)).padding(iced::Padding::ZERO.left(16.0)),
            );
        }
        block.into()
    }

    fn render_expansion_panel(
        &self, block_id: &BlockId, draft: &'a ExpansionDraft,
    ) -> Element<'a, Message> {
        let mut panel = column![].spacing(6);

        if let Some(rewrite) = &draft.rewrite {
            panel = panel.push(
                row![]
                    .spacing(8)
                    .push(container(text(format!("Rewrite: {}", rewrite))).width(Length::Fill))
                    .push(
                        button(text("Apply rewrite").font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .on_press(Message::ApplyExpandedRewrite(block_id.clone())),
                    )
                    .push(
                        button(text("Dismiss rewrite").font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .on_press(Message::RejectExpandedRewrite(block_id.clone())),
                    ),
            );
        }

        if !draft.children.is_empty() {
            panel = panel.push(
                row![]
                    .spacing(8)
                    .push(container(text("Child suggestions")).width(Length::Fill))
                    .push(
                        button(text("Accept all").font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .on_press(Message::AcceptAllExpandedChildren(block_id.clone())),
                    )
                    .push(
                        button(text("Discard all").font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .on_press(Message::DiscardExpansion(block_id.clone())),
                    ),
            );

            for (index, child) in draft.children.iter().enumerate() {
                panel = panel.push(
                    row![]
                        .spacing(8)
                        .push(container(text(child.as_str())).width(Length::Fill))
                        .push(
                            button(text("Keep").font(theme::INTER).size(13))
                                .style(theme::action_button)
                                .on_press(Message::AcceptExpandedChild(block_id.clone(), index)),
                        )
                        .push(
                            button(text("Drop").font(theme::INTER).size(13))
                                .style(theme::destructive_button)
                                .on_press(Message::RejectExpandedChild(block_id.clone(), index)),
                        ),
                );
            }
        }

        container(panel).padding(iced::Padding::from([8.0, 16.0])).style(theme::draft_panel).into()
    }

    fn action_row_context(
        &self, block_id: &BlockId, point_text: String, _node: &BlockNode,
    ) -> RowContext {
        let draft = self.state.expansion_drafts.get(block_id);
        RowContext {
            block_id: block_id.clone(),
            point_text,
            has_draft: draft.is_some(),
            draft_suggestion_count: draft.map(|d| d.children.len()).unwrap_or(0),
            has_expand_error: matches!(&self.state.expand_state, ExpandState::Error { block_id: id, .. } if id == block_id),
            has_reduce_error: matches!(&self.state.summary_state, SummaryState::Error { block_id: id, .. } if id == block_id),
            is_expanding: self.state.is_expanding(block_id),
            is_reducing: self.state.is_summarizing(block_id),
        }
    }

    fn viewport_bucket(&self) -> ViewportBucket {
        ViewportBucket::Wide
    }

    fn render_status_chip(&self, vm: &ActionBarVm) -> Element<'a, Message> {
        let label = match &vm.status_chip {
            | Some(StatusChipVm::Loading { op: ActionId::Expand }) => "Expanding...".to_string(),
            | Some(StatusChipVm::Loading { op: ActionId::Reduce }) => "Summarizing...".to_string(),
            | Some(StatusChipVm::Loading { .. }) => "Working...".to_string(),
            | Some(StatusChipVm::Error { message, .. }) => message.clone(),
            | Some(StatusChipVm::DraftActive { suggestion_count }) if *suggestion_count > 0 => {
                "Draft ready".to_string()
            }
            | Some(StatusChipVm::DraftActive { .. }) => "Draft".to_string(),
            | None => String::new(),
        };

        container(text(label).size(12).font(theme::INTER).style(theme::status_text))
            .padding(iced::Padding::from([2.0, 8.0]))
            .width(Length::Shrink)
            .into()
    }

    fn render_action_buttons(&self, block_id: &BlockId, vm: &ActionBarVm) -> Element<'a, Message> {
        let mut actions_row = row![].spacing(6);

        for descriptor in vm.visible_actions() {
            actions_row = actions_row.push(self.render_action_button(block_id, &descriptor));
        }

        if !vm.overflow.is_empty() {
            let is_open = self.state.overflow_open_for.as_ref() == Some(block_id);
            let (icon, label) =
                if is_open { (icons::icon_x(), "Close") } else { (icons::icon_ellipsis(), "More") };
            let btn = button(icon.size(16))
                .style(theme::action_button)
                .padding(4)
                .on_press(Message::ToggleOverflow(block_id.clone()));

            actions_row = actions_row.push(
                tooltip(btn, text(label).size(12).font(theme::INTER), tooltip::Position::Bottom)
                    .style(theme::tooltip)
                    .padding(6)
                    .gap(4),
            );
        }

        let mut layout = column![].spacing(4).push(actions_row);
        if self.state.overflow_open_for.as_ref() == Some(block_id) {
            let mut overflow = row![].spacing(6);
            for descriptor in &vm.overflow {
                overflow = overflow.push(self.render_action_button(block_id, descriptor));
            }
            layout = layout.push(container(overflow).padding(iced::Padding::from([4.0, 0.0])));
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
        let icon = action_icon(descriptor.id);
        let base = button(icon).style(style).padding(4);
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
            .padding(6)
            .gap(4)
            .into()
    }
}
