//! Per-block draft records and friend-block relations.
//!
//! Each draft type stores in-progress LLM suggestions or user-authored text
//! that must survive reloads and mount save/load round-trips.  The maps are
//! sparse: only blocks with pending drafts carry entries.

use super::{BlockId, BlockStore, FriendBlock, PanelBarState};
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
/// This captures the inquiry question and response for a target block until the user
/// applies or dismisses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InquiryDraftRecord {
    pub inquiry: String,
    pub response: String,
}

impl BlockStore {
    /// Get the expansion draft for a block, if any.
    ///
    /// # Returns
    /// - `Some(&ExpansionDraftRecord)` if the block has a pending expansion draft.
    /// - `None` if no expansion draft exists for this block.
    pub fn expansion_draft(&self, id: &BlockId) -> Option<&ExpansionDraftRecord> {
        self.expansion_drafts.get(*id)
    }

    /// Get a mutable reference to the expansion draft for a block, if any.
    pub fn expansion_draft_mut(&mut self, id: &BlockId) -> Option<&mut ExpansionDraftRecord> {
        self.expansion_drafts.get_mut(*id)
    }

    /// Insert or replace the expansion draft for a block.
    ///
    /// # Ensures
    /// - The draft is stored in the sparse map keyed by the block id.
    pub fn insert_expansion_draft(&mut self, id: BlockId, draft: ExpansionDraftRecord) {
        self.expansion_drafts.insert(id, draft);
    }

    /// Remove the expansion draft for a block, returning the removed draft if any.
    pub fn remove_expansion_draft(&mut self, id: &BlockId) -> Option<ExpansionDraftRecord> {
        self.expansion_drafts.remove(*id)
    }

    /// Get the reduction draft for a block, if any.
    ///
    /// # Returns
    /// - `Some(&ReductionDraftRecord)` if the block has a pending reduction draft.
    /// - `None` if no reduction draft exists for this block.
    pub fn reduction_draft(&self, id: &BlockId) -> Option<&ReductionDraftRecord> {
        self.reduction_drafts.get(*id)
    }

    /// Insert or replace the reduction draft for a block.
    pub fn insert_reduction_draft(&mut self, id: BlockId, draft: ReductionDraftRecord) {
        self.reduction_drafts.insert(id, draft);
    }

    /// Remove the reduction draft for a block, returning the removed draft if any.
    pub fn remove_reduction_draft(&mut self, id: &BlockId) -> Option<ReductionDraftRecord> {
        self.reduction_drafts.remove(*id)
    }

    pub fn instruction_draft(&self, id: &BlockId) -> Option<&InstructionDraftRecord> {
        self.instruction_drafts.get(*id)
    }

    /// Set the instruction draft for a block.
    ///
    /// # Ensures
    /// - If `instruction` is empty, removes any existing draft.
    /// - Otherwise, stores the instruction text.
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

    /// Get the inquiry draft for a block, if any.
    ///
    /// # Returns
    /// - `Some(&InquiryDraftRecord)` if the block has a pending inquiry draft.
    /// - `None` if no inquiry draft exists for this block.
    pub fn inquiry_draft(&self, id: &BlockId) -> Option<&InquiryDraftRecord> {
        self.inquiry_drafts.get(*id)
    }

    /// Set the inquiry question for a block.
    ///
    /// # Ensures
    /// - Stores the inquiry text.
    /// - If a response already exists, preserves it.
    pub fn set_inquiry(&mut self, id: BlockId, inquiry: String) {
        let trimmed = inquiry.trim().to_string();
        let existing_response = self
            .inquiry_drafts
            .get(id)
            .and_then(|r| if r.response.is_empty() { None } else { Some(r.response.clone()) });
        self.inquiry_drafts.insert(
            id,
            InquiryDraftRecord {
                inquiry: trimmed,
                response: existing_response.unwrap_or_default(),
            },
        );
    }

    /// Set the inquiry response for a block.
    ///
    /// # Ensures
    /// - If `response` is empty (after trimming), removes any existing draft.
    /// - Otherwise, stores the trimmed response text.
    /// - Preserves the inquiry question if it exists.
    pub fn set_inquiry_draft(&mut self, id: BlockId, response: String) {
        let trimmed = response.trim();
        if trimmed.is_empty() {
            self.inquiry_drafts.remove(id);
        } else {
            let existing_inquiry =
                self.inquiry_drafts.get(id).map(|r| r.inquiry.clone()).unwrap_or_default();
            self.inquiry_drafts.insert(
                id,
                InquiryDraftRecord { inquiry: existing_inquiry, response: trimmed.to_string() },
            );
        }
    }

    /// Remove the inquiry draft for a block, returning the removed draft if any.
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

    /// Get the panel state for a block, if any.
    pub fn panel_state(&self, id: &BlockId) -> Option<&PanelBarState> {
        self.panel_state.get(*id)
    }

    /// Set (or clear) panel state for a target.
    pub fn set_panel_state(&mut self, target: &BlockId, state: Option<PanelBarState>) {
        match state {
            | None => {
                self.panel_state.remove(*target);
            }
            | Some(s) => {
                self.panel_state.insert(*target, s);
            }
        }
    }
}
