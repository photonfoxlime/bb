//! Instruction panel for LLM interactions.
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
use std::time::Duration;

const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const INSTRUCTION_EDITOR_HEIGHT: f32 = 80.0;

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
            state.store.remove_inquiry_draft(&target_block_id);
            state.store.remove_instruction_draft(&target_block_id);
            state
                .persist_with_context("after consuming instruction and inquiry drafts for inquire");
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
            // Persist the instruction draft - expand will consume it
            state.store.set_instruction_draft(target_block_id, instruction.clone());
            state.persist_with_context("after persisting instruction draft for expand");
            state.editor_buffers.set_instruction_text("");
            // Close the instruction panel and trigger expand
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
            // Persist the instruction draft - reduce will consume it
            state.store.set_instruction_draft(target_block_id, instruction.clone());
            state.persist_with_context("after persisting instruction draft for reduce");
            state.editor_buffers.set_instruction_text("");
            // Close the instruction panel and trigger reduce
            state.store.set_panel_state(&target_block_id, None);
            state.persist_with_context("after closing instruction panel");
            crate::app::AppState::update(
                state,
                Message::Reduce(ReduceMessage::Start(target_block_id)),
            )
        }
        | InstructionPanelMessage::ApplyInstructionRewrite => {
            // Get inquiry draft: first check focused block, then search for any block with draft
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
            // Get inquiry draft: first check focused block, then search for any block with draft
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
            // Get inquiry draft: first check focused block, then search for any block with draft
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
            // Dismiss clears the inquiry draft from store
            state.store.remove_inquiry_draft(&target_block_id);
            state.persist_with_context("after dismissing inquiry draft");
            iced::Task::none()
        }
    }
}

fn sync_instruction_panel_from_store(state: &mut AppState, target_block_id: &BlockId) {
    // Load instruction draft from store into editor buffer
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
    use iced::widget::{column, row};

    // Get the focused block and check if instruction panel is open
    let block_id = match state.focused_block_id {
        | Some(id) if matches!(state.store.panel_state(&id), Some(PanelBarState::Instruction)) => {
            id
        }
        | _ => return container(iced::widget::Text::new("")).into(),
    };

    let instruction_content = state.editor_buffers.instruction_content();
    // Derive inquiry state from store and llm_requests
    let inquiry_result = state.store.inquiry_draft(&block_id).map(|r| r.response.as_str());
    let is_inquiring = state.llm_requests.is_inquiring(block_id);

    let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

    // Instruction text editor
    panel = panel.push(
        container(
            text_editor(instruction_content)
                .placeholder(t!("instruction_placeholder").to_string())
                .style(theme::point_editor)
                .height(INSTRUCTION_EDITOR_HEIGHT)
                .on_action(move |action| {
                    Message::InstructionPanel(block_id, InstructionPanelMessage::TextEdited(action))
                        .into()
                }),
        )
        .width(iced::Length::Fill)
        .height(iced::Length::Fixed(INSTRUCTION_EDITOR_HEIGHT)),
    );

    // Action buttons row
    let mut button_row = row![].spacing(theme::PANEL_BUTTON_GAP);

    if is_inquiring {
        // Cancel button when inquiry is in progress
        button_row = button_row.push(
            button(text(t!("instruction_cancel").to_string()).font(theme::INTER).size(13))
                .style(theme::destructive_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(block_id, InstructionPanelMessage::CancelInquire)
                        .into(),
                ),
        );
    } else {
        // Inquire button
        let inquire_btn =
            button(text(t!("instruction_inquire").to_string()).font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(block_id, InstructionPanelMessage::Inquire).into(),
                );

        button_row = button_row.push(inquire_btn);

        // Expand button
        button_row = button_row.push(
            button(text(t!("instruction_expand").to_string()).font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(
                        block_id,
                        InstructionPanelMessage::ExpandWithInstruction,
                    )
                    .into(),
                ),
        );

        // Reduce button
        button_row = button_row.push(
            button(text(t!("instruction_reduce").to_string()).font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(
                        block_id,
                        InstructionPanelMessage::ReduceWithInstruction,
                    )
                    .into(),
                ),
        );
    }

    panel = panel.push(button_row);

    // Show inquiry result if available
    if let Some(result) = inquiry_result {
        let mut result_col = column![].spacing(theme::PANEL_INNER_GAP);
        result_col = result_col.push(container(
            text(t!("instruction_response").to_string()).width(iced::Length::Fill),
        ));
        result_col = result_col.push(container(text(result)).width(iced::Length::Fill));

        // Action buttons for the result
        let mut result_buttons = row![].spacing(theme::PANEL_BUTTON_GAP);
        result_buttons = result_buttons.push(
            button(text(t!("instruction_apply_rewrite").to_string()).font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(
                        block_id,
                        InstructionPanelMessage::ApplyInstructionRewrite,
                    )
                    .into(),
                ),
        );
        result_buttons = result_buttons.push(
            button(text(t!("instruction_append_block").to_string()).font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(
                        block_id,
                        InstructionPanelMessage::AppendInstructionResponse,
                    )
                    .into(),
                ),
        );
        result_buttons = result_buttons.push(
            button(text(t!("instruction_add_child").to_string()).font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(
                        block_id,
                        InstructionPanelMessage::AddInstructionResponseAsChild,
                    )
                    .into(),
                ),
        );
        result_buttons = result_buttons.push(
            button(text(t!("ui_discard").to_string()).font(theme::INTER).size(13))
                .style(theme::destructive_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(block_id, InstructionPanelMessage::Dismiss).into(),
                ),
        );
        result_col = result_col.push(result_buttons);

        panel = panel.push(result_col);
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
        state.focused_block_id = Some(root);
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
        state.focused_block_id = Some(root);
        state.store.set_instruction_draft(root, "persisted instruction".to_string());
        state.store.set_inquiry_draft(root, "persisted inquiry".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Toggle),
        );

        assert_eq!(state.editor_buffers.instruction_content().text(), "persisted instruction");
        // Inquiry result is now in the store
        assert_eq!(
            state.store.inquiry_draft(&root).map(|r| r.response.as_str()),
            Some("persisted inquiry")
        );
    }

    #[test]
    fn instruction_toggle_closes_panel_and_preserves_draft_state() {
        let (mut state, root) = test_state();
        state.focused_block_id = Some(root);
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
        // Persisted drafts should be preserved in store
        assert_eq!(
            state.store.instruction_draft(&root).map(|d| d.instruction.as_str()),
            Some("prompt")
        );
        assert_eq!(state.store.inquiry_draft(&root).map(|r| r.response.as_str()), Some("result"));
        // Loading state should also be preserved
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
        state.focused_block_id = Some(sibling);
        // Put inquiry draft in store instead of instruction_panel
        state.store.set_inquiry_draft(sibling, "inquiry response".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(sibling, InstructionPanelMessage::AppendInstructionResponse),
        );

        assert_eq!(state.store.point(&sibling).as_deref(), Some("sibling text\n\ninquiry response"));
        assert_eq!(state.store.point(&root).as_deref(), Some("root text"));
        // Inquiry draft should be cleared from store
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
        state.focused_block_id = Some(sibling);
        // Put inquiry draft in store instead of instruction_panel
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
        // Inquiry draft should be cleared from store
        assert!(state.store.inquiry_draft(&sibling).is_none());
    }

    #[test]
    fn inquire_submission_consumes_instruction_editor_text() {
        let (mut state, root) = test_state();
        state.focused_block_id = Some(root);
        state.editor_buffers.set_instruction_text("ask this");
        state.store.set_instruction_draft(root, "ask this".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Inquire),
        );

        // Inquiry loading state is tracked in llm_requests
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
        state.focused_block_id = Some(root);
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
        state.focused_block_id = Some(root);
        state.store.set_inquiry_draft(root, "draft".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Dismiss),
        );

        assert!(state.store.inquiry_draft(&root).is_none());
    }
}
