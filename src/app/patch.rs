//! LLM-powered patch workflows: expand, atomize, and reduce.
//!
//! A single patch view with different UI texts. All three share the same
//! lifecycle: Start (abortable LLM request) → Done (stale-check, stage draft) →
//! apply/reject and child suggestions.

use super::error::{AppError, UiError};
use super::llm_requests::RequestSignature;
use super::{AppState, LLM_REQUEST_TIMEOUT, Message};
use crate::component::text_button::TextButton;
use crate::llm;
use crate::store::{
    AtomizationDraftRecord, BlockId, ExpansionDraftRecord, ReductionDraftRecord,
};
use crate::theme;
use super::diff::{word_diff, WordChange};
use iced::{Color, Element, Length, Padding, Task};
use iced::widget::{column, container, rich_text, row, span, space, text};
use rust_i18n::t;

/// Patch operation kind; determines LLM call, draft storage, and UI labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchKind {
    Expand,
    Atomize,
    Reduce,
}

/// Unified patch message; carries [`PatchKind`] where branching is required.
#[derive(Debug, Clone)]
pub enum PatchMessage {
    Start { kind: PatchKind, block_id: BlockId },
    Cancel { kind: PatchKind, block_id: BlockId },
    Done {
        kind: PatchKind,
        block_id: BlockId,
        request_signature: RequestSignature,
        result: PatchDoneResult,
    },
    /// Apply optional rewrite (expand, atomize).
    ApplyRewrite(BlockId),
    RejectRewrite(BlockId),
    /// Accept suggested child (expand, atomize).
    AcceptChild { block_id: BlockId, child_index: usize },
    RejectChild { block_id: BlockId, child_index: usize },
    AcceptAllChildren(BlockId),
    DiscardAllChildren(BlockId),
    /// Apply replacement (reduce).
    Apply(BlockId),
    Reject(BlockId),
    /// Accept deletion of redundant child (reduce).
    AcceptChildDeletion { block_id: BlockId, child_index: usize },
    RejectChildDeletion { block_id: BlockId, child_index: usize },
    AcceptAllDeletions(BlockId),
    RejectAllDeletions(BlockId),
}

#[derive(Debug, Clone)]
pub enum PatchDoneResult {
    Expand(Result<llm::ExpandResult, UiError>),
    Atomize(Result<llm::AtomizeResult, UiError>),
    Reduce(Result<llm::ReduceResult, UiError>, Vec<BlockId>),
}

/// Labels for patch panel UI; varies by [`PatchKind`].
struct PatchLabels {
    section_title: String,
    apply_primary: String,
    dismiss_primary: String,
    children_header: String,
    bulk_primary: String,
    bulk_secondary: String,
    per_item_primary: String,
    per_item_secondary: String,
    mode: PatchPanelMode,
}

enum PatchPanelMode {
    /// Optional rewrite + suggested new children (expand, atomize).
    AddChildren,
    /// Required replacement + redundant children to delete (reduce).
    ReplaceWithDeletions,
}

fn labels_for(kind: PatchKind) -> PatchLabels {
    match kind {
        PatchKind::Expand => PatchLabels {
            section_title: t!("doc_rewrite").to_string(),
            apply_primary: t!("doc_apply_rewrite").to_string(),
            dismiss_primary: t!("doc_dismiss_rewrite").to_string(),
            children_header: t!("doc_child_suggestions").to_string(),
            bulk_primary: t!("doc_accept_all").to_string(),
            bulk_secondary: t!("doc_discard_all").to_string(),
            per_item_primary: t!("doc_keep").to_string(),
            per_item_secondary: t!("doc_drop").to_string(),
            mode: PatchPanelMode::AddChildren,
        },
        PatchKind::Atomize => PatchLabels {
            section_title: t!("doc_rewrite").to_string(),
            apply_primary: t!("doc_apply_rewrite").to_string(),
            dismiss_primary: t!("doc_dismiss_rewrite").to_string(),
            children_header: t!("doc_atomize_points").to_string(),
            bulk_primary: t!("doc_accept_all").to_string(),
            bulk_secondary: t!("doc_discard_all").to_string(),
            per_item_primary: t!("doc_keep").to_string(),
            per_item_secondary: t!("doc_drop").to_string(),
            mode: PatchPanelMode::AddChildren,
        },
        PatchKind::Reduce => PatchLabels {
            section_title: t!("doc_reduce").to_string(),
            apply_primary: t!("doc_apply_reduction").to_string(),
            dismiss_primary: t!("doc_dismiss_reduction").to_string(),
            children_header: t!("doc_redundant_children").to_string(),
            bulk_primary: t!("doc_delete_all").to_string(),
            bulk_secondary: t!("doc_keep_all").to_string(),
            per_item_primary: t!("doc_delete").to_string(),
            per_item_secondary: t!("doc_keep").to_string(),
            mode: PatchPanelMode::ReplaceWithDeletions,
        },
    }
}

/// Process one patch message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: PatchMessage) -> Task<Message> {
    match message {
        PatchMessage::Start { kind, block_id } => handle_start(state, kind, block_id),
        PatchMessage::Cancel { kind, block_id } => handle_cancel(state, kind, block_id),
        PatchMessage::Done { kind, block_id, request_signature, result } => {
            handle_done(state, kind, block_id, request_signature, result)
        }
        PatchMessage::ApplyRewrite(block_id) => handle_apply_rewrite(state, block_id),
        PatchMessage::RejectRewrite(block_id) => handle_reject_rewrite(state, block_id),
        PatchMessage::AcceptChild { block_id, child_index } => {
            handle_accept_child(state, block_id, child_index)
        }
        PatchMessage::RejectChild { block_id, child_index } => {
            handle_reject_child(state, block_id, child_index)
        }
        PatchMessage::AcceptAllChildren(block_id) => handle_accept_all_children(state, block_id),
        PatchMessage::DiscardAllChildren(block_id) => handle_discard_all_children(state, block_id),
        PatchMessage::Apply(block_id) => handle_apply_reduction(state, block_id),
        PatchMessage::Reject(block_id) => handle_reject_reduction(state, block_id),
        PatchMessage::AcceptChildDeletion { block_id, child_index } => {
            handle_accept_child_deletion(state, block_id, child_index)
        }
        PatchMessage::RejectChildDeletion { block_id, child_index } => {
            handle_reject_child_deletion(state, block_id, child_index)
        }
        PatchMessage::AcceptAllDeletions(block_id) => handle_accept_all_deletions(state, block_id),
        PatchMessage::RejectAllDeletions(block_id) => handle_reject_all_deletions(state, block_id),
    }
}

fn handle_start(state: &mut AppState, kind: PatchKind, block_id: BlockId) -> Task<Message> {
    state.set_overflow_open(false);
    let is_busy = match kind {
        PatchKind::Expand => state.llm_requests.is_expanding(block_id),
        PatchKind::Atomize => state.llm_requests.is_atomizing(block_id),
        PatchKind::Reduce => state.llm_requests.is_reducing(block_id),
    };
    if is_busy {
        return Task::none();
    }
    if let Some(content) = state.editor_buffers.get(&block_id) {
        let text = content.text();
        if state.store.point(&block_id).as_deref() != Some(text.as_str()) {
            state.store.update_point(&block_id, text.to_string());
            state.editor_buffers.invalidate_token_cache(&block_id);
        }
    }
    state.snapshot_for_undo();
    let context = state.store.block_context_for_id(&block_id);
    let (config, request_signature) = match kind {
        PatchKind::Expand => {
            let Some(config) = state.llm_config_for_expand(block_id) else { return Task::none() };
            let Some(sig) = RequestSignature::from_block_context(&context) else { return Task::none() };
            state.llm_requests.mark_expand_loading(block_id, sig);
            (config, sig)
        }
        PatchKind::Atomize => {
            let Some(config) = state.llm_config_for_atomize(block_id) else { return Task::none() };
            let Some(sig) = RequestSignature::from_block_context(&context) else { return Task::none() };
            state.llm_requests.mark_atomize_loading(block_id, sig);
            (config, sig)
        }
        PatchKind::Reduce => {
            let Some(config) = state.llm_config_for_reduce(block_id) else { return Task::none() };
            let Some(sig) = RequestSignature::from_block_context(&context) else { return Task::none() };
            state.llm_requests.mark_reduce_loading(block_id, sig);
            (config, sig)
        }
    };

    let kind_name = match kind {
        PatchKind::Expand => "expand",
        PatchKind::Atomize => "atomize",
        PatchKind::Reduce => "reduce",
    };
    tracing::info!(block_id = ?block_id, "{} request started", kind_name);

    let instruction = state.store.remove_instruction_draft(&block_id).map(|d| d.instruction);

    let request_task = match kind {
        PatchKind::Expand => {
            let max_tokens = state.config.tasks.expand.token_limit.as_api_param();
            let prompt =
                llm::TaskPromptConfig::expand(
                    &state.config.tasks.expand.system_prompt,
                    &state.config.tasks.expand.user_prompt,
                );
            let task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.expand_block(
                                &context,
                                instruction.as_deref(),
                                max_tokens,
                                &prompt,
                            ),
                        )
                        .await,
                        format!(
                            "expand request timed out after {} seconds",
                            LLM_REQUEST_TIMEOUT.as_secs()
                        ),
                    )
                },
                move |r| {
                    Message::Patch(PatchMessage::Done {
                        kind: PatchKind::Expand,
                        block_id,
                        request_signature,
                        result: PatchDoneResult::Expand(r),
                    })
                },
            );
            let (task, handle) = Task::abortable(task);
            state.llm_requests.replace_expand_handle(block_id, handle);
            task
        }
        PatchKind::Atomize => {
            let max_tokens = state.config.tasks.atomize.token_limit.as_api_param();
            let prompt =
                llm::TaskPromptConfig::atomize(
                    &state.config.tasks.atomize.system_prompt,
                    &state.config.tasks.atomize.user_prompt,
                );
            let task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.atomize_block(
                                &context,
                                instruction.as_deref(),
                                max_tokens,
                                &prompt,
                            ),
                        )
                        .await,
                        format!(
                            "atomize request timed out after {} seconds",
                            LLM_REQUEST_TIMEOUT.as_secs()
                        ),
                    )
                },
                move |r| {
                    Message::Patch(PatchMessage::Done {
                        kind: PatchKind::Atomize,
                        block_id,
                        request_signature,
                        result: PatchDoneResult::Atomize(r),
                    })
                },
            );
            let (task, handle) = Task::abortable(task);
            state.llm_requests.replace_atomize_handle(block_id, handle);
            task
        }
        PatchKind::Reduce => {
            let children_snapshot = state.store.children(&block_id).to_vec();
            let max_tokens = state.config.tasks.reduce.token_limit.as_api_param();
            let prompt =
                llm::TaskPromptConfig::reduce(
                    &state.config.tasks.reduce.system_prompt,
                    &state.config.tasks.reduce.user_prompt,
                );
            let task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.reduce_block(
                                &context,
                                instruction.as_deref(),
                                max_tokens,
                                &prompt,
                            ),
                        )
                        .await,
                        "reduce request timed out after 30 seconds",
                    )
                },
                move |r| {
                    Message::Patch(PatchMessage::Done {
                        kind: PatchKind::Reduce,
                        block_id,
                        request_signature,
                        result: PatchDoneResult::Reduce(r, children_snapshot),
                    })
                },
            );
            let (task, handle) = Task::abortable(task);
            state.llm_requests.replace_reduce_handle(block_id, handle);
            task
        }
    };

    request_task
}

fn handle_cancel(state: &mut AppState, kind: PatchKind, block_id: BlockId) -> Task<Message> {
    let cancelled = match kind {
        PatchKind::Expand => state.llm_requests.cancel_expand(block_id),
        PatchKind::Atomize => state.llm_requests.cancel_atomize(block_id),
        PatchKind::Reduce => state.llm_requests.cancel_reduce(block_id),
    };
    if cancelled {
        let name = match kind {
            PatchKind::Expand => "expand",
            PatchKind::Atomize => "atomize",
            PatchKind::Reduce => "reduce",
        };
        tracing::info!(block_id = ?block_id, "{} request cancelled", name);
    }
    Task::none()
}

fn handle_done(
    state: &mut AppState,
    kind: PatchKind,
    block_id: BlockId,
    request_signature: RequestSignature,
    result: PatchDoneResult,
) -> Task<Message> {
    let pending_signature = match kind {
        PatchKind::Expand => state.llm_requests.finish_expand_request(block_id),
        PatchKind::Atomize => state.llm_requests.finish_atomize_request(block_id),
        PatchKind::Reduce => state.llm_requests.finish_reduce_request(block_id),
    };
    if state.store.node(&block_id).is_none() {
        return Task::none();
    }
    let should_discard = pending_signature != Some(request_signature)
        || (matches!(kind, PatchKind::Expand | PatchKind::Reduce)
            && state.is_stale_response(&block_id, request_signature));
    if should_discard {
        tracing::info!(block_id = ?block_id, "discarded stale response");
        return Task::none();
    }

    match (kind, result) {
        (PatchKind::Expand, PatchDoneResult::Expand(Ok(raw))) => {
            let (rewrite, children) = raw.into_parts();
            let rewrite =
                rewrite.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            let children = children
                .into_iter()
                .map(llm::ExpandSuggestion::into_point)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>();
            tracing::info!(block_id = ?block_id, has_rewrite = rewrite.is_some(), child_count = children.len(), "expand succeeded");
            if rewrite.is_none() && children.is_empty() {
                let reason = UiError::from_message("expand returned no usable suggestions");
                state.llm_requests.set_expand_error(block_id, reason.clone());
                state.record_error(AppError::Expand(reason));
                return Task::none();
            }
            state.mutate_with_undo_and_persist("after creating expansion draft", |s| {
                s.store.insert_expansion_draft(
                    block_id,
                    ExpansionDraftRecord { rewrite, children },
                );
                s.errors.retain(|e| !matches!(e, AppError::Expand(_)));
                true
            });
        }
        (PatchKind::Expand, PatchDoneResult::Expand(Err(reason))) => {
            state.llm_requests.set_expand_error(block_id, reason.clone());
            state.record_error(AppError::Expand(reason));
        }
        (PatchKind::Atomize, PatchDoneResult::Atomize(Ok(raw))) => {
            let (rewrite, points) = raw.into_parts();
            state.store.insert_atomization_draft(
                block_id,
                AtomizationDraftRecord { rewrite, points },
            );
            state.errors.retain(|e| !matches!(e, AppError::Atomize(_)));
            tracing::info!(block_id = ?block_id, "atomize done");
        }
        (PatchKind::Atomize, PatchDoneResult::Atomize(Err(reason))) => {
            state.record_error(AppError::Atomize(reason));
        }
        (PatchKind::Reduce, PatchDoneResult::Reduce(Ok(raw), ref children_snapshot)) => {
            let (reduction, indices) = raw.into_parts();
            let redundant: Vec<BlockId> =
                indices.iter().filter_map(|&i| children_snapshot.get(i).copied()).collect();
            tracing::info!(block_id = ?block_id, chars = reduction.len(), redundant = redundant.len(), "reduce succeeded");
            state.mutate_with_undo_and_persist("after creating reduction draft", |s| {
                s.store.insert_reduction_draft(
                    block_id,
                    ReductionDraftRecord { reduction, redundant_children: redundant },
                );
                s.errors.retain(|e| !matches!(e, AppError::Reduce(_)));
                true
            });
        }
        (PatchKind::Reduce, PatchDoneResult::Reduce(Err(reason), _)) => {
            state.llm_requests.set_reduce_error(block_id, reason.clone());
            state.record_error(AppError::Reduce(reason));
        }
        _ => {}
    }
    state.store.remove_instruction_draft(&block_id);
    Task::none()
}

fn handle_apply_rewrite(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    let rewrite_opt = state
        .store
        .expansion_draft_mut(&block_id)
        .and_then(|d| d.rewrite.take())
        .or_else(|| {
            state
                .store
                .atomization_draft_mut(&block_id)
                .and_then(|d| d.rewrite.take())
        });
    if let Some(rewrite) = rewrite_opt {
        state.mutate_with_undo_and_persist("after applying rewrite", |s| {
            s.store.update_point(&block_id, rewrite.clone());
            s.editor_buffers.set_text(&block_id, &rewrite);
            if let Some(d) = s.store.expansion_draft(&block_id) {
                if d.rewrite.is_none() && d.children.is_empty() {
                    s.store.remove_expansion_draft(&block_id);
                }
            }
            if let Some(d) = s.store.atomization_draft(&block_id) {
                if d.points.is_empty() {
                    s.store.remove_atomization_draft(&block_id);
                }
            }
            true
        });
    }
    Task::none()
}

fn handle_reject_rewrite(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    let mut changed = false;
    if let Some(d) = state.store.expansion_draft_mut(&block_id) {
        d.rewrite = None;
        let empty = d.rewrite.is_none() && d.children.is_empty();
        if empty {
            state.store.remove_expansion_draft(&block_id);
        }
        changed = true;
    }
    if let Some(d) = state.store.atomization_draft_mut(&block_id) {
        d.rewrite = None;
        if d.points.is_empty() {
            state.store.remove_atomization_draft(&block_id);
        }
        changed = true;
    }
    if changed {
        state.persist_with_context("after rejecting rewrite");
    }
    Task::none()
}

fn handle_accept_child(
    state: &mut AppState,
    block_id: BlockId,
    child_index: usize,
) -> Task<Message> {
    let point_opt = state
        .store
        .expansion_draft_mut(&block_id)
        .and_then(|d| {
            if child_index < d.children.len() {
                Some(d.children.remove(child_index))
            } else {
                None
            }
        })
        .or_else(|| {
            state.store.atomization_draft_mut(&block_id).and_then(|d| {
                if child_index < d.points.len() {
                    Some(d.points.remove(child_index))
                } else {
                    None
                }
            })
        });
    if let Some(point) = point_opt {
        state.mutate_with_undo_and_persist("after accepting patch child", |s| {
            let mut save = false;
            if let Some(child_id) = s.store.append_child(&block_id, point.clone()) {
                s.editor_buffers.set_text(&child_id, &point);
                save = true;
            }
            if let Some(d) = s.store.expansion_draft(&block_id) {
                if d.rewrite.is_none() && d.children.is_empty() {
                    s.store.remove_expansion_draft(&block_id);
                }
            }
            if let Some(d) = s.store.atomization_draft(&block_id) {
                if d.points.is_empty() && d.rewrite.is_none() {
                    s.store.remove_atomization_draft(&block_id);
                }
            }
            save
        });
    }
    Task::none()
}

fn handle_reject_child(
    state: &mut AppState,
    block_id: BlockId,
    child_index: usize,
) -> Task<Message> {
    let mut changed = false;
    if let Some(d) = state.store.expansion_draft_mut(&block_id) {
        if child_index < d.children.len() {
            d.children.remove(child_index);
            changed = true;
        }
        if d.rewrite.is_none() && d.children.is_empty() {
            state.store.remove_expansion_draft(&block_id);
        }
    }
    if let Some(d) = state.store.atomization_draft_mut(&block_id) {
        if child_index < d.points.len() {
            d.points.remove(child_index);
            changed = true;
        }
        if d.points.is_empty() && d.rewrite.is_none() {
            state.store.remove_atomization_draft(&block_id);
        }
    }
    if changed {
        state.persist_with_context("after rejecting patch child");
    }
    Task::none()
}

fn handle_accept_all_children(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    state.mutate_with_undo_and_persist("after accepting all patch children", |s| {
        let mut did_work = false;
        if let Some(mut draft) = s.store.remove_expansion_draft(&block_id) {
            for point in draft.children.drain(..) {
                if let Some(cid) = s.store.append_child(&block_id, point.clone()) {
                    s.editor_buffers.set_text(&cid, &point);
                    did_work = true;
                }
            }
            if draft.rewrite.is_some() {
                s.store.insert_expansion_draft(block_id, draft);
            }
            did_work = true;
        }
        if let Some(draft) = s.store.remove_atomization_draft(&block_id) {
            if let Some(r) = draft.rewrite {
                s.store.update_point(&block_id, r.clone());
                s.editor_buffers.set_text(&block_id, &r);
            }
            for point in draft.points {
                if let Some(cid) = s.store.append_child(&block_id, point.clone()) {
                    s.editor_buffers.set_text(&cid, &point);
                    did_work = true;
                }
            }
            did_work = true;
        }
        did_work
    });
    Task::none()
}

fn handle_discard_all_children(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    let mut changed = false;
    if let Some(d) = state.store.expansion_draft_mut(&block_id) {
        if !d.children.is_empty() {
            d.children.clear();
            changed = true;
        }
        if d.rewrite.is_none() && d.children.is_empty() {
            state.store.remove_expansion_draft(&block_id);
        }
    }
    if state.store.atomization_draft(&block_id).is_some() {
        state.store.remove_atomization_draft(&block_id);
        changed = true;
    }
    if changed {
        state.persist_with_context("after discarding patch children");
    }
    Task::none()
}

fn handle_apply_reduction(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    state.mutate_with_undo_and_persist("after applying reduction", |s| {
        if let Some(draft) = s.store.remove_reduction_draft(&block_id) {
            s.store.update_point(&block_id, draft.reduction.clone());
            s.editor_buffers.set_text(&block_id, &draft.reduction);
            for cid in &draft.redundant_children {
                if s.store.node(cid).is_some() {
                    if let Some(removed) = s.store.remove_block_subtree(cid) {
                        s.editor_buffers.remove_blocks(&removed);
                        for id in &removed {
                            s.llm_requests.remove_block(*id);
                        }
                    }
                }
            }
            return true;
        }
        false
    });
    Task::none()
}

fn handle_reject_reduction(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    state.store.remove_reduction_draft(&block_id);
    state.persist_with_context("after rejecting reduction");
    Task::none()
}

fn handle_accept_child_deletion(
    state: &mut AppState,
    block_id: BlockId,
    child_index: usize,
) -> Task<Message> {
    state.mutate_with_undo_and_persist("after accepting child deletion", |s| {
        let cid = s
            .store
            .reduction_draft(&block_id)
            .and_then(|d| d.redundant_children.get(child_index).copied())
            .filter(|id| s.store.node(id).is_some());
        if let Some(cid) = cid {
            if let Some(removed) = s.store.remove_block_subtree(&cid) {
                s.editor_buffers.remove_blocks(&removed);
                for id in &removed {
                    s.llm_requests.remove_block(*id);
                }
            }
        }
        if let Some(draft) = s.store.reduction_draft(&block_id) {
            let mut updated = draft.clone();
            if child_index < updated.redundant_children.len() {
                updated.redundant_children.remove(child_index);
                s.store.insert_reduction_draft(block_id, updated);
            }
        }
        true
    });
    Task::none()
}

fn handle_reject_child_deletion(
    state: &mut AppState,
    block_id: BlockId,
    child_index: usize,
) -> Task<Message> {
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

fn handle_accept_all_deletions(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    state.mutate_with_undo_and_persist("after accepting all deletions", |s| {
        if let Some(draft) = s.store.reduction_draft(&block_id).cloned() {
            for cid in &draft.redundant_children {
                if s.store.node(cid).is_some() {
                    if let Some(removed) = s.store.remove_block_subtree(cid) {
                        s.editor_buffers.remove_blocks(&removed);
                        for id in &removed {
                            s.llm_requests.remove_block(*id);
                        }
                    }
                }
            }
            s.store.insert_reduction_draft(
                block_id,
                ReductionDraftRecord { reduction: draft.reduction, redundant_children: vec![] },
            );
            return true;
        }
        false
    });
    Task::none()
}

fn handle_reject_all_deletions(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    if let Some(draft) = state.store.reduction_draft(&block_id) {
        state.store.insert_reduction_draft(
            block_id,
            ReductionDraftRecord {
                reduction: draft.reduction.clone(),
                redundant_children: vec![],
            },
        );
    }
    state.persist_with_context("after rejecting all deletions");
    Task::none()
}

// --- Rendering ---

/// Draft content for rendering; identifies kind and borrows the record.
pub enum PatchDraft<'a> {
    Expand(&'a ExpansionDraftRecord),
    Atomize(&'a AtomizationDraftRecord),
    Reduction(&'a ReductionDraftRecord),
}

/// Render a single patch panel based on draft kind and content.
pub fn render_patch_panel<'a>(
    state: &'a AppState,
    block_id: &BlockId,
    draft: PatchDraft<'a>,
) -> Element<'a, Message> {
    let (kind, labels) = match &draft {
        PatchDraft::Expand(_) => (PatchKind::Expand, labels_for(PatchKind::Expand)),
        PatchDraft::Atomize(_) => (PatchKind::Atomize, labels_for(PatchKind::Atomize)),
        PatchDraft::Reduction(_) => (PatchKind::Reduce, labels_for(PatchKind::Reduce)),
    };

    match (draft, &labels.mode) {
        (PatchDraft::Expand(d), PatchPanelMode::AddChildren) => {
            render_add_children_panel(
                state,
                block_id,
                kind,
                &labels,
                d.rewrite.as_deref(),
                &d.children,
            )
        }
        (PatchDraft::Atomize(d), PatchPanelMode::AddChildren) => {
            render_add_children_panel(
                state,
                block_id,
                kind,
                &labels,
                d.rewrite.as_deref(),
                &d.points,
            )
        }
        (PatchDraft::Reduction(d), PatchPanelMode::ReplaceWithDeletions) => {
            render_reduction_panel(state, block_id, &labels, d)
        }
        _ => unreachable!(),
    }
}

fn render_add_children_panel<'a>(
    state: &'a AppState,
    block_id: &BlockId,
    _kind: PatchKind,
    labels: &PatchLabels,
    rewrite: Option<&str>,
    children: &'a [String],
) -> Element<'a, Message> {
    let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

    if let Some(rw) = rewrite {
        let old = state.store.point(block_id).unwrap_or_default();
        let diff = render_diff_content(state.is_dark_mode(), &old, rw);
        panel = panel.push(
            column![]
                .spacing(theme::PANEL_INNER_GAP)
                .push(container(text(labels.section_title.clone())).width(Length::Fill))
                .push(container(diff).width(Length::Fill))
                .push(
                    row![]
                        .width(Length::Fill)
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(space::horizontal())
                        .push(
                            TextButton::action(labels.apply_primary.clone(), 13.0)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Patch(PatchMessage::ApplyRewrite(*block_id))),
                        )
                        .push(
                            TextButton::destructive(labels.dismiss_primary.clone(), 13.0)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Patch(PatchMessage::RejectRewrite(*block_id))),
                        ),
                ),
        );
    }

    if !children.is_empty() {
        panel = panel.push(
            row![]
                .spacing(theme::PANEL_BUTTON_GAP)
                .push(container(text(labels.children_header.clone())).width(Length::Fill))
                .push(
                    TextButton::action(labels.bulk_primary.clone(), 13.0)
                        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                        .on_press(Message::Patch(PatchMessage::AcceptAllChildren(*block_id))),
                )
                .push(
                    TextButton::destructive(labels.bulk_secondary.clone(), 13.0)
                        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                        .on_press(Message::Patch(PatchMessage::DiscardAllChildren(*block_id))),
                ),
        );
        for (i, item) in children.iter().enumerate() {
            let idx = i;
            panel = panel.push(
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(container(text(item.as_str())).width(Length::Fill))
                    .push(
                        TextButton::action(labels.per_item_primary.clone(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Patch(PatchMessage::AcceptChild {
                                block_id: *block_id,
                                child_index: idx,
                            })),
                    )
                    .push(
                        TextButton::destructive(labels.per_item_secondary.clone(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Patch(PatchMessage::RejectChild {
                                block_id: *block_id,
                                child_index: idx,
                            })),
                    ),
            );
        }
    }

    container(panel)
        .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}

fn render_reduction_panel<'a>(
    state: &'a AppState,
    block_id: &BlockId,
    labels: &PatchLabels,
    draft: &'a ReductionDraftRecord,
) -> Element<'a, Message> {
    let old = state.store.point(block_id).unwrap_or_default();
    let diff = render_diff_content(state.is_dark_mode(), &old, &draft.reduction);

    let mut panel = column![]
        .spacing(theme::PANEL_INNER_GAP)
        .push(container(text(labels.section_title.clone())).width(Length::Fill))
        .push(container(diff).width(Length::Fill))
        .push(
            row![]
                .width(Length::Fill)
                .spacing(theme::PANEL_BUTTON_GAP)
                .push(space::horizontal())
                .push(
                    TextButton::action(labels.apply_primary.clone(), 13.0)
                        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                        .on_press(Message::Patch(PatchMessage::Apply(*block_id))),
                )
                .push(
                    TextButton::destructive(labels.dismiss_primary.clone(), 13.0)
                        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                        .on_press(Message::Patch(PatchMessage::Reject(*block_id))),
                ),
        );

    let valid: Vec<(usize, String)> = draft
        .redundant_children
        .iter()
        .enumerate()
        .filter(|(_, id)| state.store.node(id).is_some())
        .map(|(idx, id)| (idx, state.store.point(id).unwrap_or_default()))
        .collect();

    if !valid.is_empty() {
        panel = panel.push(
            row![]
                .spacing(theme::PANEL_BUTTON_GAP)
                .push(container(text(labels.children_header.clone())).width(Length::Fill))
                .push(
                    TextButton::destructive(labels.bulk_primary.clone(), 13.0)
                        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                        .on_press(Message::Patch(PatchMessage::AcceptAllDeletions(*block_id))),
                )
                .push(
                    TextButton::action(labels.bulk_secondary.clone(), 13.0)
                        .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                        .on_press(Message::Patch(PatchMessage::RejectAllDeletions(*block_id))),
                ),
        );
        for (index, label) in &valid {
            let idx = *index;
            panel = panel.push(
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(container(text(label.clone())).width(Length::Fill))
                    .push(
                        TextButton::destructive(labels.per_item_primary.clone(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Patch(PatchMessage::AcceptChildDeletion {
                                block_id: *block_id,
                                child_index: idx,
                            })),
                    )
                    .push(
                        TextButton::action(labels.per_item_secondary.clone(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Patch(PatchMessage::RejectChildDeletion {
                                block_id: *block_id,
                                child_index: idx,
                            })),
                    ),
            );
        }
    }

    container(panel)
        .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}

fn render_diff_content(
    is_dark: bool,
    old_text: &str,
    new_text: &str,
) -> Element<'static, Message> {
    use iced::widget::text::Span as RichSpan;

    let changes = word_diff(old_text, new_text);
    let pal = theme::palette_for_mode(is_dark);
    let del_bg = Color { a: 0.08, ..pal.danger };
    let add_bg = Color { a: 0.08, ..pal.success };
    let ctx = pal.ink;

    let old_spans: Vec<RichSpan<'_>> = changes
        .iter()
        .filter_map(|c| match c {
            WordChange::Unchanged(s) => Some(span(s.clone()).color(ctx)),
            WordChange::Deleted(s) => Some(
                span(s.clone())
                    .color(ctx)
                    .background(del_bg)
                    .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
            ),
            WordChange::Added(_) => None,
        })
        .collect();
    let new_spans: Vec<RichSpan<'_>> = changes
        .iter()
        .filter_map(|c| match c {
            WordChange::Unchanged(s) => Some(span(s.clone()).color(ctx)),
            WordChange::Added(s) => Some(
                span(s.clone())
                    .color(ctx)
                    .background(add_bg)
                    .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
            ),
            WordChange::Deleted(_) => None,
        })
        .collect();

    container(
        column![
            rich_text(old_spans).width(Length::Fill),
            rich_text(new_spans).width(Length::Fill),
        ]
        .spacing(theme::DIFF_LINE_GAP),
    )
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> (AppState, BlockId) {
        AppState::test_state()
    }

    #[test]
    fn expand_done_success_persists_draft() {
        let (mut state, root) = test_state();
        let sig = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, sig);
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::Done {
                kind: PatchKind::Expand,
                block_id: root,
                request_signature: sig,
                result: PatchDoneResult::Expand(Ok(llm::ExpandResult::new(
                    Some("rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("child".to_string())],
                ))),
            }),
        );
        let draft = state.store.expansion_draft(&root).expect("draft created");
        assert_eq!(draft.rewrite.as_deref(), Some("rewrite"));
        assert_eq!(draft.children, vec!["child".to_string()]);
    }

    #[test]
    fn expand_done_stale_response_ignored() {
        let (mut state, root) = test_state();
        let sig = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, sig);
        state.store.update_point(&root, "edited".to_string());
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::Done {
                kind: PatchKind::Expand,
                block_id: root,
                request_signature: sig,
                result: PatchDoneResult::Expand(Ok(llm::ExpandResult::new(
                    Some("stale".to_string()),
                    vec![llm::ExpandSuggestion::new("x".to_string())],
                ))),
            }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn cancel_expand_clears_loading() {
        let (mut state, root) = test_state();
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::Start {
                kind: PatchKind::Expand,
                block_id: root,
            }),
        );
        assert!(state.llm_requests.is_expanding(root));
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::Cancel {
                kind: PatchKind::Expand,
                block_id: root,
            }),
        );
        assert!(!state.llm_requests.is_expanding(root));
    }

    #[test]
    fn apply_rewrite_updates_point() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: Some("new".to_string()), children: vec![] },
        );
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::ApplyRewrite(root)),
        );
        assert_eq!(state.store.point(&root).as_deref(), Some("new"));
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn accept_child_appends_and_updates_draft() {
        let (mut state, root) = test_state();
        let n = state.store.children(&root).len();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: None,
                children: vec!["a".to_string(), "b".to_string()],
            },
        );
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::AcceptChild { block_id: root, child_index: 0 }),
        );
        assert_eq!(state.store.children(&root).len(), n + 1);
        assert_eq!(state.store.point(&state.store.children(&root)[n]).as_deref(), Some("a"));
        let d = state.store.expansion_draft(&root).expect("draft remains");
        assert_eq!(d.children, vec!["b".to_string()]);
    }

    #[test]
    fn reduce_done_success_persists_draft() {
        let (mut state, root) = test_state();
        let sig = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_reduce_loading(root, sig);
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::Done {
                kind: PatchKind::Reduce,
                block_id: root,
                request_signature: sig,
                result: PatchDoneResult::Reduce(
                    Ok(llm::ReduceResult::new("reduced".to_string(), vec![])),
                    vec![],
                ),
            }),
        );
        let draft = state.store.reduction_draft(&root).expect("draft created");
        assert_eq!(draft.reduction, "reduced");
    }

    #[test]
    fn apply_reduction_updates_point() {
        let (mut state, root) = test_state();
        state.store.insert_reduction_draft(
            root,
            ReductionDraftRecord { reduction: "condensed".to_string(), redundant_children: vec![] },
        );
        let _ = AppState::update(&mut state, Message::Patch(PatchMessage::Apply(root)));
        assert_eq!(state.store.point(&root).as_deref(), Some("condensed"));
        assert!(state.store.reduction_draft(&root).is_none());
    }
}
