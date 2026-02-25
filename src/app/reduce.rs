//! Reduce handler: LLM-powered point reduction with redundant child management.
//!
//! Reduce takes a block's point text and its children context, sends them to the
//! LLM, and receives back a condensed version of the point plus a list of child
//! indices deemed redundant. The result is staged as a [`ReductionDraftRecord`]
//! for user review before committing.
//!
//! # Message lifecycle
//!
//! 1. [`ReduceMessage::Start`] — fires the LLM request (abortable).
//! 2. [`ReduceMessage::Cancel`] — aborts an in-flight request.
//! 3. [`ReduceMessage::Done`] — response arrived; stale-check then stage draft.
//! 4. [`ReduceMessage::Apply`] / [`ReduceMessage::Reject`] — commit or discard
//!    the staged reduction.
//! 5. Individual and bulk child-deletion accept/reject variants allow
//!    fine-grained control over which redundant children are removed.

use super::error::{AppError, UiError};
use super::llm_requests::RequestSignature;
use super::{AppState, LLM_REQUEST_TIMEOUT, Message};
use crate::llm;
use crate::store::{BlockId, ReductionDraftRecord};
use iced::Task;

/// Messages for the reduce workflow.
#[derive(Debug, Clone)]
pub enum ReduceMessage {
    Start(BlockId),
    Cancel(BlockId),
    Done {
        block_id: BlockId,
        request_signature: RequestSignature,
        result: Result<llm::ReduceResult, UiError>,
        children_snapshot: Vec<BlockId>,
    },
    Apply(BlockId),
    Reject(BlockId),
    AcceptChildDeletion {
        block_id: BlockId,
        child_index: usize,
    },
    RejectChildDeletion {
        block_id: BlockId,
        child_index: usize,
    },
    AcceptAllDeletions(BlockId),
    RejectAllDeletions(BlockId),
}

/// Handle a reduce message, returning any follow-up task.
pub fn handle(state: &mut AppState, message: ReduceMessage) -> Task<Message> {
    match message {
        | ReduceMessage::Start(block_id) => {
            state.overflow_open_for = None;
            if state.llm_requests.is_reducing(block_id) {
                return Task::none();
            }
            let context = state.store.block_context_for_id(&block_id);
            let Some(config) = state.llm_config_for_reduce(block_id) else {
                return Task::none();
            };
            tracing::info!(block_id = ?block_id, "reduce request started");
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            let children_snapshot: Vec<BlockId> = state.store.children(&block_id).to_vec();
            state.llm_requests.mark_reduce_loading(block_id, request_signature);
            let instruction = state.instruction_panel.prompt.take();
            let request_task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.reduce_block(&context, instruction.as_deref()),
                        )
                        .await,
                        "reduce request timed out after 30 seconds",
                    )
                },
                move |result| {
                    Message::Reduce(ReduceMessage::Done {
                        block_id,
                        request_signature,
                        result,
                        children_snapshot,
                    })
                },
            );
            let (request_task, handle) = Task::abortable(request_task);
            state.llm_requests.replace_reduce_handle(block_id, handle);
            request_task
        }
        | ReduceMessage::Cancel(block_id) => {
            if state.llm_requests.cancel_reduce(block_id) {
                tracing::info!(block_id = ?block_id, "reduce request cancelled");
            }
            Task::none()
        }
        | ReduceMessage::Done { block_id, request_signature, result, children_snapshot } => {
            let pending_signature = state.llm_requests.finish_reduce_request(block_id);
            if state.store.node(&block_id).is_none() {
                return Task::none();
            }
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale reduce response after context changed"
                );
                return Task::none();
            }
            match result {
                | Ok(reduce_result) => {
                    let (reduction, redundant_indices) = reduce_result.into_parts();
                    let redundant_children: Vec<BlockId> = redundant_indices
                        .iter()
                        .filter_map(|&idx| children_snapshot.get(idx).copied())
                        .collect();
                    tracing::info!(
                        block_id = ?block_id,
                        chars = reduction.len(),
                        redundant_children = redundant_children.len(),
                        "reduce request succeeded"
                    );
                    state.mutate_with_undo_and_persist("after creating reduction draft", |state| {
                        state.store.insert_reduction_draft(
                            block_id,
                            ReductionDraftRecord { reduction, redundant_children },
                        );
                        state.errors.retain(|err| !matches!(err, AppError::Reduce(_)));
                        true
                    });
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "reduce request failed");
                    state.llm_requests.set_reduce_error(block_id, reason.clone());
                    state.record_error(AppError::Reduce(reason));
                }
            }
            // Clear instruction prompt after reduce completes
            state.instruction_panel.prompt = None;
            Task::none()
        }
        | ReduceMessage::Apply(block_id) => {
            state.mutate_with_undo_and_persist("after applying reduction", |state| {
                if let Some(draft) = state.store.remove_reduction_draft(&block_id) {
                    tracing::info!(
                        block_id = ?block_id,
                        chars = draft.reduction.len(),
                        deletions = draft.redundant_children.len(),
                        "applied reduction with child deletions"
                    );
                    state.store.update_point(&block_id, draft.reduction.clone());
                    state.editor_buffers.set_text(&block_id, &draft.reduction);
                    for child_id in &draft.redundant_children {
                        if state.store.node(child_id).is_some()
                            && let Some(removed_ids) = state.store.remove_block_subtree(child_id)
                        {
                            state.editor_buffers.remove_blocks(&removed_ids);
                            for id in &removed_ids {
                                state.llm_requests.remove_block(*id);
                            }
                        }
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | ReduceMessage::Reject(block_id) => {
            tracing::info!(block_id = ?block_id, "rejected reduction");
            state.store.remove_reduction_draft(&block_id);
            state.persist_with_context("after rejecting reduction");
            Task::none()
        }
        | ReduceMessage::AcceptChildDeletion { block_id, child_index } => {
            state.mutate_with_undo_and_persist("after accepting child deletion", |state| {
                let child_id = state
                    .store
                    .reduction_draft(&block_id)
                    .and_then(|d| d.redundant_children.get(child_index).copied())
                    .filter(|id| state.store.node(id).is_some());
                if let Some(child_id) = child_id
                    && let Some(removed_ids) = state.store.remove_block_subtree(&child_id)
                {
                    state.editor_buffers.remove_blocks(&removed_ids);
                    for id in &removed_ids {
                        state.llm_requests.remove_block(*id);
                    }
                }
                if let Some(draft) = state.store.reduction_draft(&block_id) {
                    let mut updated = draft.clone();
                    if child_index < updated.redundant_children.len() {
                        updated.redundant_children.remove(child_index);
                        state.store.insert_reduction_draft(block_id, updated);
                    }
                }
                true
            });
            Task::none()
        }
        | ReduceMessage::RejectChildDeletion { block_id, child_index } => {
            if let Some(draft) = state.store.reduction_draft(&block_id) {
                let mut updated = draft.clone();
                if child_index < updated.redundant_children.len() {
                    updated.redundant_children.remove(child_index);
                    state.store.insert_reduction_draft(block_id, updated);
                }
            }
            state.persist_with_context("after rejecting child deletion");
            Task::none()
        }
        | ReduceMessage::AcceptAllDeletions(block_id) => {
            state.mutate_with_undo_and_persist("after accepting all child deletions", |state| {
                let draft = state.store.reduction_draft(&block_id).cloned();
                if let Some(draft) = draft {
                    for child_id in &draft.redundant_children {
                        if state.store.node(child_id).is_some()
                            && let Some(removed_ids) = state.store.remove_block_subtree(child_id)
                        {
                            state.editor_buffers.remove_blocks(&removed_ids);
                            for id in &removed_ids {
                                state.llm_requests.remove_block(*id);
                            }
                        }
                    }
                    state.store.insert_reduction_draft(
                        block_id,
                        ReductionDraftRecord {
                            reduction: draft.reduction,
                            redundant_children: vec![],
                        },
                    );
                    return true;
                }
                false
            });
            Task::none()
        }
        | ReduceMessage::RejectAllDeletions(block_id) => {
            if let Some(draft) = state.store.reduction_draft(&block_id) {
                state.store.insert_reduction_draft(
                    block_id,
                    ReductionDraftRecord {
                        reduction: draft.reduction.clone(),
                        redundant_children: vec![],
                    },
                );
            }
            state.persist_with_context("after rejecting all child deletions");
            Task::none()
        }
    }
}
