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

use iced::Element;
use iced::widget::{button, container, text, text_editor};
use std::time::Duration;

const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const INSTRUCTION_EDITOR_HEIGHT: f32 = 80.0;

/// Instruction panel state.
#[derive(Debug, Clone, Default)]
pub struct InstructionPanel {
    /// Result draft from the last inquiry request.
    ///
    /// This value is owned by the instruction panel workflow and can later be
    /// transformed into a concrete tree mutation (for example, rewrite/append/new-child).
    pub inquiry_result: Option<String>,
    /// Block id that the current inquiry draft is bound to.
    pub inquiry_target_block_id: Option<BlockId>,
    /// Whether an inquiry request is currently in progress.
    pub is_inquiring: bool,
    /// Instruction draft submitted into expand/reduce as additional guidance.
    pub prompt: Option<String>,
}

impl InstructionPanel {
    /// Create a new default instruction panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the panel state, clearing all fields.
    pub fn reset(&mut self) {
        self.inquiry_result = None;
        self.inquiry_target_block_id = None;
        self.is_inquiring = false;
        self.prompt = None;
    }
}

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
    state: &mut AppState, block_id: BlockId, msg: InstructionPanelMessage,
) -> iced::Task<Message> {
    use crate::app::{ExpandMessage, ReduceMessage};

    // Get the actual target block from focused_block_id
    let target_block_id = state.focused_block_id.unwrap_or(block_id);

    match msg {
        | InstructionPanelMessage::Toggle => {
            if matches!(&state.panel_bar_state, Some(PanelBarState::Instruction)) {
                state.panel_bar_state = None;
            } else {
                state.panel_bar_state = Some(PanelBarState::Instruction);
                sync_instruction_panel_from_store(state, &target_block_id);
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::TextEdited(action) => {
            state.editor_buffers.instruction_content_mut().perform(action);
            if state.store.node(&target_block_id).is_some() {
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
            state.instruction_panel.is_inquiring = true;
            state.instruction_panel.inquiry_result = None;
            state.instruction_panel.inquiry_target_block_id = None;
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
                    Message::InstructionPanel(InstructionPanelMessage::InquireDone {
                        block_id: target_block_id,
                        result,
                    })
                },
            );
            request_task
        }
        | InstructionPanelMessage::InquireDone { block_id, result } => {
            state.instruction_panel.is_inquiring = false;
            match result {
                | Ok(response) => {
                    tracing::info!(
                        block_id = ?block_id,
                        chars = response.len(),
                        "instruction inquiry succeeded"
                    );
                    state.instruction_panel.inquiry_result = Some(response.clone());
                    state.instruction_panel.inquiry_target_block_id = Some(block_id);
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
        | InstructionPanelMessage::ExpandWithInstruction => {
            let instruction = state.editor_buffers.instruction_content().text().trim().to_string();
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.instruction_panel.prompt =
                Some(format!("Additional instruction: {}", instruction));
            state.store.remove_instruction_draft(&target_block_id);
            state.persist_with_context("after consuming instruction draft for expand");
            state.editor_buffers.set_instruction_text("");
            // Close the instruction panel and trigger expand
            state.panel_bar_state = None;
            crate::app::update(state, Message::Expand(ExpandMessage::Start(target_block_id)))
        }
        | InstructionPanelMessage::ReduceWithInstruction => {
            let instruction = state.editor_buffers.instruction_content().text().trim().to_string();
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.instruction_panel.prompt =
                Some(format!("Additional instruction: {}", instruction));
            state.store.remove_instruction_draft(&target_block_id);
            state.persist_with_context("after consuming instruction draft for reduce");
            state.editor_buffers.set_instruction_text("");
            // Close the instruction panel and trigger reduce
            state.panel_bar_state = None;
            crate::app::update(state, Message::Reduce(ReduceMessage::Start(target_block_id)))
        }
        | InstructionPanelMessage::ApplyInstructionRewrite => {
            let mut cleared_target = None;
            if let (Some(rewrite), Some(target_block_id)) = (
                state.instruction_panel.inquiry_result.take(),
                state.instruction_panel.inquiry_target_block_id,
            ) {
                cleared_target = Some(target_block_id);
                state.mutate_with_undo_and_persist("after applying instruction rewrite", |state| {
                    state.store.update_point(&target_block_id, rewrite.clone());
                    state.editor_buffers.set_text(&target_block_id, &rewrite);
                    true
                });
            }
            state.instruction_panel.inquiry_result = None;
            state.instruction_panel.inquiry_target_block_id = None;
            if let Some(target) = cleared_target {
                state.store.remove_inquiry_draft(&target);
                state.persist_with_context("after clearing inquiry draft by rewrite apply");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::AppendInstructionResponse => {
            let mut cleared_target = None;
            if let (Some(response), Some(target_block_id)) = (
                state.instruction_panel.inquiry_result.take(),
                state.instruction_panel.inquiry_target_block_id,
            ) {
                cleared_target = Some(target_block_id);
                state.mutate_with_undo_and_persist(
                    "after appending instruction inquiry response",
                    |state| {
                        let current = state.store.point(&target_block_id).unwrap_or_default();
                        let next = if current.trim().is_empty() {
                            response.clone()
                        } else {
                            format!("{current}\n\n{response}")
                        };
                        state.store.update_point(&target_block_id, next.clone());
                        state.editor_buffers.set_text(&target_block_id, &next);
                        true
                    },
                );
            }
            state.instruction_panel.inquiry_result = None;
            state.instruction_panel.inquiry_target_block_id = None;
            if let Some(target) = cleared_target {
                state.store.remove_inquiry_draft(&target);
                state.persist_with_context("after clearing inquiry draft by append apply");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::AddInstructionResponseAsChild => {
            let mut cleared_target = None;
            if let (Some(response), Some(target_block_id)) = (
                state.instruction_panel.inquiry_result.take(),
                state.instruction_panel.inquiry_target_block_id,
            ) {
                cleared_target = Some(target_block_id);
                state.mutate_with_undo_and_persist(
                    "after adding instruction inquiry response as child",
                    |state| {
                        if let Some(child_id) =
                            state.store.append_child(&target_block_id, response.clone())
                        {
                            state.editor_buffers.set_text(&child_id, &response);
                            return true;
                        }
                        false
                    },
                );
            }
            state.instruction_panel.inquiry_result = None;
            state.instruction_panel.inquiry_target_block_id = None;
            if let Some(target) = cleared_target {
                state.store.remove_inquiry_draft(&target);
                state.persist_with_context("after clearing inquiry draft by add-child apply");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::Dismiss => {
            let dismiss_target =
                state.instruction_panel.inquiry_target_block_id.or(Some(target_block_id));
            state.instruction_panel.inquiry_result = None;
            state.instruction_panel.inquiry_target_block_id = None;
            if let Some(target) = dismiss_target {
                state.store.remove_inquiry_draft(&target);
                state.persist_with_context("after dismissing inquiry draft");
            }
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

    if let Some(inquiry) = state.store.inquiry_draft(target_block_id) {
        state.instruction_panel.inquiry_result = Some(inquiry.response.clone());
        state.instruction_panel.inquiry_target_block_id = Some(*target_block_id);
    } else {
        state.instruction_panel.inquiry_result = None;
        state.instruction_panel.inquiry_target_block_id = None;
    }
}

/// Render the instruction panel for the focused block.
pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    use crate::app::PanelBarState;
    use iced::Padding;
    use iced::widget::{column, row};

    // Get the focused block from panel_bar_state
    let _block_id = match (state.focused_block_id, &state.panel_bar_state) {
        | (Some(id), Some(PanelBarState::Instruction)) => id,
        | _ => return container(iced::widget::Text::new("")).into(),
    };

    let instruction_content = state.editor_buffers.instruction_content();
    let inquiry_result = &state.instruction_panel.inquiry_result;
    let is_inquiring = state.instruction_panel.is_inquiring;

    let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

    // Instruction text editor
    panel = panel.push(
        container(
            text_editor(instruction_content)
                .placeholder("Enter instruction...")
                .style(theme::point_editor)
                .height(INSTRUCTION_EDITOR_HEIGHT)
                .on_action(move |action| {
                    Message::InstructionPanel(InstructionPanelMessage::TextEdited(action)).into()
                }),
        )
        .width(iced::Length::Fill)
        .height(iced::Length::Fixed(INSTRUCTION_EDITOR_HEIGHT)),
    );

    // Action buttons row
    let mut button_row = row![].spacing(theme::PANEL_BUTTON_GAP);

    // Inquire button
    let inquire_btn = button(
        text(if is_inquiring { "Inquiring..." } else { "Inquire" }).font(theme::INTER).size(13),
    )
    .style(theme::action_button)
    .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
    .on_press(Message::InstructionPanel(InstructionPanelMessage::Inquire).into());

    button_row = button_row.push(inquire_btn);

    // Expand button
    button_row = button_row.push(
        button(text("Expand").font(theme::INTER).size(13))
            .style(theme::action_button)
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(
                Message::InstructionPanel(InstructionPanelMessage::ExpandWithInstruction).into(),
            ),
    );

    // Reduce button
    button_row = button_row.push(
        button(text("Reduce").font(theme::INTER).size(13))
            .style(theme::action_button)
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(
                Message::InstructionPanel(InstructionPanelMessage::ReduceWithInstruction).into(),
            ),
    );

    panel = panel.push(button_row);

    // Show inquiry result if available
    if let Some(result) = inquiry_result {
        let mut result_col = column![].spacing(theme::PANEL_INNER_GAP);
        result_col = result_col.push(container(text("Response")).width(iced::Length::Fill));
        result_col = result_col.push(container(text(result.as_str())).width(iced::Length::Fill));

        // Action buttons for the result
        let mut result_buttons = row![].spacing(theme::PANEL_BUTTON_GAP);
        result_buttons = result_buttons.push(
            button(text("Apply as Rewrite").font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(InstructionPanelMessage::ApplyInstructionRewrite)
                        .into(),
                ),
        );
        result_buttons = result_buttons.push(
            button(text("Append to Block").font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(InstructionPanelMessage::AppendInstructionResponse)
                        .into(),
                ),
        );
        result_buttons = result_buttons.push(
            button(text("Add as Child").font(theme::INTER).size(13))
                .style(theme::action_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(
                    Message::InstructionPanel(
                        InstructionPanelMessage::AddInstructionResponseAsChild,
                    )
                    .into(),
                ),
        );
        result_buttons = result_buttons.push(
            button(text("Dismiss").font(theme::INTER).size(13))
                .style(theme::destructive_button)
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(InstructionPanelMessage::Dismiss).into()),
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
    use super::*;

    #[test]
    fn instruction_panel_default_is_blank() {
        let panel = InstructionPanel::new();
        assert!(panel.inquiry_result.is_none());
        assert!(panel.inquiry_target_block_id.is_none());
        assert!(!panel.is_inquiring);
        assert!(panel.prompt.is_none());
    }

    #[test]
    fn instruction_panel_reset_clears_all_fields() {
        let mut panel = InstructionPanel::new();
        panel.inquiry_result = Some("test result".to_string());
        panel.inquiry_target_block_id = Some(BlockId::default());
        panel.is_inquiring = true;
        panel.prompt = Some("test prompt".to_string());

        panel.reset();

        assert!(panel.inquiry_result.is_none());
        assert!(panel.inquiry_target_block_id.is_none());
        assert!(!panel.is_inquiring);
        assert!(panel.prompt.is_none());
    }

    #[test]
    fn instruction_panel_can_store_inquiry_result() {
        let mut panel = InstructionPanel::new();
        panel.inquiry_result = Some("result".to_string());
        assert_eq!(panel.inquiry_result.as_deref(), Some("result"));
    }

    #[test]
    fn instruction_panel_tracks_inquiry_loading_state() {
        let mut panel = InstructionPanel::new();
        panel.is_inquiring = true;
        assert!(panel.is_inquiring);
    }

    #[test]
    fn instruction_panel_can_store_prompt() {
        let mut panel = InstructionPanel::new();
        panel.prompt = Some("prompt".to_string());
        assert_eq!(panel.prompt.as_deref(), Some("prompt"));
    }

    #[test]
    fn instruction_panel_clone_works() {
        let mut panel = InstructionPanel::new();
        panel.inquiry_result = Some("test".to_string());
        panel.inquiry_target_block_id = Some(BlockId::default());
        panel.is_inquiring = true;
        panel.prompt = Some("prompt".to_string());

        let cloned = panel.clone();

        assert_eq!(cloned.inquiry_result, Some("test".to_string()));
        assert_eq!(cloned.inquiry_target_block_id, Some(BlockId::default()));
        assert!(cloned.is_inquiring);
        assert_eq!(cloned.prompt, Some("prompt".to_string()));
    }
}
