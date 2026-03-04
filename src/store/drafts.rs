//! Per-block draft records and friend-block relations.
//!
//! Each draft type stores in-progress LLM suggestions or user-authored text
//! that must survive reloads and mount save/load round-trips.  The maps are
//! sparse: only blocks with pending drafts carry entries.

use super::{BlockId, BlockPanelBarState, BlockStore, FriendBlock};
use serde::{Deserialize, Serialize};

/// Persisted amplification draft payload keyed by [`BlockId`].
///
/// Stored in [`BlockStore`] so in-progress rewrite/child suggestions survive
/// reloads and mount save/load round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmplificationDraftRecord {
    pub rewrite: Option<String>,
    pub children: Vec<String>,
}

/// Persisted atomization draft payload keyed by [`BlockId`].
///
/// Stores an optional rewrite of the original text plus the list of distinct
/// information points produced by atomize until the user accepts or discards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomizationDraftRecord {
    /// Optional restatement of the target block suitable as parent heading.
    #[serde(default)]
    pub rewrite: Option<String>,
    pub points: Vec<String>,
}

/// Persisted distillation draft payload keyed by [`BlockId`].
///
/// When `redundant_children` is non-empty, the distillation draft suggests that
/// those children are captured by the condensed text and can be deleted.
/// The [`BlockId`]s are resolved at response time from the LLM's returned
/// indices into the children snapshot that was sent with the request.
///
/// Note: `reduction` is `None` when the user rejected the replacement but
/// chose to continue reviewing children; point and children are independent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillationDraftRecord {
    /// Proposed replacement text; `None` if user rejected it (children review only).
    pub reduction: Option<String>,
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

/// Persisted probe draft payload keyed by target [`BlockId`].
///
/// This captures the probe question and response for a target block until the user
/// applies or dismisses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeDraftRecord {
    pub inquiry: String,
    pub response: String,
}

impl BlockStore {
    /// Get the amplification draft for a block, if any.
    ///
    /// # Returns
    /// - `Some(&AmplificationDraftRecord)` if the block has a pending amplification draft.
    /// - `None` if no amplification draft exists for this block.
    pub fn amplification_draft(&self, id: &BlockId) -> Option<&AmplificationDraftRecord> {
        self.amplification_drafts.get(*id)
    }

    /// Get a mutable reference to the amplification draft for a block, if any.
    pub fn amplification_draft_mut(
        &mut self, id: &BlockId,
    ) -> Option<&mut AmplificationDraftRecord> {
        self.amplification_drafts.get_mut(*id)
    }

    /// Insert or replace the amplification draft for a block.
    ///
    /// # Ensures
    /// - The draft is stored in the sparse map keyed by the block id.
    pub fn insert_amplification_draft(&mut self, id: BlockId, draft: AmplificationDraftRecord) {
        self.amplification_drafts.insert(id, draft);
    }

    /// Remove the amplification draft for a block, returning the removed draft if any.
    pub fn remove_amplification_draft(&mut self, id: &BlockId) -> Option<AmplificationDraftRecord> {
        self.amplification_drafts.remove(*id)
    }

    /// Get the atomization draft for a block, if any.
    pub fn atomization_draft(&self, id: &BlockId) -> Option<&AtomizationDraftRecord> {
        self.atomization_drafts.get(*id)
    }

    /// Get a mutable reference to the atomization draft for a block, if any.
    pub fn atomization_draft_mut(&mut self, id: &BlockId) -> Option<&mut AtomizationDraftRecord> {
        self.atomization_drafts.get_mut(*id)
    }

    /// Insert or replace the atomization draft for a block.
    pub fn insert_atomization_draft(&mut self, id: BlockId, draft: AtomizationDraftRecord) {
        self.atomization_drafts.insert(id, draft);
    }

    /// Remove the atomization draft for a block, returning the removed draft if any.
    pub fn remove_atomization_draft(&mut self, id: &BlockId) -> Option<AtomizationDraftRecord> {
        self.atomization_drafts.remove(*id)
    }

    /// Get the distillation draft for a block, if any.
    ///
    /// # Returns
    /// - `Some(&DistillationDraftRecord)` if the block has a pending distillation draft.
    /// - `None` if no distillation draft exists for this block.
    pub fn distillation_draft(&self, id: &BlockId) -> Option<&DistillationDraftRecord> {
        self.distillation_drafts.get(*id)
    }

    /// Get a mutable reference to the distillation draft for a block, if any.
    pub fn distillation_draft_mut(&mut self, id: &BlockId) -> Option<&mut DistillationDraftRecord> {
        self.distillation_drafts.get_mut(*id)
    }

    /// Insert or replace the distillation draft for a block.
    pub fn insert_distillation_draft(&mut self, id: BlockId, draft: DistillationDraftRecord) {
        self.distillation_drafts.insert(id, draft);
    }

    /// Remove the distillation draft for a block, returning the removed draft if any.
    pub fn remove_distillation_draft(&mut self, id: &BlockId) -> Option<DistillationDraftRecord> {
        self.distillation_drafts.remove(*id)
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

    /// Get the probe draft for a block, if any.
    ///
    /// # Returns
    /// - `Some(&ProbeDraftRecord)` if the block has a pending probe draft.
    /// - `None` if no probe draft exists for this block.
    pub fn probe_draft(&self, id: &BlockId) -> Option<&ProbeDraftRecord> {
        self.probe_drafts.get(*id)
    }

    /// Set the probe question for a block.
    ///
    /// # Ensures
    /// - If `inquiry` is empty after trimming, removes any existing draft.
    /// - Otherwise stores the trimmed inquiry text and clears any old response.
    pub fn set_probe_question(&mut self, id: BlockId, inquiry: String) {
        let trimmed = inquiry.trim().to_string();
        if trimmed.is_empty() {
            self.probe_drafts.remove(id);
            return;
        }
        self.probe_drafts
            .insert(id, ProbeDraftRecord { inquiry: trimmed, response: String::new() });
    }

    /// Set the probe response for a block.
    ///
    /// # Ensures
    /// - If `response` is empty (after trimming), removes any existing draft.
    /// - Otherwise, stores the trimmed response text.
    /// - Preserves the probe question if it exists.
    pub fn set_probe_response(&mut self, id: BlockId, response: String) {
        let trimmed = response.trim();
        if trimmed.is_empty() {
            self.probe_drafts.remove(id);
        } else {
            let existing_inquiry =
                self.probe_drafts.get(id).map(|r| r.inquiry.clone()).unwrap_or_default();
            self.probe_drafts.insert(
                id,
                ProbeDraftRecord { inquiry: existing_inquiry, response: trimmed.to_string() },
            );
        }
    }

    /// Append one streaming response chunk to a block's inquiry draft.
    ///
    /// # Ensures
    /// - No-op if `chunk` is empty.
    /// - Preserves existing inquiry text.
    /// - Creates a draft with empty inquiry if none exists yet.
    pub fn append_inquiry_response_chunk(&mut self, id: BlockId, chunk: &str) {
        if chunk.is_empty() {
            return;
        }

        if let Some(record) = self.probe_drafts.get_mut(id) {
            record.response.push_str(chunk);
            return;
        }

        self.probe_drafts
            .insert(id, ProbeDraftRecord { inquiry: String::new(), response: chunk.to_string() });
    }

    /// Remove the probe draft for a block, returning the removed draft if any.
    pub fn remove_probe_draft(&mut self, id: &BlockId) -> Option<ProbeDraftRecord> {
        self.probe_drafts.remove(*id)
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
    pub fn block_panel_state(&self, id: &BlockId) -> Option<&BlockPanelBarState> {
        self.block_panel_state.get(*id)
    }

    /// Set (or clear) panel state for a target.
    pub fn set_block_panel_state(&mut self, target: &BlockId, state: Option<BlockPanelBarState>) {
        match state {
            | None => {
                self.block_panel_state.remove(*target);
            }
            | Some(s) => {
                self.block_panel_state.insert(*target, s);
            }
        }
    }
}
