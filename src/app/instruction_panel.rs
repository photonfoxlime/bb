//! Instruction panel for LLM interactions.
//!
//! Provides a text editor for entering instructions and three actions:
//! - Inquire: Send instruction as a one-time query, result can be applied as rewrite
//! - Expand: Inject instruction as system prompt for expand operations
//! - Reduce: Inject instruction as system prompt for reduce operations

use crate::app::{AppState, Message, PanelBarState};
use crate::llm;
use crate::store::BlockId;
use crate::theme;

use iced::Element;
use iced::widget::{button, container, text, text_editor};
use std::time::Duration;

const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Instruction panel state.
#[derive(Debug, Clone, Default)]
pub struct InstructionPanel {
    /// Result from the last inquiry request (rewrite suggestion).
    pub inquiry_result: Option<String>,
    /// Whether an inquiry request is currently in progress.
    pub is_inquiring: bool,
    /// System prompt instruction to prepend to expand/reduce requests.
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
                state.instruction_panel.reset();
                state.editor_buffers.set_instruction_text("");
            } else {
                state.panel_bar_state = Some(PanelBarState::Instruction);
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::TextEdited(action) => {
            state.editor_buffers.instruction_content_mut().perform(action);
            iced::Task::none()
        }
        | InstructionPanelMessage::Inquire => {
            let instruction = state.editor_buffers.instruction_content().text().to_string();
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.instruction_panel.is_inquiring = true;
            let context = state.store.block_context_for_id(&target_block_id);
            let Some(config) = state.llm_config.clone().ok() else {
                return iced::Task::none();
            };
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
                    state.instruction_panel.inquiry_result = Some(response);
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
            // Close the instruction panel and trigger reduce
            state.panel_bar_state = None;
            crate::app::update(state, Message::Reduce(ReduceMessage::Start(target_block_id)))
        }
        | InstructionPanelMessage::ApplyInstructionRewrite => {
            if let Some(rewrite) = state.instruction_panel.inquiry_result.take() {
                state.mutate_with_undo_and_persist("after applying instruction rewrite", |state| {
                    state.store.update_point(&target_block_id, rewrite.clone());
                    state.editor_buffers.set_text(&target_block_id, &rewrite);
                    true
                });
            }
            state.instruction_panel.inquiry_result = None;
            iced::Task::none()
        }
        | InstructionPanelMessage::Dismiss => {
            state.instruction_panel.inquiry_result = None;
            iced::Task::none()
        }
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
                .on_action(move |action| {
                    Message::InstructionPanel(InstructionPanelMessage::TextEdited(action)).into()
                }),
        )
        .height(iced::Length::Fixed(80.0)),
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
        assert!(!panel.is_inquiring);
        assert!(panel.prompt.is_none());
    }

    #[test]
    fn instruction_panel_reset_clears_all_fields() {
        let mut panel = InstructionPanel::new();
        panel.inquiry_result = Some("test result".to_string());
        panel.is_inquiring = true;
        panel.prompt = Some("test prompt".to_string());

        panel.reset();

        assert!(panel.inquiry_result.is_none());
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
        panel.is_inquiring = true;
        panel.prompt = Some("prompt".to_string());

        let cloned = panel.clone();

        assert_eq!(cloned.inquiry_result, Some("test".to_string()));
        assert!(cloned.is_inquiring);
        assert_eq!(cloned.prompt, Some("prompt".to_string()));
    }
}
