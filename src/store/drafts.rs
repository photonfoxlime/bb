//! Per-block draft records and friend-block relations.
//!
//! Each draft type stores in-progress LLM suggestions or user-authored text
//! that must survive reloads and mount save/load round-trips.  The maps are
//! sparse: only blocks with pending drafts carry entries.

use super::{BlockId, BlockStore, FriendBlock};
use serde::{Deserialize, Serialize};

/// Persisted expansion draft payload keyed by [`BlockId`].
///
/// Stored in [`BlockStore`] so in-progress rewrite/child suggestions survive
/// reloads and mount save/load round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpansionDraftRecord {
    pub rewrite: Option<String>,
    pub children: Vec<String>,
}

/// Persisted reduction draft payload keyed by [`BlockId`].
///
/// When `redundant_children` is non-empty, the reduction draft suggests that
/// those children are captured by the condensed text and can be deleted.
/// The [`BlockId`]s are resolved at response time from the LLM's returned
/// indices into the children snapshot that was sent with the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReductionDraftRecord {
    pub reduction: String,
    /// Children whose information is captured by the reduction.
    ///
    /// May contain stale ids if children were modified between response
    /// arrival and apply time; consumers must filter at render and apply.
    #[serde(default)]
    pub redundant_children: Vec<BlockId>,
}

/// Persisted instruction draft text keyed by target [`BlockId`].
///
/// Stores per-block instruction-editor input so drafts survive reloads and
/// round-trips through mount projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionDraftRecord {
    pub instruction: String,
}

/// Persisted inquiry draft payload keyed by target [`BlockId`].
///
/// This captures the latest inquiry response for a target block until the user
/// applies or dismisses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InquiryDraftRecord {
    pub response: String,
}


impl BlockStore {
    pub fn expansion_draft(&self, id: &BlockId) -> Option<&ExpansionDraftRecord> {
        self.expansion_drafts.get(*id)
    }

    pub fn expansion_draft_mut(&mut self, id: &BlockId) -> Option<&mut ExpansionDraftRecord> {
        self.expansion_drafts.get_mut(*id)
    }

    pub fn insert_expansion_draft(&mut self, id: BlockId, draft: ExpansionDraftRecord) {
        self.expansion_drafts.insert(id, draft);
    }

    pub fn remove_expansion_draft(&mut self, id: &BlockId) -> Option<ExpansionDraftRecord> {
        self.expansion_drafts.remove(*id)
    }

    pub fn reduction_draft(&self, id: &BlockId) -> Option<&ReductionDraftRecord> {
        self.reduction_drafts.get(*id)
    }

    pub fn insert_reduction_draft(&mut self, id: BlockId, draft: ReductionDraftRecord) {
        self.reduction_drafts.insert(id, draft);
    }

    pub fn remove_reduction_draft(&mut self, id: &BlockId) -> Option<ReductionDraftRecord> {
        self.reduction_drafts.remove(*id)
    }

    pub fn instruction_draft(&self, id: &BlockId) -> Option<&InstructionDraftRecord> {
        self.instruction_drafts.get(*id)
    }

    pub fn set_instruction_draft(&mut self, id: BlockId, instruction: String) {
        if instruction.is_empty() {
            self.instruction_drafts.remove(id);
        } else {
            self.instruction_drafts.insert(id, InstructionDraftRecord { instruction });
        }
    }

    pub fn remove_instruction_draft(&mut self, id: &BlockId) -> Option<InstructionDraftRecord> {
        self.instruction_drafts.remove(*id)
    }

    pub fn inquiry_draft(&self, id: &BlockId) -> Option<&InquiryDraftRecord> {
        self.inquiry_drafts.get(*id)
    }

    pub fn set_inquiry_draft(&mut self, id: BlockId, response: String) {
        let trimmed = response.trim();
        if trimmed.is_empty() {
            self.inquiry_drafts.remove(id);
        } else {
            self.inquiry_drafts.insert(id, InquiryDraftRecord { response: trimmed.to_string() });
        }
    }

    pub fn remove_inquiry_draft(&mut self, id: &BlockId) -> Option<InquiryDraftRecord> {
        self.inquiry_drafts.remove(*id)
    }

    /// Whether the given block's children are folded (hidden) in the UI.
    pub fn is_collapsed(&self, id: &BlockId) -> bool {
        self.view_collapsed.contains_key(*id)
    }

    /// Toggle the fold state of a block. Returns the new state (`true` = collapsed).
    pub fn toggle_collapsed(&mut self, id: &BlockId) -> bool {
        if self.view_collapsed.remove(*id).is_some() {
            false
        } else {
            self.view_collapsed.insert(*id, true);
            true
        }
    }

    /// Return the friend blocks for a target, or an empty slice if none.
    pub fn friend_blocks_for(&self, target: &BlockId) -> &[FriendBlock] {
        self.friend_blocks.get(*target).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Set (or clear) friend blocks for a target.
    pub fn set_friend_blocks_for(&mut self, target: &BlockId, friend_block_ids: Vec<FriendBlock>) {
        if friend_block_ids.is_empty() {
            self.friend_blocks.remove(*target);
        } else {
            self.friend_blocks.insert(*target, friend_block_ids);
        }
    }
}
