//! LLM-powered patch workflows: expand, atomize, and reduce.
//!
//! A single patch view with different UI texts. All three share the same
//! lifecycle: Start (abortable LLM request) → Done (stale-check, stage draft) →
//! apply/reject and child suggestions.

use super::error::{AppError, UiError};
use super::llm_requests::RequestSignature;
use super::{AppState, LLM_REQUEST_TIMEOUT, Message};
use crate::component::patch_panel::{
    ChildItem, ChildrenSection, PanelButton, PanelButtonStyle, RewriteSection,
};
use crate::llm;
use crate::store::{AtomizationDraftRecord, BlockId, ExpansionDraftRecord, ReductionDraftRecord};
use iced::{Element, Task};
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
    Start {
        kind: PatchKind,
        block_id: BlockId,
    },
    Cancel {
        kind: PatchKind,
        block_id: BlockId,
    },
    Done {
        kind: PatchKind,
        block_id: BlockId,
        request_signature: RequestSignature,
        result: PatchDoneResult,
    },
    /// Apply optional rewrite (expand, atomize) or required replacement (reduce).
    ApplyRewrite(BlockId),
    RejectRewrite(BlockId),
    /// Accept suggested child (expand/atomize: add; reduce: delete). Reject inverts.
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

#[derive(Debug, Clone)]
pub enum PatchDoneResult {
    Expand(Result<llm::ExpandResult, UiError>),
    Atomize(Result<llm::AtomizeResult, UiError>),
    Reduce(Result<llm::ReduceResult, UiError>, Vec<BlockId>),
}

/// Process one patch message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: PatchMessage) -> Task<Message> {
    match message {
        | PatchMessage::Start { kind, block_id } => handle_start(state, kind, block_id),
        | PatchMessage::Cancel { kind, block_id } => handle_cancel(state, kind, block_id),
        | PatchMessage::Done { kind, block_id, request_signature, result } => {
            handle_done(state, kind, block_id, request_signature, result)
        }
        | PatchMessage::ApplyRewrite(block_id) => handle_apply_rewrite(state, block_id),
        | PatchMessage::RejectRewrite(block_id) => handle_reject_rewrite(state, block_id),
        | PatchMessage::AcceptChild { block_id, child_index } => {
            handle_accept_child(state, block_id, child_index)
        }
        | PatchMessage::RejectChild { block_id, child_index } => {
            handle_reject_child(state, block_id, child_index)
        }
        | PatchMessage::AcceptAllChildren(block_id) => handle_accept_all_children(state, block_id),
        | PatchMessage::DiscardAllChildren(block_id) => {
            handle_discard_all_children(state, block_id)
        }
    }
}

fn handle_start(state: &mut AppState, kind: PatchKind, block_id: BlockId) -> Task<Message> {
    state.set_overflow_open(false);
    let is_busy = match kind {
        | PatchKind::Expand => state.llm_requests.is_expanding(block_id),
        | PatchKind::Atomize => state.llm_requests.is_atomizing(block_id),
        | PatchKind::Reduce => state.llm_requests.is_reducing(block_id),
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
        | PatchKind::Expand => {
            let Some(config) = state.llm_config_for_expand(block_id) else { return Task::none() };
            let Some(sig) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            state.llm_requests.mark_expand_loading(block_id, sig);
            (config, sig)
        }
        | PatchKind::Atomize => {
            let Some(config) = state.llm_config_for_atomize(block_id) else { return Task::none() };
            let Some(sig) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            state.llm_requests.mark_atomize_loading(block_id, sig);
            (config, sig)
        }
        | PatchKind::Reduce => {
            let Some(config) = state.llm_config_for_reduce(block_id) else { return Task::none() };
            let Some(sig) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            state.llm_requests.mark_reduce_loading(block_id, sig);
            (config, sig)
        }
    };

    let kind_name = match kind {
        | PatchKind::Expand => "expand",
        | PatchKind::Atomize => "atomize",
        | PatchKind::Reduce => "reduce",
    };
    tracing::info!(block_id = ?block_id, "{} request started", kind_name);

    let instruction = state.store.remove_instruction_draft(&block_id).map(|d| d.instruction);

    let request_task = match kind {
        | PatchKind::Expand => {
            let max_tokens = state.config.tasks.expand.token_limit.as_api_param();
            let prompt = llm::TaskPromptConfig::expand(
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
        | PatchKind::Atomize => {
            let max_tokens = state.config.tasks.atomize.token_limit.as_api_param();
            let prompt = llm::TaskPromptConfig::atomize(
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
        | PatchKind::Reduce => {
            let children_snapshot = state.store.children(&block_id).to_vec();
            let max_tokens = state.config.tasks.reduce.token_limit.as_api_param();
            let prompt = llm::TaskPromptConfig::reduce(
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
        | PatchKind::Expand => state.llm_requests.cancel_expand(block_id),
        | PatchKind::Atomize => state.llm_requests.cancel_atomize(block_id),
        | PatchKind::Reduce => state.llm_requests.cancel_reduce(block_id),
    };
    if cancelled {
        let name = match kind {
            | PatchKind::Expand => "expand",
            | PatchKind::Atomize => "atomize",
            | PatchKind::Reduce => "reduce",
        };
        tracing::info!(block_id = ?block_id, "{} request cancelled", name);
    }
    Task::none()
}

fn handle_done(
    state: &mut AppState, kind: PatchKind, block_id: BlockId, request_signature: RequestSignature,
    result: PatchDoneResult,
) -> Task<Message> {
    let pending_signature = match kind {
        | PatchKind::Expand => state.llm_requests.finish_expand_request(block_id),
        | PatchKind::Atomize => state.llm_requests.finish_atomize_request(block_id),
        | PatchKind::Reduce => state.llm_requests.finish_reduce_request(block_id),
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
        | (PatchKind::Expand, PatchDoneResult::Expand(Ok(raw))) => {
            let (rewrite, children) = raw.into_parts();
            let rewrite = rewrite.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
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
                s.store
                    .insert_expansion_draft(block_id, ExpansionDraftRecord { rewrite, children });
                s.errors.retain(|e| !matches!(e, AppError::Expand(_)));
                true
            });
        }
        | (PatchKind::Expand, PatchDoneResult::Expand(Err(reason))) => {
            state.llm_requests.set_expand_error(block_id, reason.clone());
            state.record_error(AppError::Expand(reason));
        }
        | (PatchKind::Atomize, PatchDoneResult::Atomize(Ok(raw))) => {
            let (rewrite, points) = raw.into_parts();
            state
                .store
                .insert_atomization_draft(block_id, AtomizationDraftRecord { rewrite, points });
            state.errors.retain(|e| !matches!(e, AppError::Atomize(_)));
            tracing::info!(block_id = ?block_id, "atomize done");
        }
        | (PatchKind::Atomize, PatchDoneResult::Atomize(Err(reason))) => {
            state.record_error(AppError::Atomize(reason));
        }
        | (PatchKind::Reduce, PatchDoneResult::Reduce(Ok(raw), ref children_snapshot)) => {
            let (reduction, indices) = raw.into_parts();
            let redundant: Vec<BlockId> =
                indices.iter().filter_map(|&i| children_snapshot.get(i).copied()).collect();
            tracing::info!(block_id = ?block_id, chars = reduction.len(), redundant = redundant.len(), "reduce succeeded");
            state.mutate_with_undo_and_persist("after creating reduction draft", |s| {
                s.store.insert_reduction_draft(
                    block_id,
                    ReductionDraftRecord {
                        reduction: Some(reduction),
                        redundant_children: redundant,
                    },
                );
                s.errors.retain(|e| !matches!(e, AppError::Reduce(_)));
                true
            });
        }
        | (PatchKind::Reduce, PatchDoneResult::Reduce(Err(reason), _)) => {
            state.llm_requests.set_reduce_error(block_id, reason.clone());
            state.record_error(AppError::Reduce(reason));
        }
        | _ => {}
    }
    state.store.remove_instruction_draft(&block_id);
    Task::none()
}

fn handle_apply_rewrite(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    let rewrite_opt = state
        .store
        .expansion_draft_mut(&block_id)
        .and_then(|d| d.rewrite.take())
        .or_else(|| state.store.atomization_draft_mut(&block_id).and_then(|d| d.rewrite.take()))
        .or_else(|| state.store.reduction_draft(&block_id).and_then(|d| d.reduction.clone()));
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
            if let Some(d) = s.store.reduction_draft(&block_id) {
                if d.redundant_children.is_empty() {
                    s.store.remove_reduction_draft(&block_id);
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
    let reduction_action = state
        .store
        .reduction_draft(&block_id)
        .map(|d| (d.reduction.is_some(), d.redundant_children.is_empty()));
    if let Some((had_reduction, children_empty)) = reduction_action {
        if had_reduction {
            if let Some(d) = state.store.reduction_draft_mut(&block_id) {
                d.reduction = None;
            }
            if children_empty {
                state.store.remove_reduction_draft(&block_id);
            }
        } else {
            state.store.remove_reduction_draft(&block_id);
        }
        changed = true;
    }
    if changed {
        state.persist_with_context("after rejecting rewrite");
    }
    Task::none()
}

fn handle_accept_child(
    state: &mut AppState, block_id: BlockId, child_index: usize,
) -> Task<Message> {
    let point_opt = state
        .store
        .expansion_draft_mut(&block_id)
        .and_then(|d| {
            if child_index < d.children.len() { Some(d.children.remove(child_index)) } else { None }
        })
        .or_else(|| {
            state.store.atomization_draft_mut(&block_id).and_then(|d| {
                if child_index < d.points.len() { Some(d.points.remove(child_index)) } else { None }
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
        return Task::none();
    }
    // Reduction: accept = delete child (inverse of expand).
    let cid_opt = state
        .store
        .reduction_draft(&block_id)
        .and_then(|d| d.redundant_children.get(child_index).copied());
    if let Some(cid) = cid_opt {
        state.mutate_with_undo_and_persist("after accepting child deletion", |s| {
            if s.store.node(&cid).is_some() {
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
    }
    Task::none()
}

fn handle_reject_child(
    state: &mut AppState, block_id: BlockId, child_index: usize,
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
    if let Some(draft) = state.store.reduction_draft(&block_id) {
        if child_index < draft.redundant_children.len() {
            let mut updated = draft.clone();
            updated.redundant_children.remove(child_index);
            state.store.insert_reduction_draft(block_id, updated);
            changed = true;
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
                }
            }
            did_work = true;
        }
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
    let reduction_clear_children =
        state.store.reduction_draft(&block_id).filter(|d| !d.redundant_children.is_empty()).map(
            |d| ReductionDraftRecord { reduction: d.reduction.clone(), redundant_children: vec![] },
        );
    if let Some(draft) = reduction_clear_children {
        state.store.insert_reduction_draft(block_id, draft);
        changed = true;
    }
    if changed {
        state.persist_with_context("after discarding patch children");
    }
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
    state: &'a AppState, block_id: &BlockId, draft: PatchDraft<'a>,
) -> Element<'a, Message> {
    let is_dark = state.is_dark_mode();
    match draft {
        | PatchDraft::Expand(d) => {
            let current_point = state.store.point(block_id).unwrap_or_default();
            let rewrite = d.rewrite.as_deref().map(|rw| RewriteSection::Diff {
                title: t!("doc_rewrite").to_string(),
                old_text: current_point,
                new_text: rw.to_string(),
                buttons: vec![
                    PanelButton {
                        label: t!("doc_apply_rewrite").to_string(),
                        style: PanelButtonStyle::Action,
                        on_press: Message::Patch(PatchMessage::ApplyRewrite(*block_id)),
                    },
                    PanelButton {
                        label: t!("doc_dismiss_rewrite").to_string(),
                        style: PanelButtonStyle::Destructive,
                        on_press: Message::Patch(PatchMessage::RejectRewrite(*block_id)),
                    },
                ],
            });
            let children = build_add_children_section(
                block_id,
                t!("doc_child_suggestions").to_string(),
                t!("doc_accept_all").to_string(),
                t!("doc_discard_all").to_string(),
                t!("doc_keep").to_string(),
                t!("doc_drop").to_string(),
                d.children.iter().enumerate().map(|(i, s)| (i, s.clone())).collect(),
            );
            crate::component::patch_panel::view(is_dark, rewrite, children)
        }
        | PatchDraft::Atomize(d) => {
            let current_point = state.store.point(block_id).unwrap_or_default();
            let rewrite = d.rewrite.as_deref().map(|rw| RewriteSection::Diff {
                title: t!("doc_rewrite").to_string(),
                old_text: current_point,
                new_text: rw.to_string(),
                buttons: vec![
                    PanelButton {
                        label: t!("doc_apply_rewrite").to_string(),
                        style: PanelButtonStyle::Action,
                        on_press: Message::Patch(PatchMessage::ApplyRewrite(*block_id)),
                    },
                    PanelButton {
                        label: t!("doc_dismiss_rewrite").to_string(),
                        style: PanelButtonStyle::Destructive,
                        on_press: Message::Patch(PatchMessage::RejectRewrite(*block_id)),
                    },
                ],
            });
            let children = build_add_children_section(
                block_id,
                t!("doc_atomize_points").to_string(),
                t!("doc_accept_all").to_string(),
                t!("doc_discard_all").to_string(),
                t!("doc_keep").to_string(),
                t!("doc_drop").to_string(),
                d.points.iter().enumerate().map(|(i, s)| (i, s.clone())).collect(),
            );
            crate::component::patch_panel::view(is_dark, rewrite, children)
        }
        | PatchDraft::Reduction(d) => {
            let current_point = state.store.point(block_id).unwrap_or_default();
            let point_applied = d.reduction.as_ref().map_or(false, |r| current_point == *r);
            let rewrite = if point_applied {
                None
            } else if let Some(ref r) = d.reduction {
                Some(RewriteSection::Diff {
                    title: t!("doc_reduce").to_string(),
                    old_text: current_point,
                    new_text: r.to_string(),
                    buttons: vec![
                        PanelButton {
                            label: t!("doc_apply_reduction").to_string(),
                            style: PanelButtonStyle::Action,
                            on_press: Message::Patch(PatchMessage::ApplyRewrite(*block_id)),
                        },
                        PanelButton {
                            label: t!("doc_dismiss_reduction").to_string(),
                            style: PanelButtonStyle::Destructive,
                            on_press: Message::Patch(PatchMessage::RejectRewrite(*block_id)),
                        },
                    ],
                })
            } else {
                None
            };
            // Delete-children section: primary = delete (destructive), secondary = keep (action).
            let child_items: Vec<(usize, String)> = d
                .redundant_children
                .iter()
                .enumerate()
                .filter(|(_, id)| state.store.node(id).is_some())
                .map(|(idx, id)| (idx, state.store.point(id).unwrap_or_default()))
                .collect();
            let children = if child_items.is_empty() {
                None
            } else {
                Some(ChildrenSection {
                    header: t!("doc_redundant_children").to_string(),
                    bulk_primary: PanelButton {
                        label: t!("doc_delete_all").to_string(),
                        style: PanelButtonStyle::Destructive,
                        on_press: Message::Patch(PatchMessage::AcceptAllChildren(*block_id)),
                    },
                    bulk_secondary: PanelButton {
                        label: t!("doc_keep_all").to_string(),
                        style: PanelButtonStyle::Action,
                        on_press: Message::Patch(PatchMessage::DiscardAllChildren(*block_id)),
                    },
                    items: child_items
                        .into_iter()
                        .map(|(idx, point)| ChildItem {
                            text: point,
                            primary: PanelButton {
                                label: t!("doc_delete").to_string(),
                                style: PanelButtonStyle::Destructive,
                                on_press: Message::Patch(PatchMessage::AcceptChild {
                                    block_id: *block_id,
                                    child_index: idx,
                                }),
                            },
                            secondary: PanelButton {
                                label: t!("doc_keep").to_string(),
                                style: PanelButtonStyle::Action,
                                on_press: Message::Patch(PatchMessage::RejectChild {
                                    block_id: *block_id,
                                    child_index: idx,
                                }),
                            },
                        })
                        .collect(),
                })
            };
            crate::component::patch_panel::view(is_dark, rewrite, children)
        }
    }
}

/// Build a `ChildrenSection` for add-children operations (expand, atomize).
///
/// Primary = keep (action style), secondary = drop (destructive style).
fn build_add_children_section(
    block_id: &BlockId, header: String, bulk_primary_label: String, bulk_secondary_label: String,
    per_item_primary_label: String, per_item_secondary_label: String, items: Vec<(usize, String)>,
) -> Option<ChildrenSection<Message>> {
    if items.is_empty() {
        return None;
    }
    Some(ChildrenSection {
        header,
        bulk_primary: PanelButton {
            label: bulk_primary_label,
            style: PanelButtonStyle::Action,
            on_press: Message::Patch(PatchMessage::AcceptAllChildren(*block_id)),
        },
        bulk_secondary: PanelButton {
            label: bulk_secondary_label,
            style: PanelButtonStyle::Destructive,
            on_press: Message::Patch(PatchMessage::DiscardAllChildren(*block_id)),
        },
        items: items
            .into_iter()
            .map(|(idx, point)| ChildItem {
                text: point,
                primary: PanelButton {
                    label: per_item_primary_label.clone(),
                    style: PanelButtonStyle::Action,
                    on_press: Message::Patch(PatchMessage::AcceptChild {
                        block_id: *block_id,
                        child_index: idx,
                    }),
                },
                secondary: PanelButton {
                    label: per_item_secondary_label.clone(),
                    style: PanelButtonStyle::Destructive,
                    on_press: Message::Patch(PatchMessage::RejectChild {
                        block_id: *block_id,
                        child_index: idx,
                    }),
                },
            })
            .collect(),
    })
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
            Message::Patch(PatchMessage::Start { kind: PatchKind::Expand, block_id: root }),
        );
        assert!(state.llm_requests.is_expanding(root));
        let _ = AppState::update(
            &mut state,
            Message::Patch(PatchMessage::Cancel { kind: PatchKind::Expand, block_id: root }),
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
        let _ = AppState::update(&mut state, Message::Patch(PatchMessage::ApplyRewrite(root)));
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
        assert_eq!(draft.reduction.as_deref(), Some("reduced"));
    }

    #[test]
    fn apply_reduction_updates_point() {
        let (mut state, root) = test_state();
        state.store.insert_reduction_draft(
            root,
            ReductionDraftRecord {
                reduction: Some("condensed".to_string()),
                redundant_children: vec![],
            },
        );
        let _ = AppState::update(&mut state, Message::Patch(PatchMessage::ApplyRewrite(root)));
        assert_eq!(state.store.point(&root).as_deref(), Some("condensed"));
        assert!(state.store.reduction_draft(&root).is_none());
    }
}
