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

use crate::app::{AppState, BlockPanelBarState, Message, RequestSignature};
use crate::component::text_button::TextButton;
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
    /// One inquiry response chunk arrived.
    InquireChunk { block_id: BlockId, request_signature: RequestSignature, chunk: String },
    /// Inquiry stream reported an error.
    InquireFailed {
        block_id: BlockId,
        request_signature: RequestSignature,
        reason: crate::app::UiError,
    },
    /// Inquiry request completed (successfully or with error).
    InquireFinished { block_id: BlockId, request_signature: RequestSignature },
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
    use super::patch::{PatchKind, PatchMessage};

    match msg {
        | InstructionPanelMessage::Toggle => {
            let current_state = state.store.block_panel_state(&target_block_id).copied();
            if matches!(current_state, Some(BlockPanelBarState::Instruction)) {
                state.store.set_block_panel_state(&target_block_id, None);
            } else {
                state
                    .store
                    .set_block_panel_state(&target_block_id, Some(BlockPanelBarState::Instruction));
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
            let context = state.store.block_context_for_id(&target_block_id);
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return iced::Task::none();
            };
            let Some(config) = state.llm_config_for_inquire() else {
                return iced::Task::none();
            };
            state.llm_requests.mark_inquiry_loading(target_block_id, request_signature);
            state.store.set_inquiry(target_block_id, instruction.clone());
            state.store.remove_instruction_draft(&target_block_id);
            state.persist_with_context("after storing inquiry and consuming instruction draft");
            state.editor_buffers.set_instruction_text("");
            tracing::info!(block_id = ?target_block_id, "instruction inquiry started");
            let inquire_max_tokens = state.config.tasks.inquire.token_limit.as_api_param();
            let prompt_config = llm::TaskPromptConfig::inquire(
                &state.config.tasks.inquire.system_prompt,
                &state.config.tasks.inquire.user_prompt,
            );
            let client = llm::LlmClient::new(config);
            let request_task = iced::Task::run(
                client.inquire_stream(
                    context,
                    instruction,
                    LLM_REQUEST_TIMEOUT,
                    inquire_max_tokens,
                    prompt_config,
                ),
                move |event| match event {
                    | llm::InquireStreamEvent::Chunk(chunk) => Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::InquireChunk {
                            block_id: target_block_id,
                            request_signature,
                            chunk,
                        },
                    ),
                    | llm::InquireStreamEvent::Failed(err) => Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::InquireFailed {
                            block_id: target_block_id,
                            request_signature,
                            reason: crate::app::UiError::from_message(err),
                        },
                    ),
                    | llm::InquireStreamEvent::Finished => Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::InquireFinished {
                            block_id: target_block_id,
                            request_signature,
                        },
                    ),
                },
            );
            let (request_task, handle) = iced::Task::abortable(request_task);
            state.llm_requests.replace_inquiry_handle(target_block_id, handle);
            request_task
        }
        | InstructionPanelMessage::InquireChunk { block_id, request_signature, chunk } => {
            if state.store.node(&block_id).is_none() {
                return iced::Task::none();
            }
            if state.is_stale_response(&block_id, request_signature) {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale instruction inquiry chunk after context changed"
                );
                return iced::Task::none();
            }
            state.store.append_inquiry_response_chunk(block_id, &chunk);
            iced::Task::none()
        }
        | InstructionPanelMessage::InquireFailed { block_id, request_signature, reason } => {
            if state.store.node(&block_id).is_none() {
                return iced::Task::none();
            }
            if state.is_stale_response(&block_id, request_signature) {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale instruction inquiry error after context changed"
                );
                return iced::Task::none();
            }
            tracing::error!(
                block_id = ?block_id,
                reason = %reason.as_str(),
                "instruction inquiry stream failed"
            );
            state.llm_requests.set_inquiry_error(block_id, reason);
            iced::Task::none()
        }
        | InstructionPanelMessage::InquireFinished { block_id, request_signature } => {
            let (pending_signature, pending_error) =
                state.llm_requests.finish_inquiry_request(block_id);
            if state.store.node(&block_id).is_none() {
                return iced::Task::none();
            }
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale instruction inquiry response after context changed"
                );
                return iced::Task::none();
            }
            let response_len = state
                .store
                .inquiry_draft(&block_id)
                .map(|record| record.response.trim())
                .filter(|response| !response.is_empty())
                .map(str::len)
                .unwrap_or(0);
            let had_stream_error = pending_error.is_some();

            if let Some(reason) = pending_error {
                state.record_error(crate::app::AppError::Inquire(reason));
            }

            if response_len > 0 {
                tracing::info!(block_id = ?block_id, chars = response_len, "instruction inquiry completed");
                if !had_stream_error {
                    state.errors.retain(|err| !matches!(err, crate::app::AppError::Inquire(_)));
                }
                state.persist_with_context("after persisting streamed instruction inquiry draft");
            } else if !had_stream_error {
                state.record_error(crate::app::AppError::Inquire(
                    crate::app::UiError::from_message("inquire returned no usable text"),
                ));
                tracing::error!(
                    block_id = ?block_id,
                    "instruction inquiry finished without usable response"
                );
            } else {
                tracing::error!(
                    block_id = ?block_id,
                    "instruction inquiry finished after stream error"
                );
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
            state.store.set_block_panel_state(&target_block_id, None);
            state.persist_with_context("after closing instruction panel");
            crate::app::AppState::update(
                state,
                Message::Patch(PatchMessage::Start {
                    kind: PatchKind::Expand,
                    block_id: target_block_id,
                }),
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
            state.store.set_block_panel_state(&target_block_id, None);
            state.persist_with_context("after closing instruction panel");
            crate::app::AppState::update(
                state,
                Message::Patch(PatchMessage::Start {
                    kind: PatchKind::Reduce,
                    block_id: target_block_id,
                }),
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
    use crate::store::BlockPanelBarState;
    use iced::Padding;
    use iced::widget::{column, row, scrollable};

    let block_id = match state.focus().map(|s| s.block_id) {
        | Some(id)
            if matches!(
                state.store.block_panel_state(&id),
                Some(BlockPanelBarState::Instruction)
            ) =>
        {
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
            scrollable(text(inquiry_text).font(theme::LXGW_WENKAI).size(theme::INPUT_TEXT_SIZE))
                .width(iced::Length::Fill),
        )
        .padding(Padding::from([theme::COMPACT_PAD_V, theme::PANEL_PAD_V]))
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
                    .font(theme::DEFAULT_FONT)
                    .size(theme::INPUT_TEXT_SIZE)
                    .line_height(iced::widget::text::LineHeight::Absolute(
                        (theme::INPUT_TEXT_SIZE * theme::EDITOR_LINE_HEIGHT).into(),
                    ))
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
            TextButton::action(
                t!("instruction_expand").to_string(),
                theme::INSTRUCTION_BUTTON_SIZE,
            )
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(
                block_id,
                InstructionPanelMessage::ExpandWithInstruction,
            )),
        );

        button_row = button_row.push(
            TextButton::action(
                t!("instruction_reduce").to_string(),
                theme::INSTRUCTION_BUTTON_SIZE,
            )
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(
                block_id,
                InstructionPanelMessage::ReduceWithInstruction,
            )),
        );

        button_row = button_row.push(
            TextButton::action(
                t!("instruction_inquire").to_string(),
                theme::INSTRUCTION_BUTTON_SIZE,
            )
            .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(block_id, InstructionPanelMessage::Inquire)),
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

    if let Some(result) = inquiry_result {
        let response_text = result.response.as_str();
        if !response_text.is_empty() {
            use crate::component::patch_panel::{PanelButton, PanelButtonStyle, RewriteSection};
            let content: iced::Element<'_, Message> = container(
                scrollable(
                    text(response_text).font(theme::LXGW_WENKAI).size(theme::INPUT_TEXT_SIZE),
                )
                .width(iced::Length::Fill),
            )
            .width(iced::Length::Fill)
            .into();
            let buttons = if !is_inquiring {
                vec![
                    PanelButton {
                        label: t!("instruction_apply_rewrite").to_string(),
                        style: PanelButtonStyle::Action,
                        on_press: Message::InstructionPanel(
                            block_id,
                            InstructionPanelMessage::ApplyInstructionRewrite,
                        ),
                    },
                    PanelButton {
                        label: t!("instruction_append_block").to_string(),
                        style: PanelButtonStyle::Action,
                        on_press: Message::InstructionPanel(
                            block_id,
                            InstructionPanelMessage::AppendInstructionResponse,
                        ),
                    },
                    PanelButton {
                        label: t!("instruction_add_child").to_string(),
                        style: PanelButtonStyle::Action,
                        on_press: Message::InstructionPanel(
                            block_id,
                            InstructionPanelMessage::AddInstructionResponseAsChild,
                        ),
                    },
                    PanelButton {
                        label: t!("ui_discard").to_string(),
                        style: PanelButtonStyle::Destructive,
                        on_press: Message::InstructionPanel(
                            block_id,
                            InstructionPanelMessage::Dismiss,
                        ),
                    },
                ]
            } else {
                vec![]
            };
            let result_panel = crate::component::patch_panel::view(
                state.is_dark_mode(),
                Some(RewriteSection::Content {
                    title: t!("instruction_response").to_string(),
                    content,
                    buttons,
                }),
                None,
            );
            panel = panel.push(result_panel);
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

        assert_eq!(
            state.store.block_panel_state(&root).copied(),
            Some(BlockPanelBarState::Instruction)
        );
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
        state.store.set_block_panel_state(&root, Some(BlockPanelBarState::Instruction));
        state.store.set_instruction_draft(root, "prompt".to_string());
        state.store.set_inquiry_draft(root, "result".to_string());
        let signature = state.block_context_signature(&root).expect("root has request signature");
        state.llm_requests.mark_inquiry_loading(root, signature);
        state.editor_buffers.set_instruction_text("keep me");

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Toggle),
        );

        assert_eq!(state.store.block_panel_state(&root).copied(), None);
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
    fn inquire_finished_persists_streamed_inquiry_draft() {
        let (mut state, root) = test_state();
        let request_signature =
            state.block_context_signature(&root).expect("root has request signature");
        state.llm_requests.mark_inquiry_loading(root, request_signature);
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireChunk {
                    block_id: root,
                    request_signature,
                    chunk: "persisted ".to_string(),
                },
            ),
        );
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireChunk {
                    block_id: root,
                    request_signature,
                    chunk: "response".to_string(),
                },
            ),
        );
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireFinished { block_id: root, request_signature },
            ),
        );

        assert_eq!(
            state.store.inquiry_draft(&root).map(|draft| draft.response.as_str()),
            Some("persisted response")
        );
    }

    #[test]
    fn inquire_with_invalid_provider_does_not_enter_loading() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.providers.update_preset(
            crate::llm::PresetProvider::OpenAI,
            crate::llm::PresetConfig { api_key: String::new() },
        );
        state.editor_buffers.set_instruction_text("ask this");

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Inquire),
        );

        assert!(!state.llm_requests.is_inquiring(root));
        assert!(
            state.errors.iter().any(|err| matches!(err, crate::app::AppError::Configuration(_)))
        );
        assert_eq!(state.editor_buffers.instruction_content().text(), "ask this");
        assert!(state.store.inquiry_draft(&root).is_none());
    }

    #[test]
    fn inquire_finished_discards_stale_response_after_context_change() {
        let (mut state, root) = test_state();
        let request_signature =
            state.block_context_signature(&root).expect("root has request signature");
        state.llm_requests.mark_inquiry_loading(root, request_signature);
        state.store.set_inquiry(root, "original inquiry".to_string());
        state.store.update_point(&root, "changed context".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireChunk {
                    block_id: root,
                    request_signature,
                    chunk: "stale response".to_string(),
                },
            ),
        );
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireFinished { block_id: root, request_signature },
            ),
        );

        assert_eq!(state.store.inquiry_draft(&root).map(|draft| draft.response.as_str()), Some(""));
    }

    #[test]
    fn inquire_stream_error_is_recorded_on_finish() {
        let (mut state, root) = test_state();
        let request_signature =
            state.block_context_signature(&root).expect("root has request signature");
        state.llm_requests.mark_inquiry_loading(root, request_signature);
        state.store.set_inquiry(root, "question".to_string());

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireFailed {
                    block_id: root,
                    request_signature,
                    reason: crate::app::UiError::from_message("network failed"),
                },
            ),
        );
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::InquireFinished { block_id: root, request_signature },
            ),
        );

        assert!(state.errors.iter().any(|err| matches!(err, crate::app::AppError::Inquire(_))));
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
