//! Atomize handler: LLM-powered block decomposition into distinct information points.
//!
//! Atomize takes a block's point text and context, sends them to the LLM, and
//! receives back a list of distinct information points. The result is staged as
//! an [`AtomizationDraftRecord`] for user review before committing.
//!
//! # Message lifecycle
//!
//! 1. [`AtomizeMessage::Start`] — fires the LLM request (abortable).
//! 2. [`AtomizeMessage::Cancel`] — aborts an in-flight request.
//! 3. [`AtomizeMessage::Done`] — response arrived; stale-check then stage draft.
//! 4. [`AtomizeMessage::AcceptChild`] / [`AtomizeMessage::RejectChild`] —
//!    accept or reject individual suggested points.
//! 5. [`AtomizeMessage::AcceptAllChildren`] / [`AtomizeMessage::DiscardAllChildren`] —
//!    bulk accept or discard.

use super::error::{AppError, UiError};
use super::llm_requests::RequestSignature;
use super::{AppState, LLM_REQUEST_TIMEOUT, Message};
use crate::llm;
use crate::store::{AtomizationDraftRecord, BlockId};
use iced::Task;

/// Messages for the atomize workflow.
#[derive(Debug, Clone)]
pub enum AtomizeMessage {
    Start(BlockId),
    Cancel(BlockId),
    Done {
        block_id: BlockId,
        request_signature: RequestSignature,
        result: Result<llm::AtomizeResult, UiError>,
    },
    AcceptChild {
        block_id: BlockId,
        child_index: usize,
    },
    RejectChild {
        block_id: BlockId,
        child_index: usize,
    },
    AcceptAllChildren(BlockId),
    DiscardAllChildren(BlockId),
}

/// Process one atomize message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: AtomizeMessage) -> Task<Message> {
    match message {
        | AtomizeMessage::Start(block_id) => {
            state.set_overflow_open(false);
            if state.llm_requests.is_atomizing(block_id) {
                return Task::none();
            }
            let context = state.store.block_context_for_id(&block_id);
            let Some(config) = state.llm_config_for_atomize(block_id) else {
                return Task::none();
            };

            tracing::info!(block_id = ?block_id, "atomize request started");
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            state.llm_requests.mark_atomize_loading(block_id, request_signature);
            let instruction =
                state.store.remove_instruction_draft(&block_id).map(|d| d.instruction);
            let atomize_max_tokens = state.config.tasks.atomize.token_limit.as_api_param();
            let prompt_config = llm::TaskPromptConfig::atomize(
                &state.config.tasks.atomize.system_prompt,
                &state.config.tasks.atomize.user_prompt,
            );
            let request_task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.atomize_block(
                                &context,
                                instruction.as_deref(),
                                atomize_max_tokens,
                                &prompt_config,
                            ),
                        )
                        .await,
                        format!(
                            "atomize request timed out after {} seconds",
                            LLM_REQUEST_TIMEOUT.as_secs()
                        ),
                    )
                },
                move |result| {
                    Message::Atomize(AtomizeMessage::Done {
                        block_id,
                        request_signature,
                        result,
                    })
                },
            );
            let (request_task, handle) = Task::abortable(request_task);
            state.llm_requests.replace_atomize_handle(block_id, handle);
            request_task
        }
        | AtomizeMessage::Cancel(block_id) => {
            if state.llm_requests.cancel_atomize(block_id) {
                tracing::info!(block_id = ?block_id, "atomize request cancelled");
            }
            Task::none()
        }
        | AtomizeMessage::Done { block_id, request_signature, result } => {
            let pending_signature = state.llm_requests.finish_atomize_request(block_id);
            if state.store.node(&block_id).is_none() {
                return Task::none();
            }
            if pending_signature != Some(request_signature) {
                tracing::debug!(block_id = ?block_id, "stale atomize response discarded");
                return Task::none();
            }

            match result {
                | Ok(atomize_result) => {
                    let points = atomize_result.into_points();
                    let len = points.len();
                    state.store.insert_atomization_draft(
                        block_id,
                        AtomizationDraftRecord { points },
                    );
                    state.errors.retain(|err| !matches!(err, AppError::Atomize(_)));
                    tracing::info!(block_id = ?block_id, points = len, "atomize done");
                }
                | Err(reason) => {
                    state.record_error(AppError::Atomize(reason));
                    tracing::error!(block_id = ?block_id, "atomize failed");
                }
            }
            Task::none()
        }
        | AtomizeMessage::AcceptChild { block_id, child_index } => {
            let point_opt = state
                .store
                .atomization_draft_mut(&block_id)
                .and_then(|draft| {
                    if child_index < draft.points.len() {
                        Some(draft.points.remove(child_index))
                    } else {
                        None
                    }
                });
            if let Some(point) = point_opt {
                let _ = state.store.append_child(&block_id, point);
                state.persist_with_context("after atomize accept child");
                if let Some(draft) = state.store.atomization_draft(&block_id) {
                    if draft.points.is_empty() {
                        state.store.remove_atomization_draft(&block_id);
                    }
                }
            }
            Task::none()
        }
        | AtomizeMessage::RejectChild { block_id, child_index } => {
            if let Some(draft) = state.store.atomization_draft_mut(&block_id) {
                if child_index < draft.points.len() {
                    draft.points.remove(child_index);
                    if draft.points.is_empty() {
                        state.store.remove_atomization_draft(&block_id);
                    }
                }
            }
            Task::none()
        }
        | AtomizeMessage::AcceptAllChildren(block_id) => {
            if let Some(draft) = state.store.remove_atomization_draft(&block_id) {
                for point in draft.points {
                    let _ = state.store.append_child(&block_id, point);
                }
                state.persist_with_context("after atomize accept all");
            }
            Task::none()
        }
        | AtomizeMessage::DiscardAllChildren(block_id) => {
            state.store.remove_atomization_draft(&block_id);
            Task::none()
        }
    }
}
