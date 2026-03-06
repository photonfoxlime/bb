//! Application-local runtime state for LLM requests (amplify, distill, atomize, probe).
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.

use super::error::UiError;
use crate::llm;
use crate::store::BlockId;
use iced::task;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Default)]
/// Runtime state container for per-block LLM request lifecycle.
///
/// This struct owns transient request state only; persisted drafts remain in
/// `BlockStore` so request orchestration and persisted content stay decoupled.
pub struct LlmRequests {
    amplify_states: FxHashMap<BlockId, AmplifyState>,
    distill_states: FxHashMap<BlockId, DistillState>,
    atomize_states: FxHashMap<BlockId, AtomizeState>,
    probe_states: FxHashMap<BlockId, ProbeState>,
    amplify_handles: FxHashMap<BlockId, task::Handle>,
    distill_handles: FxHashMap<BlockId, task::Handle>,
    atomize_handles: FxHashMap<BlockId, task::Handle>,
    probe_handles: FxHashMap<BlockId, task::Handle>,
    pending_amplify_signatures: FxHashMap<BlockId, RequestSignature>,
    pending_distill_signatures: FxHashMap<BlockId, RequestSignature>,
    pending_atomize_signatures: FxHashMap<BlockId, RequestSignature>,
    pending_probe_signatures: FxHashMap<BlockId, RequestSignature>,
    probe_errors: FxHashMap<BlockId, UiError>,
}

impl LlmRequests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.amplify_states.clear();
        self.distill_states.clear();
        self.atomize_states.clear();
        self.probe_states.clear();
        self.amplify_handles.clear();
        self.distill_handles.clear();
        self.atomize_handles.clear();
        self.probe_handles.clear();
        self.pending_amplify_signatures.clear();
        self.pending_distill_signatures.clear();
        self.pending_atomize_signatures.clear();
        self.pending_probe_signatures.clear();
        self.probe_errors.clear();
    }

    pub fn is_distilling(&self, block_id: BlockId) -> bool {
        self.distill_states
            .get(&block_id)
            .is_some_and(|state| matches!(state, DistillState::Loading))
    }

    pub fn is_atomizing(&self, block_id: BlockId) -> bool {
        self.atomize_states
            .get(&block_id)
            .is_some_and(|state| matches!(state, AtomizeState::Loading))
    }

    pub fn is_amplifying(&self, block_id: BlockId) -> bool {
        self.amplify_states
            .get(&block_id)
            .is_some_and(|state| matches!(state, AmplifyState::Loading))
    }

    pub fn is_probing(&self, block_id: BlockId) -> bool {
        self.probe_states.get(&block_id).is_some_and(|state| matches!(state, ProbeState::Loading))
    }

    pub fn has_distill_error(&self, block_id: BlockId) -> bool {
        self.distill_states
            .get(&block_id)
            .is_some_and(|state| matches!(state, DistillState::Error { .. }))
    }

    pub fn has_atomize_error(&self, block_id: BlockId) -> bool {
        self.atomize_states
            .get(&block_id)
            .is_some_and(|state| matches!(state, AtomizeState::Error { .. }))
    }

    pub fn has_amplify_error(&self, block_id: BlockId) -> bool {
        self.amplify_states
            .get(&block_id)
            .is_some_and(|state| matches!(state, AmplifyState::Error { .. }))
    }

    pub fn mark_distill_loading(&mut self, block_id: BlockId, request_signature: RequestSignature) {
        self.distill_states.insert(block_id, DistillState::Loading);
        self.pending_distill_signatures.insert(block_id, request_signature);
    }

    pub fn mark_atomize_loading(&mut self, block_id: BlockId, request_signature: RequestSignature) {
        self.atomize_states.insert(block_id, AtomizeState::Loading);
        self.pending_atomize_signatures.insert(block_id, request_signature);
    }

    pub fn mark_amplify_loading(&mut self, block_id: BlockId, request_signature: RequestSignature) {
        self.amplify_states.insert(block_id, AmplifyState::Loading);
        self.pending_amplify_signatures.insert(block_id, request_signature);
    }

    pub fn mark_probe_loading(&mut self, block_id: BlockId, request_signature: RequestSignature) {
        self.probe_states.insert(block_id, ProbeState::Loading);
        self.pending_probe_signatures.insert(block_id, request_signature);
        self.probe_errors.remove(&block_id);
    }

    pub fn replace_distill_handle(&mut self, block_id: BlockId, handle: task::Handle) {
        if let Some(previous) = self.distill_handles.remove(&block_id) {
            previous.abort();
        }
        self.distill_handles.insert(block_id, handle.abort_on_drop());
    }

    pub fn replace_atomize_handle(&mut self, block_id: BlockId, handle: task::Handle) {
        if let Some(previous) = self.atomize_handles.remove(&block_id) {
            previous.abort();
        }
        self.atomize_handles.insert(block_id, handle.abort_on_drop());
    }

    pub fn replace_amplify_handle(&mut self, block_id: BlockId, handle: task::Handle) {
        if let Some(previous) = self.amplify_handles.remove(&block_id) {
            previous.abort();
        }
        self.amplify_handles.insert(block_id, handle.abort_on_drop());
    }

    pub fn replace_probe_handle(&mut self, block_id: BlockId, handle: task::Handle) {
        if let Some(previous) = self.probe_handles.remove(&block_id) {
            previous.abort();
        }
        self.probe_handles.insert(block_id, handle.abort_on_drop());
    }

    /// Finalize a reduce request and return its captured lineage signature.
    ///
    /// Finalization is atomic per block: loading marker, handle, and pending
    /// signature are cleared together.
    pub fn finish_distill_request(&mut self, block_id: BlockId) -> Option<RequestSignature> {
        self.distill_handles.remove(&block_id);
        self.distill_states.remove(&block_id);
        self.pending_distill_signatures.remove(&block_id)
    }

    /// Finalize an atomize request and return its captured lineage signature.
    pub fn finish_atomize_request(&mut self, block_id: BlockId) -> Option<RequestSignature> {
        self.atomize_handles.remove(&block_id);
        self.atomize_states.remove(&block_id);
        self.pending_atomize_signatures.remove(&block_id)
    }

    /// Finalize an expand request and return its captured lineage signature.
    ///
    /// Finalization is atomic per block: loading marker, handle, and pending
    /// signature are cleared together.
    pub fn finish_amplify_request(&mut self, block_id: BlockId) -> Option<RequestSignature> {
        self.amplify_handles.remove(&block_id);
        self.amplify_states.remove(&block_id);
        self.pending_amplify_signatures.remove(&block_id)
    }

    /// Finalize an inquiry request and return signature + deferred error.
    ///
    /// Finalization is atomic per block: loading marker, handle, pending
    /// signature, and deferred stream error are cleared together.
    pub fn finish_probe_request(
        &mut self, block_id: BlockId,
    ) -> (Option<RequestSignature>, Option<UiError>) {
        self.probe_handles.remove(&block_id);
        self.probe_states.remove(&block_id);
        (self.pending_probe_signatures.remove(&block_id), self.probe_errors.remove(&block_id))
    }

    /// Record a deferred inquiry stream error for later finalization.
    pub fn set_probe_error(&mut self, block_id: BlockId, reason: UiError) {
        self.probe_errors.insert(block_id, reason);
    }

    pub fn set_distill_error(&mut self, block_id: BlockId, reason: UiError) {
        self.distill_states.insert(block_id, DistillState::Error { reason });
    }

    pub fn set_atomize_error(&mut self, block_id: BlockId, reason: UiError) {
        self.atomize_states.insert(block_id, AtomizeState::Error { reason });
    }

    pub fn set_amplify_error(&mut self, block_id: BlockId, reason: UiError) {
        self.amplify_states.insert(block_id, AmplifyState::Error { reason });
    }

    pub fn cancel_distill(&mut self, block_id: BlockId) -> bool {
        if !self.is_distilling(block_id) {
            return false;
        }
        if let Some(handle) = self.distill_handles.remove(&block_id) {
            handle.abort();
        }
        self.distill_states.remove(&block_id);
        self.pending_distill_signatures.remove(&block_id);
        true
    }

    pub fn cancel_atomize(&mut self, block_id: BlockId) -> bool {
        if !self.is_atomizing(block_id) {
            return false;
        }
        if let Some(handle) = self.atomize_handles.remove(&block_id) {
            handle.abort();
        }
        self.atomize_states.remove(&block_id);
        self.pending_atomize_signatures.remove(&block_id);
        true
    }

    pub fn cancel_amplify(&mut self, block_id: BlockId) -> bool {
        if !self.is_amplifying(block_id) {
            return false;
        }
        if let Some(handle) = self.amplify_handles.remove(&block_id) {
            handle.abort();
        }
        self.amplify_states.remove(&block_id);
        self.pending_amplify_signatures.remove(&block_id);
        true
    }

    pub fn cancel_probe(&mut self, block_id: BlockId) -> bool {
        if !self.is_probing(block_id) {
            return false;
        }
        if let Some(handle) = self.probe_handles.remove(&block_id) {
            handle.abort();
        }
        self.probe_states.remove(&block_id);
        self.pending_probe_signatures.remove(&block_id);
        self.probe_errors.remove(&block_id);
        true
    }

    pub fn remove_block(&mut self, block_id: BlockId) {
        if let Some(handle) = self.distill_handles.remove(&block_id) {
            handle.abort();
        }
        if let Some(handle) = self.atomize_handles.remove(&block_id) {
            handle.abort();
        }
        if let Some(handle) = self.amplify_handles.remove(&block_id) {
            handle.abort();
        }
        if let Some(handle) = self.probe_handles.remove(&block_id) {
            handle.abort();
        }
        self.pending_distill_signatures.remove(&block_id);
        self.pending_atomize_signatures.remove(&block_id);
        self.pending_amplify_signatures.remove(&block_id);
        self.pending_probe_signatures.remove(&block_id);
        self.probe_errors.remove(&block_id);
        self.distill_states.remove(&block_id);
        self.atomize_states.remove(&block_id);
        self.amplify_states.remove(&block_id);
        self.probe_states.remove(&block_id);
    }
}

/// Per-block amplify operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AmplifyState {
    #[default]
    Idle,
    Loading,
    Error {
        reason: UiError,
    },
}

/// Per-block distill operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DistillState {
    #[default]
    Idle,
    Loading,
    Error {
        reason: UiError,
    },
}

/// Per-block atomize operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AtomizeState {
    #[default]
    Idle,
    Loading,
    Error {
        reason: UiError,
    },
}

/// Per-block probe operation state: Idle → Loading → Idle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProbeState {
    #[default]
    Idle,
    Loading,
}

/// Captured request-context fingerprint for async amplify/distill.
///
/// Built from full lineage (root-to-target points). Responses are applied only
/// when the current lineage fingerprint matches this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestSignature {
    hash: u64,
    item_count: usize,
}

impl RequestSignature {
    #[cfg(test)]
    pub fn from_lineage(lineage: &llm::LineageContext) -> Option<Self> {
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
    /// This includes lineage points, existing children points, and friend block
    /// points so async
    /// amplify/distill responses are invalidated when either input changes.
    pub fn from_block_context(context: &llm::BlockContext) -> Option<Self> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut item_count = 0usize;
        for point in context.lineage().points() {
            Self::text_signature(point).hash(&mut hasher);
            item_count += 1;
        }
        for child_point in context.existing_children().point_strs() {
            Self::text_signature(child_point).hash(&mut hasher);
            item_count += 1;
        }
        for friend_block in context.friend_blocks() {
            Self::text_signature(friend_block.point()).hash(&mut hasher);
            item_count += 1;
            match friend_block.perspective() {
                | Some(perspective) => {
                    1u8.hash(&mut hasher);
                    item_count += 1;
                    Self::text_signature(perspective).hash(&mut hasher);
                    item_count += 1;
                }
                | None => {
                    0u8.hash(&mut hasher);
                    item_count += 1;
                }
            }
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
    fn distill_state_default_is_idle() {
        assert_eq!(DistillState::default(), DistillState::Idle);
    }

    #[test]
    fn amplify_state_default_is_idle() {
        assert_eq!(AmplifyState::default(), AmplifyState::Idle);
    }

    #[test]
    fn request_signature_from_empty_lineage_is_none() {
        let lineage = llm::LineageContext::from_points(vec![]);
        assert!(RequestSignature::from_lineage(&lineage).is_none());
    }

    #[test]
    fn request_signature_changes_when_lineage_changes() {
        let first = llm::LineageContext::from_points(vec!["root".to_string(), "child".to_string()]);
        let second =
            llm::LineageContext::from_points(vec!["root changed".to_string(), "child".to_string()]);
        assert_ne!(RequestSignature::from_lineage(&first), RequestSignature::from_lineage(&second));
    }

    #[test]
    fn request_signature_from_block_context_changes_when_children_change() {
        let lineage = llm::LineageContext::from_points(vec!["root".to_string()]);
        let ctx1 = llm::BlockContext::new(lineage.clone(), vec!["child_a".to_string()], vec![]);
        let ctx2 = llm::BlockContext::new(lineage.clone(), vec!["child_b".to_string()], vec![]);
        assert_ne!(
            RequestSignature::from_block_context(&ctx1),
            RequestSignature::from_block_context(&ctx2)
        );
    }

    #[test]
    fn request_signature_from_block_context_matches_lineage_when_no_children() {
        let lineage =
            llm::LineageContext::from_points(vec!["root".to_string(), "child".to_string()]);
        let ctx = llm::BlockContext::new(lineage.clone(), vec![], vec![]);
        assert_eq!(
            RequestSignature::from_lineage(&lineage),
            RequestSignature::from_block_context(&ctx)
        );
    }

    #[test]
    fn request_signature_from_block_context_changes_when_friend_blocks_change() {
        let lineage = llm::LineageContext::from_points(vec!["root".to_string()]);
        let ctx1 = llm::BlockContext::new(
            lineage.clone(),
            vec![],
            vec![llm::FriendContext::with_context(
                "friend a".to_string(),
                None,
                true,
                true,
                None,
                None,
            )],
        );
        let ctx2 = llm::BlockContext::new(
            lineage,
            vec![],
            vec![llm::FriendContext::with_context(
                "friend b".to_string(),
                None,
                true,
                true,
                None,
                None,
            )],
        );
        assert_ne!(
            RequestSignature::from_block_context(&ctx1),
            RequestSignature::from_block_context(&ctx2)
        );
    }

    #[test]
    fn request_signature_from_block_context_changes_when_friend_perspective_changes() {
        let lineage = llm::LineageContext::from_points(vec!["root".to_string()]);
        let ctx1 = llm::BlockContext::new(
            lineage.clone(),
            vec![],
            vec![llm::FriendContext::with_context(
                "friend".to_string(),
                Some("supportive".to_string()),
                true,
                true,
                None,
                None,
            )],
        );
        let ctx2 = llm::BlockContext::new(
            lineage,
            vec![],
            vec![llm::FriendContext::with_context(
                "friend".to_string(),
                Some("critical".to_string()),
                true,
                true,
                None,
                None,
            )],
        );
        assert_ne!(
            RequestSignature::from_block_context(&ctx1),
            RequestSignature::from_block_context(&ctx2)
        );
    }
}
