//! Application-local runtime state for LLM reduce/expand requests.

use super::error::UiError;
use crate::llm as llm_api;
use crate::store::BlockId;
use iced::task;
use slotmap::SparseSecondaryMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Default)]
/// Runtime state container for per-block LLM request lifecycle.
///
/// This struct owns transient request state only; persisted drafts remain in
/// `BlockStore` so request orchestration and persisted content stay decoupled.
pub(crate) struct LlmRequests {
    reduce_states: SparseSecondaryMap<BlockId, ReduceState>,
    expand_states: SparseSecondaryMap<BlockId, ExpandState>,
    reduce_handles: SparseSecondaryMap<BlockId, task::Handle>,
    expand_handles: SparseSecondaryMap<BlockId, task::Handle>,
    pending_reduce_signatures: SparseSecondaryMap<BlockId, RequestSignature>,
    pending_expand_signatures: SparseSecondaryMap<BlockId, RequestSignature>,
}

impl LlmRequests {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn clear(&mut self) {
        self.reduce_states.clear();
        self.expand_states.clear();
        self.reduce_handles.clear();
        self.expand_handles.clear();
        self.pending_reduce_signatures.clear();
        self.pending_expand_signatures.clear();
    }

    pub(crate) fn is_reducing(&self, block_id: BlockId) -> bool {
        self.reduce_states.get(block_id).is_some_and(|state| matches!(state, ReduceState::Loading))
    }

    pub(crate) fn is_expanding(&self, block_id: BlockId) -> bool {
        self.expand_states.get(block_id).is_some_and(|state| matches!(state, ExpandState::Loading))
    }

    pub(crate) fn has_reduce_error(&self, block_id: BlockId) -> bool {
        self.reduce_states
            .get(block_id)
            .is_some_and(|state| matches!(state, ReduceState::Error { .. }))
    }

    pub(crate) fn has_expand_error(&self, block_id: BlockId) -> bool {
        self.expand_states
            .get(block_id)
            .is_some_and(|state| matches!(state, ExpandState::Error { .. }))
    }

    pub(crate) fn mark_reduce_loading(
        &mut self, block_id: BlockId, request_signature: RequestSignature,
    ) {
        self.reduce_states.insert(block_id, ReduceState::Loading);
        self.pending_reduce_signatures.insert(block_id, request_signature);
    }

    pub(crate) fn mark_expand_loading(
        &mut self, block_id: BlockId, request_signature: RequestSignature,
    ) {
        self.expand_states.insert(block_id, ExpandState::Loading);
        self.pending_expand_signatures.insert(block_id, request_signature);
    }

    pub(crate) fn replace_reduce_handle(&mut self, block_id: BlockId, handle: task::Handle) {
        if let Some(previous) = self.reduce_handles.remove(block_id) {
            previous.abort();
        }
        self.reduce_handles.insert(block_id, handle.abort_on_drop());
    }

    pub(crate) fn replace_expand_handle(&mut self, block_id: BlockId, handle: task::Handle) {
        if let Some(previous) = self.expand_handles.remove(block_id) {
            previous.abort();
        }
        self.expand_handles.insert(block_id, handle.abort_on_drop());
    }

    /// Finalize a reduce request and return its captured lineage signature.
    ///
    /// Finalization is atomic per block: loading marker, handle, and pending
    /// signature are cleared together.
    pub(crate) fn finish_reduce_request(&mut self, block_id: BlockId) -> Option<RequestSignature> {
        self.reduce_handles.remove(block_id);
        self.reduce_states.remove(block_id);
        self.pending_reduce_signatures.remove(block_id)
    }

    /// Finalize an expand request and return its captured lineage signature.
    ///
    /// Finalization is atomic per block: loading marker, handle, and pending
    /// signature are cleared together.
    pub(crate) fn finish_expand_request(&mut self, block_id: BlockId) -> Option<RequestSignature> {
        self.expand_handles.remove(block_id);
        self.expand_states.remove(block_id);
        self.pending_expand_signatures.remove(block_id)
    }

    pub(crate) fn set_reduce_error(&mut self, block_id: BlockId, reason: UiError) {
        self.reduce_states.insert(block_id, ReduceState::Error { reason });
    }

    pub(crate) fn set_expand_error(&mut self, block_id: BlockId, reason: UiError) {
        self.expand_states.insert(block_id, ExpandState::Error { reason });
    }

    pub(crate) fn cancel_reduce(&mut self, block_id: BlockId) -> bool {
        if !self.is_reducing(block_id) {
            return false;
        }
        if let Some(handle) = self.reduce_handles.remove(block_id) {
            handle.abort();
        }
        self.reduce_states.remove(block_id);
        self.pending_reduce_signatures.remove(block_id);
        true
    }

    pub(crate) fn cancel_expand(&mut self, block_id: BlockId) -> bool {
        if !self.is_expanding(block_id) {
            return false;
        }
        if let Some(handle) = self.expand_handles.remove(block_id) {
            handle.abort();
        }
        self.expand_states.remove(block_id);
        self.pending_expand_signatures.remove(block_id);
        true
    }

    pub(crate) fn remove_block(&mut self, block_id: BlockId) {
        if let Some(handle) = self.reduce_handles.remove(block_id) {
            handle.abort();
        }
        if let Some(handle) = self.expand_handles.remove(block_id) {
            handle.abort();
        }
        self.pending_reduce_signatures.remove(block_id);
        self.pending_expand_signatures.remove(block_id);
        self.reduce_states.remove(block_id);
        self.expand_states.remove(block_id);
    }

    #[cfg(test)]
    pub(crate) fn has_pending_reduce_signature(&self, block_id: BlockId) -> bool {
        self.pending_reduce_signatures.get(block_id).is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_pending_expand_signature(&self, block_id: BlockId) -> bool {
        self.pending_expand_signatures.get(block_id).is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_reduce_handle(&self, block_id: BlockId) -> bool {
        self.reduce_handles.get(block_id).is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_expand_handle(&self, block_id: BlockId) -> bool {
        self.expand_handles.get(block_id).is_some()
    }
}

/// Per-block reduce operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ReduceState {
    #[default]
    Idle,
    Loading,
    Error {
        reason: UiError,
    },
}

/// Per-block expand operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ExpandState {
    #[default]
    Idle,
    Loading,
    Error {
        reason: UiError,
    },
}

/// Captured request-context fingerprint for async expand/reduce.
///
/// Built from full lineage (root-to-target points). Responses are applied only
/// when the current lineage fingerprint matches this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestSignature {
    hash: u64,
    item_count: usize,
}

impl RequestSignature {
    #[cfg(test)]
    pub(crate) fn from_lineage(lineage: &llm_api::Lineage) -> Option<Self> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut item_count = 0usize;
        for point in lineage.points() {
            Self::text_signature(point).hash(&mut hasher);
            item_count += 1;
        }
        if item_count == 0 {
            return None;
        }
        Some(Self { hash: hasher.finish(), item_count })
    }

    /// Build a request signature from full block context.
    ///
    /// This includes both lineage points and existing children points so async
    /// expand/reduce responses are invalidated when either input changes.
    pub(crate) fn from_block_context(context: &llm_api::BlockContext) -> Option<Self> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut item_count = 0usize;
        for point in context.lineage().points() {
            Self::text_signature(point).hash(&mut hasher);
            item_count += 1;
        }
        for child_point in context.existing_children() {
            Self::text_signature(child_point).hash(&mut hasher);
            item_count += 1;
        }
        if item_count == 0 {
            return None;
        }
        Some(Self { hash: hasher.finish(), item_count })
    }

    fn text_signature(text: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_state_default_is_idle() {
        assert_eq!(ReduceState::default(), ReduceState::Idle);
    }

    #[test]
    fn expand_state_default_is_idle() {
        assert_eq!(ExpandState::default(), ExpandState::Idle);
    }

    #[test]
    fn request_signature_from_empty_lineage_is_none() {
        let lineage = llm_api::Lineage::from_points(vec![]);
        assert!(RequestSignature::from_lineage(&lineage).is_none());
    }

    #[test]
    fn request_signature_changes_when_lineage_changes() {
        let first = llm_api::Lineage::from_points(vec!["root".to_string(), "child".to_string()]);
        let second =
            llm_api::Lineage::from_points(vec!["root changed".to_string(), "child".to_string()]);
        assert_ne!(RequestSignature::from_lineage(&first), RequestSignature::from_lineage(&second));
    }

    #[test]
    fn request_signature_from_block_context_changes_when_children_change() {
        let lineage = llm_api::Lineage::from_points(vec!["root".to_string()]);
        let ctx1 = llm_api::BlockContext::new(lineage.clone(), vec!["child_a".to_string()]);
        let ctx2 = llm_api::BlockContext::new(lineage.clone(), vec!["child_b".to_string()]);
        assert_ne!(
            RequestSignature::from_block_context(&ctx1),
            RequestSignature::from_block_context(&ctx2)
        );
    }

    #[test]
    fn request_signature_from_block_context_matches_lineage_when_no_children() {
        let lineage = llm_api::Lineage::from_points(vec!["root".to_string(), "child".to_string()]);
        let ctx = llm_api::BlockContext::new(lineage.clone(), vec![]);
        assert_eq!(
            RequestSignature::from_lineage(&lineage),
            RequestSignature::from_block_context(&ctx)
        );
    }
}
