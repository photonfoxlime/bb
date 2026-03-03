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
    ApplyRewrite(BlockId),
    RejectRewrite(BlockId),
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
            // Sync editor buffer to store when focus was on the point editor, then snapshot
            // for undo (aligns with expand and reduce behavior).
            if let Some(content) = state.editor_buffers.get(&block_id) {
                let text = content.text();
                if state.store.point(&block_id).as_deref() != Some(text.as_str()) {
                    state.store.update_point(&block_id, text.to_string());
                    state.editor_buffers.invalidate_token_cache(&block_id);
                }
            }
            state.snapshot_for_undo();
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
        | AtomizeMessage::ApplyRewrite(block_id) => {
            state.mutate_with_undo_and_persist("after atomize apply rewrite", |state| {
                let rewrite_opt = state
                    .store
                    .atomization_draft_mut(&block_id)
                    .and_then(|draft| draft.rewrite.take());
                if let Some(rewrite) = rewrite_opt {
                    state.store.update_point(&block_id, rewrite.clone());
                    state.editor_buffers.set_text(&block_id, &rewrite);
                    if let Some(draft) = state.store.atomization_draft(&block_id) {
                        if draft.points.is_empty() {
                            state.store.remove_atomization_draft(&block_id);
                        }
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | AtomizeMessage::RejectRewrite(block_id) => {
            if let Some(draft) = state.store.atomization_draft_mut(&block_id) {
                draft.rewrite = None;
                if draft.points.is_empty() {
                    state.store.remove_atomization_draft(&block_id);
                }
            }
            Task::none()
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
                    let (rewrite, points) = atomize_result.into_parts();
                    let rewrite_len = rewrite.is_some();
                    let points_len = points.len();
                    state.store.insert_atomization_draft(
                        block_id,
                        AtomizationDraftRecord { rewrite, points },
                    );
                    state.errors.retain(|err| !matches!(err, AppError::Atomize(_)));
                    tracing::info!(
                        block_id = ?block_id,
                        rewrite = rewrite_len,
                        points = points_len,
                        "atomize done"
                    );
                }
                | Err(reason) => {
                    state.record_error(AppError::Atomize(reason));
                    tracing::error!(block_id = ?block_id, "atomize failed");
                }
            }
            Task::none()
        }
        | AtomizeMessage::AcceptChild { block_id, child_index } => {
            state.mutate_with_undo_and_persist("after atomize accept child", |state| {
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
                    if let Some(child_id) = state.store.append_child(&block_id, point.clone()) {
                        state.editor_buffers.set_text(&child_id, &point);
                    }
                    if let Some(draft) = state.store.atomization_draft(&block_id) {
                        if draft.points.is_empty() && draft.rewrite.is_none() {
                            state.store.remove_atomization_draft(&block_id);
                        }
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | AtomizeMessage::RejectChild { block_id, child_index } => {
            if let Some(draft) = state.store.atomization_draft_mut(&block_id) {
                if child_index < draft.points.len() {
                    draft.points.remove(child_index);
                    if draft.points.is_empty() && draft.rewrite.is_none() {
                        state.store.remove_atomization_draft(&block_id);
                    }
                }
            }
            Task::none()
        }
        | AtomizeMessage::AcceptAllChildren(block_id) => {
            state.mutate_with_undo_and_persist("after atomize accept all", |state| {
                if let Some(draft) = state.store.remove_atomization_draft(&block_id) {
                    if let Some(rewrite) = draft.rewrite {
                        state.store.update_point(&block_id, rewrite.clone());
                        state.editor_buffers.set_text(&block_id, &rewrite);
                    }
                    for point in draft.points {
                        if let Some(child_id) = state.store.append_child(&block_id, point.clone()) {
                            state.editor_buffers.set_text(&child_id, &point);
                        }
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | AtomizeMessage::DiscardAllChildren(block_id) => {
            state.store.remove_atomization_draft(&block_id);
            Task::none()
        }
    }
}
