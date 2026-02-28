//! Instruction panel for LLM interactions.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! This module defines the behavior contract for instruction-driven operations
//! from one focused block. A block's visible context is the union of:
//! - the block point itself,
//! - the full parent chain (root -> target),
//! - all direct children of the target,
//! - all user-selected friend blocks for the target.
//!
//! # Instruction Draft Lifecycle
//!
//! The instruction editor is treated as a short-lived draft buffer whose text is
//! authored before submission through one of three actions:
//! - `Inquire`: ask a free-form question against visible context and surface a
//!   response draft for user-directed insertion.
//! - `Expand`: run normal expand semantics with the draft injected as extra
//!   instruction.
//! - `Reduce`: run normal reduce semantics with the draft injected as extra
//!   instruction.
//!
//! # Inquire Result Contract
//!
//! Inquiry returns one response draft scoped to the focused block context at the
//! time of submission. The product intent is that this draft can be inserted by
//! the user either into the current point or as a new child point.
//!
//! # Expand/Reduce Contract
//!
//! Expand and reduce preserve their canonical semantics from `crate::llm`
//! prompt builders; instruction text only adds additional guidance and does not
//! redefine the output schema.
//!
//! # Inquiry Apply Operations
//!
//! Inquiry drafts currently support three explicit apply actions:
//! - replace target point with response,
//! - append response to target point,
//! - add response as a new child under the target.

use crate::app::{AppState, Message, PanelBarState};
use crate::llm;
use crate::store::BlockId;
use crate::theme;
use rust_i18n::t;

use iced::Element;
use iced::widget::{button, container, text, text_editor};
use lucide_icons::iced as icons;
use std::time::Duration;

const LLM_REQUEST_TIMEOUT: Duration = theme::INSTRUCTION_LLM_TIMEOUT;

/// Message types for instruction panel interactions.
#[derive(Debug, Clone)]
pub enum InstructionPanelMessage {
    /// Toggle instruction panel visibility for the focused block.
    Toggle,
    /// Text edited in the instruction panel.
    TextEdited(iced::widget::text_editor::Action),
    /// Send inquiry to LLM with the instruction.
    Inquire,
    /// Inquiry request completed.
    InquireDone { block_id: BlockId, result: Result<String, crate::app::UiError> },
    /// Cancel an in-flight inquiry request.
    CancelInquire,
    /// Expand with instruction as system prompt.
    ExpandWithInstruction,
    /// Reduce with instruction as system prompt.
    ReduceWithInstruction,
    /// Apply rewrite from inquiry result.
    ApplyInstructionRewrite,
    /// Append inquiry result to the target block point.
    AppendInstructionResponse,
    /// Add inquiry result as a new child under the target block.
    AddInstructionResponseAsChild,
    /// Dismiss inquiry result.
    Dismiss,
}

/// Handle instruction panel messages.
/// The block_id parameter is only needed for Toggle to check focus match.
pub fn handle(
    state: &mut AppState, target_block_id: BlockId, msg: InstructionPanelMessage,
) -> iced::Task<Message> {
    use crate::app::{ExpandMessage, ReduceMessage};

    match msg {
        | InstructionPanelMessage::Toggle => {
            let current_state = state.store.panel_state(&target_block_id).copied();
            if matches!(current_state, Some(PanelBarState::Instruction)) {
                state.store.set_panel_state(&target_block_id, None);
            } else {
                state.store.set_panel_state(&target_block_id, Some(PanelBarState::Instruction));
                sync_instruction_panel_from_store(state, &target_block_id);
            }
            state.persist_with_context("after toggling instruction panel");
            iced::Task::none()
        }
        | InstructionPanelMessage::TextEdited(action) => {
            state.editor_buffers.instruction_content_mut().perform(action);
            if state.store.node(&target_block_id).is_none() {
                let updated_text = state.editor_buffers.instruction_content().text().to_string();
                state.store.set_instruction_draft(target_block_id, updated_text);
                state.persist_with_context("after editing instruction draft");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::Inquire => {
            let instruction = state.editor_buffers.instruction_content().text().trim().to_string();
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.llm_requests.mark_inquiry_loading(target_block_id);
            let context = state.store.block_context_for_id(&target_block_id);
            let config = match state.providers.resolve_active() {
                | Ok(c) => c,
                | Err(_) => return iced::Task::none(),
            };
            state.store.set_inquiry(target_block_id, instruction.clone());
            state.store.remove_instruction_draft(&target_block_id);
            state.persist_with_context("after storing inquiry and consuming instruction draft");
            state.editor_buffers.set_instruction_text("");
            tracing::info!(block_id = ?target_block_id, "instruction inquiry started");
            let request_task = iced::Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.inquire(&context, &instruction),
                        )
                        .await,
                        "inquire request timed out after 30 seconds",
                    )
                },
                move |result| {
                    Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::InquireDone { block_id: target_block_id, result },
                    )
                },
            );
            let (request_task, handle) = iced::Task::abortable(request_task);
            state.llm_requests.replace_inquiry_handle(target_block_id, handle);
            request_task
        }
        | InstructionPanelMessage::InquireDone { block_id, result } => {
            state.llm_requests.finish_inquiry_request(block_id);
            match result {
                | Ok(response) => {
                    tracing::info!(
                        block_id = ?block_id,
                        chars = response.len(),
                        "instruction inquiry succeeded"
                    );
                    state.store.set_inquiry_draft(block_id, response);
                    state.persist_with_context("after persisting instruction inquiry draft");
                }
                | Err(reason) => {
                    tracing::error!(
                        block_id = ?block_id,
                        reason = %reason.as_str(),
                        "instruction inquiry failed"
                    );
                    state.record_error(crate::app::AppError::Inquire(reason));
                }
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::CancelInquire => {
            if state.llm_requests.cancel_inquiry(target_block_id) {
                tracing::info!(block_id = ?target_block_id, "instruction inquiry cancelled");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::ExpandWithInstruction => {
            let instruction = state.editor_buffers.instruction_content().text().trim().to_string();
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.store.set_instruction_draft(target_block_id, instruction.clone());
            state.persist_with_context("after persisting instruction draft for expand");
            state.editor_buffers.set_instruction_text("");
            state.store.set_panel_state(&target_block_id, None);
            state.persist_with_context("after closing instruction panel");
            crate::app::AppState::update(
                state,
                Message::Expand(ExpandMessage::Start(target_block_id)),
            )
        }
        | InstructionPanelMessage::ReduceWithInstruction => {
            let instruction = state.editor_buffers.instruction_content().text().trim().to_string();
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.store.set_instruction_draft(target_block_id, instruction.clone());
            state.persist_with_context("after persisting instruction draft for reduce");
            state.editor_buffers.set_instruction_text("");
            state.store.set_panel_state(&target_block_id, None);
            state.persist_with_context("after closing instruction panel");
            crate::app::AppState::update(
                state,
                Message::Reduce(ReduceMessage::Start(target_block_id)),
            )
        }
        | InstructionPanelMessage::ApplyInstructionRewrite => {
            if state.store.inquiry_draft(&target_block_id).is_none() {
                tracing::error!(block_id = ?target_block_id, "no inquiry draft found");
                return iced::Task::none();
            }
            let block_id = target_block_id;
            if let Some(inquiry_draft) = state.store.inquiry_draft(&block_id) {
                let rewrite = inquiry_draft.response.clone();
                state.mutate_with_undo_and_persist("after applying instruction rewrite", |state| {
                    state.store.update_point(&block_id, rewrite.clone());
                    state.editor_buffers.set_text(&block_id, &rewrite);
                    true
                });
                state.store.remove_inquiry_draft(&block_id);
                state.persist_with_context("after clearing inquiry draft by rewrite apply");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::AppendInstructionResponse => {
            if state.store.inquiry_draft(&target_block_id).is_none() {
                tracing::error!(block_id = ?target_block_id, "no inquiry draft found");
                return iced::Task::none();
            }
            let block_id = target_block_id;
            if let Some(inquiry_draft) = state.store.inquiry_draft(&block_id) {
                let response = inquiry_draft.response.clone();
                state.mutate_with_undo_and_persist(
                    "after appending instruction inquiry response",
                    |state| {
                        let current = state.store.point(&block_id).unwrap_or_default();
                        let next = if current.trim().is_empty() {
                            response.clone()
                        } else {
                            format!("{current}\n\n{response}")
                        };
                        state.store.update_point(&block_id, next.clone());
                        state.editor_buffers.set_text(&block_id, &next);
                        true
                    },
                );
                state.store.remove_inquiry_draft(&block_id);
                state.persist_with_context("after clearing inquiry draft by append apply");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::AddInstructionResponseAsChild => {
            if state.store.inquiry_draft(&target_block_id).is_none() {
                tracing::error!(block_id = ?target_block_id, "no inquiry draft found");
                return iced::Task::none();
            }
            let block_id = target_block_id;
            if let Some(inquiry_draft) = state.store.inquiry_draft(&block_id) {
                let response = inquiry_draft.response.clone();
                state.mutate_with_undo_and_persist(
                    "after adding instruction inquiry response as child",
                    |state| {
                        if let Some(child_id) =
                            state.store.append_child(&block_id, response.clone())
                        {
                            state.editor_buffers.set_text(&child_id, &response);
                            return true;
                        }
                        false
                    },
                );
                state.store.remove_inquiry_draft(&block_id);
                state.persist_with_context("after clearing inquiry draft by add-child apply");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::Dismiss => {
            state.store.remove_inquiry_draft(&target_block_id);
            state.persist_with_context("after dismissing inquiry draft");
            iced::Task::none()
        }
    }
}

fn sync_instruction_panel_from_store(state: &mut AppState, target_block_id: &BlockId) {
    let instruction = state
        .store
        .instruction_draft(target_block_id)
        .map(|draft| draft.instruction.clone())
        .unwrap_or_default();
    state.editor_buffers.set_instruction_text(&instruction);
}

/// Render the instruction panel for the focused block.
pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    use crate::store::PanelBarState;
    use iced::Padding;
    use iced::widget::{column, row, scrollable};

    let block_id = match state.focus().map(|s| s.block_id) {
        | Some(id) if matches!(state.store.panel_state(&id), Some(PanelBarState::Instruction)) => {
            id
        }
        | _ => return container(iced::widget::Text::new("")).into(),
    };

    let instruction_content = state.editor_buffers.instruction_content();
    let inquiry_result = state.store.inquiry_draft(&block_id);
    let is_inquiring = state.llm_requests.is_inquiring(block_id);

    let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

    if is_inquiring || inquiry_result.is_some() {
        let inquiry_text = inquiry_result.as_ref().map(|r| r.inquiry.as_str()).unwrap_or_default();

        let inquiry_section = column![].spacing(theme::PANEL_INNER_GAP).push(
            container(
                text(t!("instruction_inquiry_label").to_string())
                    .font(theme::INTER)
                    .size(theme::INSTRUCTION_BUTTON_SIZE),
            )
            .width(iced::Length::Fill),
        );

        let inquiry_content = container(
            scrollable(text(inquiry_text).font(theme::LXGW_WENKAI).size(14))
                .width(iced::Length::Fill),
        )
        .padding(Padding::from([6.0, 8.0]))
        .style(theme::draft_panel)
        .width(iced::Length::Fill);

        panel = panel.push(inquiry_section);
        panel = panel.push(inquiry_content);
    } else {
        let mut instruction_section = column![].spacing(theme::PANEL_INNER_GAP);

        instruction_section = instruction_section.push(
            container(
                text_editor(instruction_content)
                    .placeholder(t!("instruction_placeholder").to_string())
                    .style(theme::point_editor)
                    .height(theme::INSTRUCTION_EDITOR_HEIGHT)
                    .on_action(move |action| {
                        Message::InstructionPanel(
                            block_id,
                            InstructionPanelMessage::TextEdited(action),
                        )
                    }),
            )
            .width(iced::Length::Fill),
        );

        let mut button_row = row![].spacing(theme::PANEL_BUTTON_GAP);

        button_row = button_row.push(
            button(
                text(t!("instruction_inquire").to_string())
                    .font(theme::INTER)
                    .size(theme::INSTRUCTION_BUTTON_SIZE),
            )
            .style(theme::action_button)
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(block_id, InstructionPanelMessage::Inquire)),
        );

        button_row = button_row.push(
            button(
                text(t!("instruction_expand").to_string())
                    .font(theme::INTER)
                    .size(theme::INSTRUCTION_BUTTON_SIZE),
            )
            .style(theme::action_button)
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(
                block_id,
                InstructionPanelMessage::ExpandWithInstruction,
            )),
        );

        button_row = button_row.push(
            button(
                text(t!("instruction_reduce").to_string())
                    .font(theme::INTER)
                    .size(theme::INSTRUCTION_BUTTON_SIZE),
            )
            .style(theme::action_button)
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(
                block_id,
                InstructionPanelMessage::ReduceWithInstruction,
            )),
        );

        instruction_section = instruction_section.push(button_row);
        panel = panel.push(instruction_section);
    }

    if is_inquiring {
        let button_row = row![].spacing(theme::PANEL_BUTTON_GAP).push(
            button(
                row![]
                    .spacing(4)
                    .align_y(iced::Alignment::Center)
                    .push(icons::icon_loader().size(theme::INSTRUCTION_BUTTON_SIZE).center())
                    .push(
                        text(t!("instruction_inquiring").to_string())
                            .font(theme::INTER)
                            .size(theme::INSTRUCTION_BUTTON_SIZE),
                    ),
            )
            .style(theme::destructive_button)
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(block_id, InstructionPanelMessage::CancelInquire)),
        );
        panel = panel.push(button_row);
    }

    'inquiry: {
        if let Some(result) = inquiry_result {
            let response_text = result.response.as_str();
            if response_text.is_empty() {
                break 'inquiry;
            }
            let mut result_section = column![].spacing(theme::PANEL_INNER_GAP);

            result_section = result_section.push(
                container(
                    text(t!("instruction_response").to_string())
                        .font(theme::INTER)
                        .size(theme::INSTRUCTION_BUTTON_SIZE),
                )
                .width(iced::Length::Fill),
            );

            let result_content = container(
                scrollable(text(response_text).font(theme::LXGW_WENKAI).size(14))
                    .width(iced::Length::Fill),
            )
            .padding(Padding::from([6.0, 8.0]))
            .style(theme::draft_panel)
            .width(iced::Length::Fill);

            result_section = result_section.push(result_content);

            let mut action_buttons = row![].spacing(theme::PANEL_BUTTON_GAP);
            action_buttons = action_buttons.push(
                button(
                    text(t!("instruction_apply_rewrite").to_string())
                        .font(theme::INTER)
                        .size(theme::INSTRUCTION_BUTTON_SIZE),
                )
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(
                    block_id,
                    InstructionPanelMessage::ApplyInstructionRewrite,
                )),
            );
            action_buttons = action_buttons.push(
                button(
                    text(t!("instruction_append_block").to_string())
                        .font(theme::INTER)
                        .size(theme::INSTRUCTION_BUTTON_SIZE),
                )
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(
                    block_id,
                    InstructionPanelMessage::AppendInstructionResponse,
                )),
            );
            action_buttons = action_buttons.push(
                button(
                    text(t!("instruction_add_child").to_string())
                        .font(theme::INTER)
                        .size(theme::INSTRUCTION_BUTTON_SIZE),
                )
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(
                    block_id,
                    InstructionPanelMessage::AddInstructionResponseAsChild,
                )),
            );
            action_buttons = action_buttons.push(
                button(
                    text(t!("ui_discard").to_string())
                        .font(theme::INTER)
                        .size(theme::INSTRUCTION_BUTTON_SIZE),
                )
                .style(theme::destructive_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(block_id, InstructionPanelMessage::Dismiss)),
            );

            result_section = result_section.push(action_buttons);
            panel = panel.push(result_section);
        }
    }

    container(panel)
        .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> (AppState, BlockId) {
        AppState::test_state()
    }

    #[test]
    fn instruction_toggle_opens_panel_without_clearing_input() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.editor_buffers.set_instruction_text("keep this instruction");
        state.store.set_instruction_draft(root, "keep this instruction".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Toggle),
        );

        assert_eq!(state.store.panel_state(&root).copied(), Some(PanelBarState::Instruction));
        assert_eq!(state.editor_buffers.instruction_content().text(), "keep this instruction");
    }

    #[test]
    fn instruction_toggle_opens_panel_with_persisted_drafts() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.store.set_instruction_draft(root, "persisted instruction".to_string());
        state.store.set_inquiry_draft(root, "persisted inquiry".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Toggle),
        );

        assert_eq!(state.editor_buffers.instruction_content().text(), "persisted instruction");
        assert_eq!(
            state.store.inquiry_draft(&root).map(|r| r.response.as_str()),
            Some("persisted inquiry")
        );
    }

    #[test]
    fn instruction_toggle_closes_panel_and_preserves_draft_state() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.store.set_panel_state(&root, Some(PanelBarState::Instruction));
        state.store.set_instruction_draft(root, "prompt".to_string());
        state.store.set_inquiry_draft(root, "result".to_string());
        state.llm_requests.mark_inquiry_loading(root);
        state.editor_buffers.set_instruction_text("keep me");

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Toggle),
        );

        assert_eq!(state.store.panel_state(&root).copied(), None);
        assert_eq!(
            state.store.instruction_draft(&root).map(|d| d.instruction.as_str()),
            Some("prompt")
        );
        assert_eq!(state.store.inquiry_draft(&root).map(|r| r.response.as_str()), Some("result"));
        assert!(state.llm_requests.is_inquiring(root));
        assert_eq!(state.editor_buffers.instruction_content().text(), "keep me");
    }

    #[test]
    fn inquire_append_applies_to_bound_target_not_current_focus() {
        let (mut state, root) = test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling text".to_string())
            .expect("append sibling succeeds");
        state.store.update_point(&root, "root text".to_string());
        state.editor_buffers.set_text(&root, "root text");
        state.set_focus(sibling);
        state.store.set_inquiry_draft(sibling, "inquiry response".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(sibling, InstructionPanelMessage::AppendInstructionResponse),
        );

        assert_eq!(
            state.store.point(&sibling).as_deref(),
            Some("sibling text\n\ninquiry response")
        );
        assert_eq!(state.store.point(&root).as_deref(), Some("root text"));
        assert!(state.store.inquiry_draft(&root).is_none());
    }

    #[test]
    fn inquire_add_child_applies_to_bound_target_not_current_focus() {
        let (mut state, root) = test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling text".to_string())
            .expect("append sibling succeeds");
        let before_len = state.store.children(&sibling).len();
        state.set_focus(sibling);
        state.store.set_inquiry_draft(sibling, "child from inquiry".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                sibling,
                InstructionPanelMessage::AddInstructionResponseAsChild,
            ),
        );

        let children = state.store.children(&sibling);
        assert_eq!(children.len(), before_len + 1);
        let child_id = *children.last().expect("new child added under sibling");
        assert_eq!(state.store.point(&child_id).as_deref(), Some("child from inquiry"));
        assert!(state.store.inquiry_draft(&sibling).is_none());
    }

    #[test]
    fn inquire_submission_consumes_instruction_editor_text() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.editor_buffers.set_instruction_text("ask this");
        state.store.set_instruction_draft(root, "ask this".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Inquire),
        );

        assert!(state.llm_requests.is_inquiring(root));
        assert!(state.editor_buffers.instruction_content().text().is_empty());
        assert!(state.store.instruction_draft(&root).is_none());
    }

    #[test]
    fn inquire_done_persists_inquiry_draft() {
        let (mut state, root) = test_state();
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireDone {
                    block_id: root,
                    result: Ok("persisted response".to_string()),
                },
            ),
        );

        assert_eq!(
            state.store.inquiry_draft(&root).map(|draft| draft.response.as_str()),
            Some("persisted response")
        );
    }

    #[test]
    fn expand_with_instruction_consumes_persisted_instruction_draft() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.store.set_instruction_draft(root, "expand this".to_string());
        state.editor_buffers.set_instruction_text("expand this");

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::ExpandWithInstruction),
        );

        assert!(state.store.instruction_draft(&root).is_none());
    }

    #[test]
    fn dismiss_clears_persisted_inquiry_draft() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.store.set_inquiry_draft(root, "draft".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Dismiss),
        );

        assert!(state.store.inquiry_draft(&root).is_none());
    }
}
