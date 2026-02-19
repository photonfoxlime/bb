use crate::graph::BlockId;

// Some variants (Overflow, CollapseBranch, ExpandBranch, OpenAsFocus) are
// matched in dispatch and action_icon but not yet constructed; they represent
// planned actions.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    Expand,
    Reduce,
    AddChild,
    AcceptAll,
    Retry,
    DismissDraft,
    Overflow,
    CollapseBranch,
    ExpandBranch,
    AddSibling,
    OpenAsFocus,
    DuplicateBlock,
    ArchiveBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionAvailability {
    Enabled,
    DisabledBusy,
    DisabledEmptyPoint,
}

impl ActionAvailability {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionPriority {
    Pinned,
    Contextual,
    OverflowOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub label: &'static str,
    pub availability: ActionAvailability,
    pub priority: ActionPriority,
    pub destructive: bool,
}

impl ActionDescriptor {
    pub fn new(
        id: ActionId, label: &'static str, availability: ActionAvailability,
        priority: ActionPriority,
    ) -> Self {
        Self { id, label, availability, priority, destructive: false }
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusChipVm {
    Loading { op: ActionId },
    Error { op: ActionId, message: String, retry_action: ActionId },
    DraftActive { suggestion_count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionBarVm {
    pub primary: Vec<ActionDescriptor>,
    pub contextual: Vec<ActionDescriptor>,
    pub overflow: Vec<ActionDescriptor>,
    pub status_chip: Option<StatusChipVm>,
}

impl ActionBarVm {
    pub fn empty() -> Self {
        Self { primary: vec![], contextual: vec![], overflow: vec![], status_chip: None }
    }

    pub fn visible_actions(&self) -> Vec<ActionDescriptor> {
        let mut actions = Vec::with_capacity(self.primary.len() + self.contextual.len());
        actions.extend(self.primary.iter().cloned());
        actions.extend(self.contextual.iter().cloned());
        actions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowContext {
    pub block_id: BlockId,
    pub point_text: String,
    pub has_draft: bool,
    pub draft_suggestion_count: usize,
    pub has_expand_error: bool,
    pub has_reduce_error: bool,
    pub is_expanding: bool,
    pub is_reducing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowUiState {
    Idle,
    BusyExpand,
    BusyReduce,
    DraftActive,
    ErrorExpand,
    ErrorReduce,
}

impl RowContext {
    pub fn ui_state(&self) -> RowUiState {
        if self.is_expanding {
            return RowUiState::BusyExpand;
        }
        if self.is_reducing {
            return RowUiState::BusyReduce;
        }
        if self.has_expand_error {
            return RowUiState::ErrorExpand;
        }
        if self.has_reduce_error {
            return RowUiState::ErrorReduce;
        }
        if self.has_draft {
            return RowUiState::DraftActive;
        }
        RowUiState::Idle
    }

    pub fn is_empty_point(&self) -> bool {
        self.point_text.trim().is_empty()
    }
}
