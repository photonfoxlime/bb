//! Expand handler: LLM-powered block expansion with child suggestions and rewrites.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Expand takes a block's point text and context, sends them to the LLM, and
//! receives back an optional rewrite of the point plus a list of child
//! suggestions. The result is staged as an [`ExpansionDraftRecord`] for user
//! review before committing.
//!
//! # Message lifecycle
//!
//! 1. [`ExpandMessage::Start`] — fires the LLM request (abortable).
//! 2. [`ExpandMessage::Cancel`] — aborts an in-flight request.
//! 3. [`ExpandMessage::Done`] — response arrived; stale-check then stage draft.
//! 4. [`ExpandMessage::ApplyRewrite`] / [`ExpandMessage::RejectRewrite`] —
//!    commit or discard the suggested rewrite.
//! 5. Individual and bulk child accept/reject variants allow fine-grained
//!    control over which suggested children are kept.

use super::error::{AppError, UiError};
use super::llm_requests::RequestSignature;
use super::{AppState, LLM_REQUEST_TIMEOUT, Message};
use crate::llm;
use crate::store::{BlockId, ExpansionDraftRecord};
use iced::Task;

/// Messages for the expand workflow.
#[derive(Debug, Clone)]
pub enum ExpandMessage {
    Start(BlockId),
    Cancel(BlockId),
    Done {
        block_id: BlockId,
        request_signature: RequestSignature,
        result: Result<llm::ExpandResult, UiError>,
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

/// Process one expand message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: ExpandMessage) -> Task<Message> {
    match message {
        | ExpandMessage::Start(block_id) => {
            state.set_overflow_open(false);
            if state.llm_requests.is_expanding(block_id) {
                return Task::none();
            }
            let context = state.store.block_context_for_id(&block_id);
            let Some(config) = state.llm_config_for_expand(block_id) else {
                return Task::none();
            };

            tracing::info!(block_id = ?block_id, "expand request started");
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            state.llm_requests.mark_expand_loading(block_id, request_signature);
            // Get instruction draft from store and consume it
            let instruction =
                state.store.remove_instruction_draft(&block_id).map(|d| d.instruction);
            let expand_max_tokens = state.config.token_limits.expand.as_api_param();
            let request_task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(
                            LLM_REQUEST_TIMEOUT,
                            client.expand_block(
                                &context,
                                instruction.as_deref(),
                                expand_max_tokens,
                            ),
                        )
                        .await,
                        format!(
                            "expand request timed out after {} seconds",
                            LLM_REQUEST_TIMEOUT.as_secs()
                        ),
                    )
                },
                move |result| {
                    Message::Expand(ExpandMessage::Done { block_id, request_signature, result })
                },
            );
            let (request_task, handle) = Task::abortable(request_task);
            state.llm_requests.replace_expand_handle(block_id, handle);
            request_task
        }
        | ExpandMessage::Cancel(block_id) => {
            if state.llm_requests.cancel_expand(block_id) {
                tracing::info!(block_id = ?block_id, "expand request cancelled");
            }
            Task::none()
        }
        | ExpandMessage::Done { block_id, request_signature, result } => {
            let pending_signature = state.llm_requests.finish_expand_request(block_id);
            if state.store.node(&block_id).is_none() {
                return Task::none();
            }
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale expand response after point changed"
                );
                return Task::none();
            }
            match result {
                | Ok(raw_result) => {
                    let (rewrite, children) = raw_result.into_parts();
                    let rewrite =
                        rewrite.map(|value| value.trim().to_string()).filter(|v| !v.is_empty());
                    let children = children
                        .into_iter()
                        .map(llm::ExpandSuggestion::into_point)
                        .map(|value| value.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .collect::<Vec<_>>();
                    tracing::info!(
                        block_id = ?block_id,
                        has_rewrite = rewrite.is_some(),
                        child_count = children.len(),
                        "expand request succeeded"
                    );
                    if rewrite.is_none() && children.is_empty() {
                        let reason = UiError::from_message("expand returned no usable suggestions");
                        state.llm_requests.set_expand_error(block_id, reason.clone());
                        state.record_error(AppError::Expand(reason));
                        return Task::none();
                    }
                    state.mutate_with_undo_and_persist("after creating expansion draft", |state| {
                        state.store.insert_expansion_draft(
                            block_id,
                            ExpansionDraftRecord { rewrite, children },
                        );
                        state.errors.retain(|err| !matches!(err, AppError::Expand(_)));
                        true
                    });
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "expand request failed");
                    state.llm_requests.set_expand_error(block_id, reason.clone());
                    state.record_error(AppError::Expand(reason));
                }
            }
            // Clear instruction draft after expand completes
            state.store.remove_instruction_draft(&block_id);
            Task::none()
        }
        | ExpandMessage::ApplyRewrite(block_id) => {
            state.mutate_with_undo_and_persist("after applying rewrite", |state| {
                    let mut should_save = false;
                    let mut should_remove_draft = false;
                    let mut applied_rewrite: Option<String> = None;
                    if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                        applied_rewrite = draft.rewrite.take();
                        should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
                    }
                    if let Some(rewrite) = applied_rewrite {
                        tracing::info!(block_id = ?block_id, chars = rewrite.len(), "applied expanded rewrite");
                        state.store.update_point(&block_id, rewrite.clone());
                        state.editor_buffers.set_text(&block_id, &rewrite);
                        should_save = true;
                    }
                    if should_remove_draft {
                        state.store.remove_expansion_draft(&block_id);
                    }
                    should_save
                });
            Task::none()
        }
        | ExpandMessage::RejectRewrite(block_id) => {
            let mut changed = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                draft.rewrite = None;
                tracing::info!(block_id = ?block_id, "rejected expanded rewrite");
                should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
                changed = true;
            }
            if should_remove_draft {
                state.store.remove_expansion_draft(&block_id);
            }
            if changed {
                state.persist_with_context("after rejecting rewrite");
            }
            Task::none()
        }
        | ExpandMessage::AcceptChild { block_id, child_index } => {
            state.mutate_with_undo_and_persist("after accepting expanded child", |state| {
                let mut should_save = false;
                let mut should_remove_draft = false;
                let mut accepted_child_point: Option<String> = None;
                if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                    if child_index < draft.children.len() {
                        accepted_child_point = Some(draft.children.remove(child_index));
                    }
                    if draft.rewrite.is_none() && draft.children.is_empty() {
                        should_remove_draft = true;
                    }
                }
                if let Some(point) = accepted_child_point
                    && let Some(child_id) = state.store.append_child(&block_id, point.clone())
                {
                    tracing::info!(
                        parent_block_id = ?block_id,
                        child_block_id = ?child_id,
                        chars = point.len(),
                        "accepted expanded child"
                    );
                    state.editor_buffers.set_text(&child_id, &point);
                    should_save = true;
                }
                if should_remove_draft {
                    state.store.remove_expansion_draft(&block_id);
                }
                should_save
            });
            Task::none()
        }
        | ExpandMessage::RejectChild { block_id, child_index } => {
            let mut changed = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                if child_index < draft.children.len() {
                    draft.children.remove(child_index);
                    tracing::info!(block_id = ?block_id, child_index, "rejected expanded child");
                    changed = true;
                }
                should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
            }
            if should_remove_draft {
                state.store.remove_expansion_draft(&block_id);
            }
            if changed {
                state.persist_with_context("after rejecting expanded child");
            }
            Task::none()
        }
        | ExpandMessage::AcceptAllChildren(block_id) => {
            state.mutate_with_undo_and_persist("after accepting expanded children", |state| {
                if let Some(mut draft) = state.store.remove_expansion_draft(&block_id) {
                    for point in draft.children.drain(..) {
                        if let Some(child_id) = state.store.append_child(&block_id, point.clone()) {
                            tracing::info!(
                                parent_block_id = ?block_id,
                                child_block_id = ?child_id,
                                chars = point.len(),
                                "accepted expanded child (bulk)"
                            );
                            state.editor_buffers.set_text(&child_id, &point);
                        }
                    }
                    if draft.rewrite.is_some() {
                        state.store.insert_expansion_draft(block_id, draft);
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | ExpandMessage::DiscardAllChildren(block_id) => {
            let mut changed = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                if !draft.children.is_empty() {
                    draft.children.clear();
                    tracing::info!(block_id = ?block_id, "discarded all expanded children");
                    changed = true;
                }
                should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
            }
            if should_remove_draft {
                state.store.remove_expansion_draft(&block_id);
            }
            if changed {
                state.persist_with_context("after discarding expanded children");
            }
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> (AppState, BlockId) {
        AppState::test_state()
    }

    #[test]
    fn expand_done_success_persists_draft_in_store() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ExpandResult::new(
                    Some("rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("child".to_string())],
                )),
            }),
        );
        let draft = state.store.expansion_draft(&root).expect("draft is created");
        assert_eq!(draft.rewrite.as_deref(), Some("rewrite"));
        assert_eq!(draft.children, vec!["child".to_string()]);
    }

    #[test]
    fn expand_done_stale_response_is_ignored() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        state.store.update_point(&root, "edited while pending".to_string());
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ExpandResult::new(
                    Some("stale rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("stale child".to_string())],
                )),
            }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn cancel_expand_clears_loading_state_and_pending_signature() {
        let (mut state, root) = test_state();
        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::Start(root)));
        assert!(state.llm_requests.is_expanding(root));
        assert!(state.llm_requests.has_pending_expand_signature(root));
        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::Cancel(root)));
        assert!(!state.llm_requests.is_expanding(root));
        assert!(!state.llm_requests.has_pending_expand_signature(root));
    }

    #[test]
    fn apply_expanded_rewrite_updates_point_and_clears_empty_draft() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: Some("rewritten point".to_string()), children: vec![] },
        );
        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::ApplyRewrite(root)));
        assert_eq!(state.store.point(&root).as_deref(), Some("rewritten point"));
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn reject_expanded_rewrite_keeps_child_suggestions() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: Some("rewrite".to_string()),
                children: vec!["child a".to_string()],
            },
        );
        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::RejectRewrite(root)));
        let draft = state.store.expansion_draft(&root).expect("draft remains with children");
        assert!(draft.rewrite.is_none());
        assert_eq!(draft.children, vec!["child a".to_string()]);
    }

    #[test]
    fn accept_expanded_child_appends_child_and_updates_draft() {
        let (mut state, root) = test_state();
        let before_children_len = state.store.children(&root).len();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: None,
                children: vec!["child a".to_string(), "child b".to_string()],
            },
        );
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::AcceptChild { block_id: root, child_index: 0 }),
        );
        let children = state.store.children(&root);
        assert_eq!(children.len(), before_children_len + 1);
        let child_id = *children.last().expect("new child is appended");
        assert_eq!(state.store.point(&child_id).as_deref(), Some("child a"));
        let draft = state.store.expansion_draft(&root).expect("draft remains with one child");
        assert_eq!(draft.children, vec!["child b".to_string()]);
    }

    #[test]
    fn accept_all_expanded_children_keeps_rewrite_and_clears_children() {
        let (mut state, root) = test_state();
        let before_children_len = state.store.children(&root).len();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: Some("rewrite".to_string()),
                children: vec!["child a".to_string(), "child b".to_string()],
            },
        );
        let _ =
            AppState::update(&mut state, Message::Expand(ExpandMessage::AcceptAllChildren(root)));
        let children = state.store.children(&root);
        assert_eq!(children.len(), before_children_len + 2);
        let first = children[before_children_len];
        let second = children[before_children_len + 1];
        assert_eq!(state.store.point(&first).as_deref(), Some("child a"));
        assert_eq!(state.store.point(&second).as_deref(), Some("child b"));
        let draft = state.store.expansion_draft(&root).expect("rewrite-only draft remains");
        assert_eq!(draft.rewrite.as_deref(), Some("rewrite"));
        assert!(draft.children.is_empty());
    }

    #[test]
    fn discard_all_expanded_children_removes_empty_draft() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: None, children: vec!["child a".to_string()] },
        );
        let _ =
            AppState::update(&mut state, Message::Expand(ExpandMessage::DiscardAllChildren(root)));
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn discard_all_expanded_children_after_reexpand_preserves_rewrite() {
        let (mut state, root) = test_state();

        let first_signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, first_signature);
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: first_signature,
                result: Ok(llm::ExpandResult::new(
                    Some("first rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("first child".to_string())],
                )),
            }),
        );
        let _ =
            AppState::update(&mut state, Message::Expand(ExpandMessage::AcceptAllChildren(root)));

        let second_signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, second_signature);
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: second_signature,
                result: Ok(llm::ExpandResult::new(
                    Some("second rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("second child".to_string())],
                )),
            }),
        );

        let _ =
            AppState::update(&mut state, Message::Expand(ExpandMessage::DiscardAllChildren(root)));

        let draft = state.store.expansion_draft(&root).expect("rewrite draft remains");
        assert_eq!(draft.rewrite.as_deref(), Some("second rewrite"));
        assert!(draft.children.is_empty());
    }

    #[test]
    fn reject_expanded_child_removes_draft_when_last_child() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: None, children: vec!["only child".to_string()] },
        );
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::RejectChild { block_id: root, child_index: 0 }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn expand_done_error_sets_expand_error_state() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Err(UiError::from_message("failed")),
            }),
        );
        assert!(state.llm_requests.has_expand_error(root));
    }

    #[test]
    fn cancel_expand_then_late_response_is_ignored() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::Cancel(root)));
        let _ = AppState::update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ExpandResult::new(
                    Some("late rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("late child".to_string())],
                )),
            }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
        assert!(!state.llm_requests.is_expanding(root));
        assert!(!state.llm_requests.has_expand_error(root));
    }

    #[test]
    fn expand_handles_are_isolated_per_block_on_cancel() {
        let (mut state, root) = test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");

        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::Start(root)));
        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::Start(sibling)));

        assert!(state.llm_requests.has_expand_handle(root));
        assert!(state.llm_requests.has_expand_handle(sibling));

        let _ = AppState::update(&mut state, Message::Expand(ExpandMessage::Cancel(root)));

        assert!(!state.llm_requests.has_expand_handle(root));
        assert!(state.llm_requests.has_expand_handle(sibling));
    }
}
