//! Probe panels for instruction-driven LLM interactions.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Each click on the toolbar `Probe` action creates a fresh transient panel for
//! the target block. This matches the patch-panel lifecycle more closely than a
//! toggle surface: once created, a panel is only closed by actions inside that
//! panel.
//!
//! Probe panels are intentionally decoupled from the persisted block-panel bar
//! selection. The reference panel still uses that single-select persisted slot,
//! while probe panels stack independently so opening a probe never collapses an
//! already-open reference panel.
//!
//! A panel's visible context is the union of:
//! - the block point itself,
//! - the full parent chain (root -> target),
//! - all direct children of the target,
//! - all user-selected friend blocks for the target.
//!
//! # Instruction Draft Lifecycle
//!
//! The instruction editor is treated as a short-lived panel-local draft buffer
//! whose text is authored before submission through one of three actions:
//! - **Probe**: Ask targeted questions to clarify meaning, fill gaps, or challenge
//!   assumptions. Returns a free-form response draft for user-directed insertion.
//! - **Amplify**: Add detail, examples, and context; draft injected as extra guidance.
//! - **Distill**: Summarize into a shorter version; draft injected as extra guidance.
//!
//! # Probe Result Contract
//!
//! Probe returns one response draft scoped to the focused block context at the
//! time of submission. The product intent is that this draft can be inserted by
//! the user either into the current point or as a new child point.
//!
//! # Amplify/Distill Contract
//!
//! Amplify and distill preserve their canonical semantics from `crate::llm`
//! prompt builders; instruction text only adds additional guidance and does not
//! redefine the output schema.
//!
//! # Probe Apply Operations
//!
//! Probe response drafts support three explicit apply actions:
//! - replace target point with response,
//! - append response to target point,
//! - add response as a new child under the target.
//!
//! Note: the editor phase has an explicit close button that clears the current
//! panel-local input draft. Once a probe request is pending or a probe result
//! exists, that header close affordance is intentionally hidden so dismissing
//! the result remains an explicit action.

use crate::app::{AppState, Message, RequestSignature};
use crate::component::floating_panel::PanelHeader;
use crate::component::icon_button::IconButton;
use crate::component::text_button::TextButton;
use crate::llm;
use crate::store::BlockId;
use crate::theme;
use rust_i18n::t;

use iced::Element;
use iced::widget::{button, container, text, text_editor};
use lucide_icons::iced as icons;
use std::time::Duration;

use super::state::{ProbePanelId, ProbePanelState};

const LLM_REQUEST_TIMEOUT: Duration = theme::INSTRUCTION_LLM_TIMEOUT;

/// Message types for probe-panel interactions.
#[derive(Debug, Clone)]
pub enum InstructionPanelMessage {
    /// Append a new probe panel instance for the target block.
    OpenPanel,
    /// Close the editor phase and discard the current panel-local input draft.
    ClosePanel { panel_id: ProbePanelId },
    /// Text edited in one probe panel.
    TextEdited { panel_id: ProbePanelId, action: iced::widget::text_editor::Action },
    /// Send probe to LLM with the current panel instruction.
    Probe { panel_id: ProbePanelId },
    /// One probe response chunk arrived.
    ProbeChunk {
        block_id: BlockId,
        panel_id: ProbePanelId,
        request_signature: RequestSignature,
        chunk: String,
    },
    /// Probe stream reported an error.
    ProbeFailed {
        block_id: BlockId,
        panel_id: ProbePanelId,
        request_signature: RequestSignature,
        reason: crate::app::UiError,
    },
    /// Probe request completed (successfully or with error).
    ProbeFinished { block_id: BlockId, panel_id: ProbePanelId, request_signature: RequestSignature },
    /// Cancel an in-flight probe request.
    CancelProbe { panel_id: ProbePanelId },
    /// Amplify with the panel instruction as extra prompt guidance.
    AmplifyWithInstruction { panel_id: ProbePanelId },
    /// Distill with the panel instruction as extra prompt guidance.
    DistillWithInstruction { panel_id: ProbePanelId },
    /// Apply rewrite from one probe result.
    ApplyInstructionRewrite { panel_id: ProbePanelId },
    /// Append probe result to the target block point.
    AppendInstructionResponse { panel_id: ProbePanelId },
    /// Add probe result as a new child under the target block.
    AddInstructionResponseAsChild { panel_id: ProbePanelId },
    /// Dismiss one probe result panel.
    Dismiss { panel_id: ProbePanelId },
}

/// Handle probe-panel messages.
pub fn handle(
    state: &mut AppState, target_block_id: BlockId, msg: InstructionPanelMessage,
) -> iced::Task<Message> {
    use super::patch::{PatchKind, PatchMessage};

    match msg {
        | InstructionPanelMessage::OpenPanel => {
            let panel_id = next_panel_id(state);
            state
                .ui_mut()
                .probe_panels
                .entry(target_block_id)
                .or_default()
                .push(ProbePanelState::new(panel_id));
            state.persist_with_context("after opening probe panel");
            iced::Task::none()
        }
        | InstructionPanelMessage::ClosePanel { panel_id } => {
            remove_probe_panel(state, target_block_id, panel_id);
            iced::Task::none()
        }
        | InstructionPanelMessage::TextEdited { panel_id, action } => {
            if let Some(panel) = probe_panel_mut(state, target_block_id, panel_id) {
                panel.instruction.perform(action);
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::Probe { panel_id } => {
            if state.llm_requests.is_probing(target_block_id) {
                return iced::Task::none();
            }
            let Some(instruction) = panel_instruction(state, target_block_id, panel_id) else {
                return iced::Task::none();
            };
            if instruction.is_empty() {
                return iced::Task::none();
            }
            let context = state.store.block_context_for_id(&target_block_id);
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return iced::Task::none();
            };
            let Some(config) = state.llm_config_for_probe() else {
                return iced::Task::none();
            };
            if let Some(panel) = probe_panel_mut(state, target_block_id, panel_id) {
                panel.inquiry = Some(instruction.clone());
                panel.response.clear();
                panel.is_probing = true;
                panel.instruction = text_editor::Content::new();
            }
            state.llm_requests.mark_probe_loading(target_block_id, request_signature);
            tracing::info!(block_id = ?target_block_id, panel_id = panel_id.0, "probe panel inquiry started");
            let probe_max_tokens = state.config.tasks.probe.token_limit.as_api_param();
            let prompt_config = llm::TaskPromptConfig::probe(
                &state.config.tasks.probe.system_prompt,
                &state.config.tasks.probe.user_prompt,
            );
            let client = llm::LlmClient::new(config);
            let request_task = iced::Task::run(
                client.probe_stream(
                    context,
                    instruction,
                    LLM_REQUEST_TIMEOUT,
                    probe_max_tokens,
                    prompt_config,
                ),
                move |event| match event {
                    | llm::ProbeStreamEvent::Chunk(chunk) => Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::ProbeChunk {
                            block_id: target_block_id,
                            panel_id,
                            request_signature,
                            chunk,
                        },
                    ),
                    | llm::ProbeStreamEvent::Failed(err) => Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::ProbeFailed {
                            block_id: target_block_id,
                            panel_id,
                            request_signature,
                            reason: crate::app::UiError::from_message(err),
                        },
                    ),
                    | llm::ProbeStreamEvent::Finished => Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::ProbeFinished {
                            block_id: target_block_id,
                            panel_id,
                            request_signature,
                        },
                    ),
                },
            );
            let (request_task, handle) = iced::Task::abortable(request_task);
            state.llm_requests.replace_probe_handle(target_block_id, handle);
            request_task
        }
        | InstructionPanelMessage::ProbeChunk { block_id, panel_id, request_signature, chunk } => {
            if state.store.node(&block_id).is_none() {
                return iced::Task::none();
            }
            if state.is_stale_response(&block_id, request_signature) {
                tracing::info!(
                    block_id = ?block_id,
                    panel_id = panel_id.0,
                    "discarded stale probe panel chunk after context changed"
                );
                return iced::Task::none();
            }
            if let Some(panel) = probe_panel_mut(state, block_id, panel_id) {
                panel.response.push_str(&chunk);
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::ProbeFailed {
            block_id,
            panel_id,
            request_signature,
            reason,
        } => {
            if state.store.node(&block_id).is_none() {
                return iced::Task::none();
            }
            if state.is_stale_response(&block_id, request_signature) {
                tracing::info!(
                    block_id = ?block_id,
                    panel_id = panel_id.0,
                    "discarded stale probe panel error after context changed"
                );
                return iced::Task::none();
            }
            tracing::error!(
                block_id = ?block_id,
                panel_id = panel_id.0,
                reason = %reason.as_str(),
                "probe panel stream failed"
            );
            state.llm_requests.set_probe_error(block_id, reason);
            iced::Task::none()
        }
        | InstructionPanelMessage::ProbeFinished { block_id, panel_id, request_signature } => {
            let (pending_signature, pending_error) =
                state.llm_requests.finish_probe_request(block_id);
            if state.store.node(&block_id).is_none() {
                return iced::Task::none();
            }
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    panel_id = panel_id.0,
                    "discarded stale probe panel response after context changed"
                );
                return iced::Task::none();
            }
            let response_len = probe_panel(state, block_id, panel_id)
                .map(|panel| panel.response.trim())
                .filter(|response| !response.is_empty())
                .map(str::len)
                .unwrap_or(0);
            let had_stream_error = pending_error.is_some();

            if let Some(panel) = probe_panel_mut(state, block_id, panel_id) {
                panel.is_probing = false;
                if response_len == 0 {
                    panel.inquiry = None;
                }
            }

            if let Some(reason) = pending_error {
                state.record_error(crate::app::AppError::Probe(reason));
            }

            if response_len > 0 {
                tracing::info!(
                    block_id = ?block_id,
                    panel_id = panel_id.0,
                    chars = response_len,
                    "probe panel completed"
                );
                if !had_stream_error {
                    state.errors.retain(|err| !matches!(err, crate::app::AppError::Probe(_)));
                }
            } else if !had_stream_error {
                state.record_error(crate::app::AppError::Probe(crate::app::UiError::from_message(
                    "probe returned no usable text",
                )));
                tracing::error!(
                    block_id = ?block_id,
                    panel_id = panel_id.0,
                    "probe panel finished without usable response"
                );
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::CancelProbe { panel_id } => {
            if state.llm_requests.cancel_probe(target_block_id) {
                if let Some(panel) = probe_panel_mut(state, target_block_id, panel_id) {
                    panel.is_probing = false;
                    if panel.response.trim().is_empty() {
                        panel.inquiry = None;
                    }
                }
                tracing::info!(block_id = ?target_block_id, panel_id = panel_id.0, "probe panel inquiry cancelled");
            }
            iced::Task::none()
        }
        | InstructionPanelMessage::AmplifyWithInstruction { panel_id } => {
            let Some(instruction) = panel_instruction(state, target_block_id, panel_id) else {
                return iced::Task::none();
            };
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.store.set_instruction_draft(target_block_id, instruction);
            remove_probe_panel(state, target_block_id, panel_id);
            crate::app::AppState::update(
                state,
                Message::Patch(PatchMessage::Start {
                    kind: PatchKind::Amplify,
                    block_id: target_block_id,
                }),
            )
        }
        | InstructionPanelMessage::DistillWithInstruction { panel_id } => {
            let Some(instruction) = panel_instruction(state, target_block_id, panel_id) else {
                return iced::Task::none();
            };
            if instruction.is_empty() {
                return iced::Task::none();
            }
            state.store.set_instruction_draft(target_block_id, instruction);
            remove_probe_panel(state, target_block_id, panel_id);
            crate::app::AppState::update(
                state,
                Message::Patch(PatchMessage::Start {
                    kind: PatchKind::Distill,
                    block_id: target_block_id,
                }),
            )
        }
        | InstructionPanelMessage::ApplyInstructionRewrite { panel_id } => {
            let Some(response) = panel_response(state, target_block_id, panel_id) else {
                return iced::Task::none();
            };
            state.mutate_with_undo_and_persist("after applying probe panel rewrite", |state| {
                state.store.update_point(&target_block_id, response.clone());
                state.editor_buffers.set_text(&target_block_id, &response);
                true
            });
            remove_probe_panel(state, target_block_id, panel_id);
            iced::Task::none()
        }
        | InstructionPanelMessage::AppendInstructionResponse { panel_id } => {
            let Some(response) = panel_response(state, target_block_id, panel_id) else {
                return iced::Task::none();
            };
            state.mutate_with_undo_and_persist("after appending probe panel response", |state| {
                let current = state.store.point(&target_block_id).unwrap_or_default();
                let next = if current.trim().is_empty() {
                    response.clone()
                } else {
                    format!("{current}\n\n{response}")
                };
                state.store.update_point(&target_block_id, next.clone());
                state.editor_buffers.set_text(&target_block_id, &next);
                true
            });
            remove_probe_panel(state, target_block_id, panel_id);
            iced::Task::none()
        }
        | InstructionPanelMessage::AddInstructionResponseAsChild { panel_id } => {
            let Some(response) = panel_response(state, target_block_id, panel_id) else {
                return iced::Task::none();
            };
            state.mutate_with_undo_and_persist(
                "after adding probe panel response as child",
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
            remove_probe_panel(state, target_block_id, panel_id);
            iced::Task::none()
        }
        | InstructionPanelMessage::Dismiss { panel_id } => {
            remove_probe_panel(state, target_block_id, panel_id);
            iced::Task::none()
        }
    }
}

fn next_panel_id(state: &mut AppState) -> ProbePanelId {
    let next = ProbePanelId(state.ui().next_probe_panel_id);
    state.ui_mut().next_probe_panel_id += 1;
    next
}

fn probe_panel(
    state: &AppState, block_id: BlockId, panel_id: ProbePanelId,
) -> Option<&ProbePanelState> {
    state
        .ui()
        .probe_panels
        .get(&block_id)
        .and_then(|panels| panels.iter().find(|panel| panel.id == panel_id))
}

fn probe_panel_mut(
    state: &mut AppState, block_id: BlockId, panel_id: ProbePanelId,
) -> Option<&mut ProbePanelState> {
    state
        .ui_mut()
        .probe_panels
        .get_mut(&block_id)
        .and_then(|panels| panels.iter_mut().find(|panel| panel.id == panel_id))
}

fn remove_probe_panel(state: &mut AppState, block_id: BlockId, panel_id: ProbePanelId) {
    let became_empty = {
        let Some(panels) = state.ui_mut().probe_panels.get_mut(&block_id) else {
            return;
        };
        panels.retain(|panel| panel.id != panel_id);
        panels.is_empty()
    };

    if became_empty {
        state.ui_mut().probe_panels.remove(&block_id);
        state.persist_with_context("after removing last probe panel");
    }
}

fn panel_instruction(
    state: &AppState, block_id: BlockId, panel_id: ProbePanelId,
) -> Option<String> {
    probe_panel(state, block_id, panel_id)
        .map(|panel| panel.instruction.text().trim().to_string())
        .filter(|instruction| !instruction.is_empty())
}

fn panel_response(state: &AppState, block_id: BlockId, panel_id: ProbePanelId) -> Option<String> {
    probe_panel(state, block_id, panel_id)
        .map(|panel| panel.response.trim().to_string())
        .filter(|response| !response.is_empty())
}

/// Render all probe panels for `target_block_id`.
pub fn view<'a>(state: &'a AppState, target_block_id: BlockId) -> Element<'a, Message> {
    use iced::widget::{column, row, scrollable};

    let Some(panels) = state.ui().probe_panels.get(&target_block_id) else {
        return container(iced::widget::Text::new("")).into();
    };

    let mut content = column![].spacing(theme::PANEL_INNER_GAP);
    for panel in panels {
        if panel.is_result_phase() {
            let inquiry_text = panel.inquiry.as_deref().unwrap_or_default();
            let inquiry_section = column![].spacing(theme::PANEL_INNER_GAP).push(
                container(
                    text(t!("instruction_probe_label").to_string())
                        .font(theme::INTER)
                        .size(theme::INSTRUCTION_BUTTON_SIZE),
                )
                .width(iced::Length::Fill),
            );

            let inquiry_content = container(
                scrollable(
                    text(inquiry_text).font(theme::LXGW_WENKAI).size(theme::INPUT_TEXT_SIZE),
                )
                .width(iced::Length::Fill),
            )
            .padding(iced::Padding::from([theme::COMPACT_PAD_V, theme::PANEL_PAD_V]))
            .style(theme::draft_panel)
            .width(iced::Length::Fill);

            let mut panel_content =
                column![inquiry_section, inquiry_content].spacing(theme::PANEL_INNER_GAP);

            if panel.is_probing {
                let button_row = row![].spacing(theme::PANEL_BUTTON_GAP).push(
                    button(
                        row![]
                            .spacing(theme::INLINE_GAP)
                            .align_y(iced::Alignment::Center)
                            .push(
                                icons::icon_loader().size(theme::INSTRUCTION_BUTTON_SIZE).center(),
                            )
                            .push(
                                text(t!("instruction_probing").to_string())
                                    .font(theme::INTER)
                                    .size(theme::INSTRUCTION_BUTTON_SIZE),
                            ),
                    )
                    .style(theme::destructive_button)
                    .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .on_press(Message::InstructionPanel(
                        target_block_id,
                        InstructionPanelMessage::CancelProbe { panel_id: panel.id },
                    )),
                );
                panel_content = panel_content.push(button_row);
            }

            if !panel.response.trim().is_empty() {
                use super::patch_panel::{PanelButton, PanelButtonStyle, RewriteSection};
                let response = panel.response.as_str().to_string();
                let response_content: iced::Element<'_, Message> = container(
                    scrollable(
                        text(response).font(theme::LXGW_WENKAI).size(theme::INPUT_TEXT_SIZE),
                    )
                    .width(iced::Length::Fill),
                )
                .width(iced::Length::Fill)
                .into();
                let buttons = if !panel.is_probing {
                    vec![
                        PanelButton {
                            label: t!("instruction_apply_rewrite").to_string(),
                            style: PanelButtonStyle::Action,
                            on_press: Message::InstructionPanel(
                                target_block_id,
                                InstructionPanelMessage::ApplyInstructionRewrite {
                                    panel_id: panel.id,
                                },
                            ),
                        },
                        PanelButton {
                            label: t!("instruction_append_block").to_string(),
                            style: PanelButtonStyle::Action,
                            on_press: Message::InstructionPanel(
                                target_block_id,
                                InstructionPanelMessage::AppendInstructionResponse {
                                    panel_id: panel.id,
                                },
                            ),
                        },
                        PanelButton {
                            label: t!("instruction_add_child").to_string(),
                            style: PanelButtonStyle::Action,
                            on_press: Message::InstructionPanel(
                                target_block_id,
                                InstructionPanelMessage::AddInstructionResponseAsChild {
                                    panel_id: panel.id,
                                },
                            ),
                        },
                        PanelButton {
                            label: t!("ui_discard").to_string(),
                            style: PanelButtonStyle::Destructive,
                            on_press: Message::InstructionPanel(
                                target_block_id,
                                InstructionPanelMessage::Dismiss { panel_id: panel.id },
                            ),
                        },
                    ]
                } else {
                    vec![]
                };
                panel_content = panel_content.push(super::patch_panel::view(
                    state.is_dark_mode(),
                    Some(RewriteSection::Content {
                        title: t!("instruction_response").to_string(),
                        content: response_content,
                        buttons,
                    }),
                    None,
                ));
            }

            content = content.push(
                container(panel_content)
                    .padding(iced::Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
                    .style(theme::draft_panel),
            );
            continue;
        }

        let title = container(
            text(t!("action_probe").to_string())
                .font(theme::INTER)
                .size(theme::INSTRUCTION_BUTTON_SIZE),
        )
        .padding(iced::Padding::from([theme::COMPACT_PAD_V, theme::COMPACT_PAD_H]));
        let close_btn = IconButton::panel_close().on_press(Message::InstructionPanel(
            target_block_id,
            InstructionPanelMessage::ClosePanel { panel_id: panel.id },
        ));
        let header = PanelHeader::new(title, close_btn);

        let editor = container(
            text_editor(&panel.instruction)
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
                        target_block_id,
                        InstructionPanelMessage::TextEdited { panel_id: panel.id, action },
                    )
                }),
        )
        .padding(iced::Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]));

        let actions = row![]
            .spacing(theme::PANEL_BUTTON_GAP)
            .push(
                TextButton::action(
                    t!("instruction_amplify").to_string(),
                    theme::INSTRUCTION_BUTTON_SIZE,
                )
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(
                    target_block_id,
                    InstructionPanelMessage::AmplifyWithInstruction { panel_id: panel.id },
                )),
            )
            .push(
                TextButton::action(
                    t!("instruction_distill").to_string(),
                    theme::INSTRUCTION_BUTTON_SIZE,
                )
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(
                    target_block_id,
                    InstructionPanelMessage::DistillWithInstruction { panel_id: panel.id },
                )),
            )
            .push(
                TextButton::action(
                    t!("instruction_probe").to_string(),
                    theme::INSTRUCTION_BUTTON_SIZE,
                )
                .height(iced::Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(
                    target_block_id,
                    InstructionPanelMessage::Probe { panel_id: panel.id },
                )),
            );

        content = content.push(
            container(column![header, editor, actions].spacing(theme::PANEL_INNER_GAP))
                .padding(iced::Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
                .style(theme::draft_panel),
        );
    }

    content.into()
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> (AppState, BlockId) {
        AppState::test_state()
    }

    fn open_panel(state: &mut AppState, block_id: BlockId) -> ProbePanelId {
        let _ = AppState::update(
            state,
            Message::InstructionPanel(block_id, InstructionPanelMessage::OpenPanel),
        );
        state.ui().probe_panels[&block_id].last().expect("panel created").id
    }

    #[test]
    fn open_panel_appends_multiple_probe_panels() {
        let (mut state, root) = test_state();
        state.set_focus(root);

        let first = open_panel(&mut state, root);
        let second = open_panel(&mut state, root);

        assert_ne!(first, second);
        assert_eq!(state.ui().probe_panels[&root].len(), 2);
    }

    #[test]
    fn close_panel_removes_only_target_probe_panel() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        let first = open_panel(&mut state, root);
        let second = open_panel(&mut state, root);

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::ClosePanel { panel_id: first },
            ),
        );

        let remaining = &state.ui().probe_panels[&root];
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, second);
    }

    #[test]
    fn close_last_panel_clears_last_transient_probe_panel() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        let panel_id = open_panel(&mut state, root);

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::ClosePanel { panel_id }),
        );

        assert!(state.ui().probe_panels.get(&root).is_none());
    }

    #[test]
    fn opening_probe_panel_preserves_reference_panel_selection() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.store.set_block_panel_state(&root, Some(BlockPanelBarState::References));

        let _ = open_panel(&mut state, root);

        assert_eq!(
            state.store.block_panel_state(&root).copied(),
            Some(BlockPanelBarState::References)
        );
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
        let panel_id = open_panel(&mut state, sibling);
        if let Some(panel) = probe_panel_mut(&mut state, sibling, panel_id) {
            panel.inquiry = Some("question".to_string());
            panel.response = "inquiry response".to_string();
        }

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                sibling,
                InstructionPanelMessage::AppendInstructionResponse { panel_id },
            ),
        );

        assert_eq!(
            state.store.point(&sibling).as_deref(),
            Some("sibling text\n\ninquiry response")
        );
        assert_eq!(state.store.point(&root).as_deref(), Some("root text"));
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
        let panel_id = open_panel(&mut state, sibling);
        if let Some(panel) = probe_panel_mut(&mut state, sibling, panel_id) {
            panel.inquiry = Some("question".to_string());
            panel.response = "child from inquiry".to_string();
        }

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                sibling,
                InstructionPanelMessage::AddInstructionResponseAsChild { panel_id },
            ),
        );

        let children = state.store.children(&sibling);
        assert_eq!(children.len(), before_len + 1);
        let child_id = *children.last().expect("new child added under sibling");
        assert_eq!(state.store.point(&child_id).as_deref(), Some("child from inquiry"));
    }

    #[test]
    fn inquire_submission_consumes_panel_input() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        let panel_id = open_panel(&mut state, root);
        if let Some(panel) = probe_panel_mut(&mut state, root, panel_id) {
            panel.instruction = text_editor::Content::with_text("ask this");
        }

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Probe { panel_id }),
        );

        assert!(state.llm_requests.is_probing(root));
        let panel = probe_panel(&state, root, panel_id).expect("panel still present");
        assert_eq!(panel.inquiry.as_deref(), Some("ask this"));
        assert!(panel.instruction.text().is_empty());
    }

    #[test]
    fn inquire_finished_persists_streamed_probe_draft_on_panel() {
        let (mut state, root) = test_state();
        let panel_id = open_panel(&mut state, root);
        let request_signature =
            state.block_context_signature(&root).expect("root has request signature");
        state.llm_requests.mark_probe_loading(root, request_signature);
        if let Some(panel) = probe_panel_mut(&mut state, root, panel_id) {
            panel.inquiry = Some("question".to_string());
            panel.is_probing = true;
        }
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::ProbeChunk {
                    block_id: root,
                    panel_id,
                    request_signature,
                    chunk: "persisted ".to_string(),
                },
            ),
        );
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::ProbeChunk {
                    block_id: root,
                    panel_id,
                    request_signature,
                    chunk: "response".to_string(),
                },
            ),
        );
        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::ProbeFinished {
                    block_id: root,
                    panel_id,
                    request_signature,
                },
            ),
        );

        let panel = probe_panel(&state, root, panel_id).expect("panel present");
        assert_eq!(panel.response, "persisted response");
        assert!(!panel.is_probing);
    }

    #[test]
    fn expand_with_instruction_closes_only_submitting_panel() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        let first = open_panel(&mut state, root);
        let second = open_panel(&mut state, root);
        if let Some(panel) = probe_panel_mut(&mut state, root, first) {
            panel.instruction = text_editor::Content::with_text("expand this");
        }

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(
                root,
                InstructionPanelMessage::AmplifyWithInstruction { panel_id: first },
            ),
        );

        let remaining = &state.ui().probe_panels[&root];
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, second);
    }

    #[test]
    fn dismiss_removes_only_target_result_panel() {
        let (mut state, root) = test_state();
        let first = open_panel(&mut state, root);
        let second = open_panel(&mut state, root);
        if let Some(panel) = probe_panel_mut(&mut state, root, first) {
            panel.inquiry = Some("q1".to_string());
            panel.response = "r1".to_string();
        }
        if let Some(panel) = probe_panel_mut(&mut state, root, second) {
            panel.inquiry = Some("q2".to_string());
            panel.response = "r2".to_string();
        }

        let _ = AppState::update(
            &mut state,
            Message::InstructionPanel(root, InstructionPanelMessage::Dismiss { panel_id: first }),
        );

        let remaining = &state.ui().probe_panels[&root];
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].response, "r2");
    }

    #[test]
    fn result_phase_hides_editor_close_button() {
        let panel = ProbePanelState::new(ProbePanelId(1));
        assert!(!panel.is_result_phase());

        let mut probing = ProbePanelState::new(ProbePanelId(2));
        probing.is_probing = true;
        assert!(probing.is_result_phase());

        let mut result = ProbePanelState::new(ProbePanelId(3));
        result.inquiry = Some("question".to_string());
        assert!(result.is_result_phase());
    }
}
