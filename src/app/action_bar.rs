//! Action bar: types, view-model construction, responsive projection,
//! keyboard shortcuts, and dispatch for the per-block toolbar.
//!
//! The action bar is a pure function pipeline:
//!   `RowContext` -> `build_action_bar_vm` -> `project_for_viewport` -> `ActionBarVm`
//!
//! `ActionBarVm` is a view-model: it carries no mutable state and is rebuilt
//! on every frame from the current `RowContext`.

use super::{AppState, ExpandState, Message, ReduceState};
use crate::store::BlockId;
use iced::keyboard::{Key, Modifiers, key::Named};

/// Identifier for a user-visible action in the action bar.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    Expand,
    Reduce,
    Cancel,
    AddChild,
    AcceptAll,
    Retry,
    DismissDraft,
    CollapseBranch,
    ExpandBranch,
    AddSibling,
    DuplicateBlock,
    ArchiveBlock,
    SaveToFile,
    LoadFromFile,
}

/// Whether an action can fire given the current row state.
///
/// The view layer uses this to disable buttons without removing them.
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

/// Display tier for an action: determines which section of the bar it lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionPriority {
    /// Always visible in the primary row.
    Pinned,
    /// Shown when context applies; demoted to overflow on narrower viewports.
    Contextual,
    /// Only reachable through the overflow menu.
    OverflowOnly,
}

/// Complete description of one toolbar button: identity, display text,
/// availability state, display tier, and whether it requires confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub label: &'static str,
    pub availability: ActionAvailability,
    pub priority: ActionPriority,
    /// Destructive actions are rendered with a warning style.
    pub destructive: bool,
}

impl ActionDescriptor {
    /// Create a non-destructive action descriptor.
    pub fn new(
        id: ActionId, label: &'static str, availability: ActionAvailability,
        priority: ActionPriority,
    ) -> Self {
        Self { id, label, availability, priority, destructive: false }
    }

    /// Builder method: mark this action as destructive.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
}

/// Status indicator rendered below the action bar row.
///
/// Exactly one chip is shown per block, determined by [`RowContext::ui_state`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusChipVm {
    Loading { op: ActionId },
    Error { op: ActionId, message: String, retry_action: ActionId },
    DraftActive { suggestion_count: usize },
}

/// View-model for one block's action bar.
///
/// Actions live in three buckets: `primary` (always visible), `contextual`
/// (visible when applicable), and `overflow` (behind a menu).
/// [`project_for_viewport`] only moves actions between buckets — never
/// creates or removes them.
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

/// Snapshot of one block's state relevant to action bar construction.
///
/// Built from live `AppState` each frame; consumed by [`build_action_bar_vm`].
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
    /// Whether this block is already part of a mounted file.
    /// When true, "Save to file" is disabled (one node = one file).
    pub is_mounted: bool,
    /// Whether this block has any children.
    /// When false and not mounted, "Load from file" is available.
    pub has_children: bool,
    /// Whether this block is an unexpanded mount (children still on disk).
    /// When true, SaveToFile and LoadFromFile are hidden.
    pub is_unexpanded_mount: bool,
}

/// Resolved UI state for a row, used to pick availability and status chips.
///
/// Priority: busy > error > draft > idle.
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

/// Build a full action bar view-model from a row's context.
///
/// This is the first stage of the pipeline; call [`project_for_viewport`]
/// afterward to adapt to the current viewport width.
pub fn build_action_bar_vm(ctx: &RowContext) -> ActionBarVm {
    let row_state = ctx.ui_state();
    let mut vm = ActionBarVm::empty();

    let reduce_availability = if ctx.is_empty_point() {
        ActionAvailability::DisabledEmptyPoint
    } else if matches!(row_state, RowUiState::BusyExpand | RowUiState::BusyReduce) {
        ActionAvailability::DisabledBusy
    } else {
        ActionAvailability::Enabled
    };

    let expand_availability =
        if matches!(row_state, RowUiState::BusyExpand | RowUiState::BusyReduce) {
            ActionAvailability::DisabledBusy
        } else {
            ActionAvailability::Enabled
        };

    let add_child_availability = if matches!(row_state, RowUiState::BusyExpand) {
        ActionAvailability::DisabledBusy
    } else {
        ActionAvailability::Enabled
    };

    vm.primary.push(ActionDescriptor::new(
        ActionId::Expand,
        "Expand",
        expand_availability,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::Reduce,
        "Reduce",
        reduce_availability,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::AddChild,
        "Add child",
        add_child_availability,
        ActionPriority::Pinned,
    ));

    if ctx.draft_suggestion_count > 0 {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::AcceptAll,
            "Accept all",
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    if matches!(row_state, RowUiState::ErrorExpand | RowUiState::ErrorReduce) {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::Retry,
            "Retry",
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    if matches!(row_state, RowUiState::BusyExpand | RowUiState::BusyReduce) {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::Cancel,
            "Cancel",
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    if ctx.has_draft {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::DismissDraft,
            "Dismiss",
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    vm.overflow.push(ActionDescriptor::new(
        ActionId::AddSibling,
        "Add sibling",
        ActionAvailability::Enabled,
        ActionPriority::OverflowOnly,
    ));
    vm.overflow.push(ActionDescriptor::new(
        ActionId::DuplicateBlock,
        "Duplicate",
        ActionAvailability::Enabled,
        ActionPriority::OverflowOnly,
    ));
    if !ctx.is_mounted && !ctx.is_unexpanded_mount {
        vm.overflow.push(ActionDescriptor::new(
            ActionId::SaveToFile,
            "Save to file",
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        ));
    }
    if !ctx.has_children && !ctx.is_mounted && !ctx.is_unexpanded_mount {
        vm.overflow.push(ActionDescriptor::new(
            ActionId::LoadFromFile,
            "Load from file",
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        ));
    }
    vm.overflow.push(
        ActionDescriptor::new(
            ActionId::ArchiveBlock,
            "Archive",
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        )
        .destructive(),
    );

    vm.status_chip = match row_state {
        | RowUiState::BusyExpand => Some(StatusChipVm::Loading { op: ActionId::Expand }),
        | RowUiState::BusyReduce => Some(StatusChipVm::Loading { op: ActionId::Reduce }),
        | RowUiState::ErrorExpand => Some(StatusChipVm::Error {
            op: ActionId::Expand,
            message: "Expand failed".to_string(),
            retry_action: ActionId::Retry,
        }),
        | RowUiState::ErrorReduce => Some(StatusChipVm::Error {
            op: ActionId::Reduce,
            message: "Reduce failed".to_string(),
            retry_action: ActionId::Retry,
        }),
        | RowUiState::DraftActive => {
            Some(StatusChipVm::DraftActive { suggestion_count: ctx.draft_suggestion_count })
        }
        | RowUiState::Idle => None,
    };

    vm
}

/// Responsive breakpoint bucket for viewport-dependent layout.
///
/// Variants beyond `Wide` are matched in `project_for_viewport` but not yet
/// constructed outside tests; they exist for upcoming responsive breakpoints.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportBucket {
    Wide,
    Medium,
    Compact,
    TouchCompact,
}

/// Redistribute actions across buckets based on available viewport width.
///
/// Moves actions from higher-visibility buckets to overflow as space shrinks.
/// Never creates or removes actions.
pub fn project_for_viewport(mut vm: ActionBarVm, bucket: ViewportBucket) -> ActionBarVm {
    match bucket {
        | ViewportBucket::Wide => vm,
        | ViewportBucket::Medium => {
            vm.overflow.append(&mut vm.contextual);
            vm
        }
        | ViewportBucket::Compact => {
            vm.overflow.append(&mut vm.contextual);
            if let Some(index) = vm.primary.iter().position(|action| action.id == ActionId::Reduce)
            {
                vm.overflow.push(vm.primary.remove(index));
            }
            vm
        }
        | ViewportBucket::TouchCompact => {
            vm.overflow.append(&mut vm.contextual);
            vm.overflow.append(&mut vm.primary);
            vm
        }
    }
}

/// Map a key press to an action shortcut, if any.
pub fn shortcut_to_action(key: Key, modifiers: Modifiers) -> Option<ActionId> {
    if !modifiers.control() {
        return None;
    }

    if modifiers.shift() {
        match key {
            | Key::Named(Named::Enter) => return Some(ActionId::AddSibling),
            | Key::Character(value) if value.eq_ignore_ascii_case("a") => {
                return Some(ActionId::AcceptAll);
            }
            | _ => {}
        }
    }

    match key {
        | Key::Character(value) if value == "." => Some(ActionId::Expand),
        | Key::Character(value) if value == "," => Some(ActionId::Reduce),
        | Key::Named(Named::Enter) => Some(ActionId::AddChild),
        | Key::Named(Named::Backspace) => Some(ActionId::ArchiveBlock),
        | _ => None,
    }
}

/// Convert an action descriptor to a message, returning `None` if disabled.
pub fn action_to_message(
    state: &AppState, block_id: &BlockId, descriptor: &ActionDescriptor,
) -> Option<Message> {
    if !descriptor.availability.is_enabled() {
        return None;
    }

    action_to_message_by_id(state, block_id, descriptor.id)
}

/// Convert an action id directly to a message, bypassing availability checks.
///
/// Returns `None` for unimplemented actions and for retry when no matching
/// error state exists.
pub fn action_to_message_by_id(
    state: &AppState, block_id: &BlockId, action_id: ActionId,
) -> Option<Message> {
    match action_id {
        | ActionId::Expand => Some(Message::Expand(*block_id)),
        | ActionId::Reduce => Some(Message::Reduce(*block_id)),
        | ActionId::AddChild => Some(Message::AddChild(*block_id)),
        | ActionId::AcceptAll => Some(Message::AcceptAllExpandedChildren(*block_id)),
        | ActionId::Cancel => cancel_message_for_block(state, block_id),
        | ActionId::Retry => retry_message_for_block(state, block_id),
        | ActionId::DismissDraft => {
            // Dismiss whichever draft exists (or both if both exist)
            // The message handler will check and dismiss appropriately
            if state.store.reduction_draft(block_id).is_some() {
                Some(Message::RejectReduction(*block_id))
            } else if state.store.expansion_draft(block_id).is_some() {
                Some(Message::DiscardExpansion(*block_id))
            } else {
                None
            }
        }
        | ActionId::AddSibling => Some(Message::AddSibling(*block_id)),
        | ActionId::DuplicateBlock => Some(Message::DuplicateBlock(*block_id)),
        | ActionId::ArchiveBlock => Some(Message::ArchiveBlock(*block_id)),
        | ActionId::SaveToFile => Some(Message::SaveToFile(*block_id)),
        | ActionId::LoadFromFile => Some(Message::LoadFromFile(*block_id)),
        | ActionId::CollapseBranch | ActionId::ExpandBranch => None,
    }
}

fn cancel_message_for_block(state: &AppState, block_id: &BlockId) -> Option<Message> {
    if state.expand_states.get(*block_id).is_some_and(|s| matches!(s, ExpandState::Loading)) {
        return Some(Message::CancelExpand(*block_id));
    }
    if state.reduce_states.get(*block_id).is_some_and(|s| matches!(s, ReduceState::Loading)) {
        return Some(Message::CancelReduce(*block_id));
    }
    None
}

fn retry_message_for_block(state: &AppState, block_id: &BlockId) -> Option<Message> {
    if state.expand_states.get(*block_id).is_some_and(|s| matches!(s, ExpandState::Error { .. })) {
        return Some(Message::Expand(*block_id));
    }
    if state.reduce_states.get(*block_id).is_some_and(|s| matches!(s, ReduceState::Error { .. })) {
        return Some(Message::Reduce(*block_id));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_context() -> RowContext {
        RowContext {
            block_id: BlockId::default(),
            point_text: "hello".to_string(),
            has_draft: false,
            draft_suggestion_count: 0,
            has_expand_error: false,
            has_reduce_error: false,
            is_expanding: false,
            is_reducing: false,
            is_mounted: false,
            has_children: true,
            is_unexpanded_mount: false,
        }
    }

    #[test]
    fn shows_primary_actions_by_default() {
        let vm = build_action_bar_vm(&row_context());
        let ids = vm.primary.into_iter().map(|action| action.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![ActionId::Expand, ActionId::Reduce, ActionId::AddChild]);
    }

    #[test]
    fn compact_moves_reduce_to_overflow() {
        let vm = build_action_bar_vm(&row_context());
        let projected = project_for_viewport(vm, ViewportBucket::Compact);
        assert!(projected.primary.iter().all(|action| action.id != ActionId::Reduce));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Reduce));
    }

    #[test]
    fn shows_accept_all_when_draft_has_children() {
        let mut ctx = row_context();
        ctx.has_draft = true;
        ctx.draft_suggestion_count = 2;
        let vm = build_action_bar_vm(&ctx);
        assert!(vm.contextual.iter().any(|action| action.id == ActionId::AcceptAll));
    }

    #[test]
    fn shows_cancel_when_busy_expand() {
        let mut ctx = row_context();
        ctx.is_expanding = true;
        let vm = build_action_bar_vm(&ctx);
        assert!(vm.contextual.iter().any(|action| action.id == ActionId::Cancel));
    }

    #[test]
    fn shows_cancel_when_busy_reduce() {
        let mut ctx = row_context();
        ctx.is_reducing = true;
        let vm = build_action_bar_vm(&ctx);
        assert!(vm.contextual.iter().any(|action| action.id == ActionId::Cancel));
    }

    #[test]
    fn ui_state_expanding_takes_priority() {
        let mut ctx = row_context();
        ctx.is_expanding = true;
        ctx.has_expand_error = true;
        assert_eq!(ctx.ui_state(), RowUiState::BusyExpand);
    }

    #[test]
    fn ui_state_reducing_takes_priority_over_draft() {
        let mut ctx = row_context();
        ctx.is_reducing = true;
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::BusyReduce);
    }

    #[test]
    fn ui_state_error_expand_over_draft() {
        let mut ctx = row_context();
        ctx.has_expand_error = true;
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::ErrorExpand);
    }

    #[test]
    fn ui_state_error_reduce_over_draft() {
        let mut ctx = row_context();
        ctx.has_reduce_error = true;
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::ErrorReduce);
    }

    #[test]
    fn ui_state_draft_active() {
        let mut ctx = row_context();
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::DraftActive);
    }

    #[test]
    fn ui_state_idle_default() {
        let ctx = row_context();
        assert_eq!(ctx.ui_state(), RowUiState::Idle);
    }

    #[test]
    fn is_empty_point_empty_string() {
        let mut ctx = row_context();
        ctx.point_text = "".to_string();
        assert!(ctx.is_empty_point());
    }

    #[test]
    fn is_empty_point_whitespace_only() {
        let mut ctx = row_context();
        ctx.point_text = "   ".to_string();
        assert!(ctx.is_empty_point());
    }

    #[test]
    fn is_empty_point_with_text() {
        let ctx = row_context();
        assert!(!ctx.is_empty_point());
    }

    #[test]
    fn is_enabled_true_for_enabled() {
        let availability = ActionAvailability::Enabled;
        assert!(availability.is_enabled());
    }

    #[test]
    fn is_enabled_false_for_disabled_busy() {
        let availability = ActionAvailability::DisabledBusy;
        assert!(!availability.is_enabled());
    }

    #[test]
    fn visible_actions_combines_primary_and_contextual() {
        let mut ctx = row_context();
        ctx.has_draft = true;
        let vm = build_action_bar_vm(&ctx);
        let visible = vm.visible_actions();

        assert!(visible.iter().any(|action| action.id == ActionId::Expand));
        assert!(visible.iter().any(|action| action.id == ActionId::Reduce));
        assert!(visible.iter().any(|action| action.id == ActionId::AddChild));
        assert!(visible.iter().any(|action| action.id == ActionId::DismissDraft));
        assert!(visible.iter().all(|action| action.priority != ActionPriority::OverflowOnly));
    }

    #[test]
    fn destructive_sets_flag() {
        let descriptor = ActionDescriptor::new(
            ActionId::ArchiveBlock,
            "Archive",
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        )
        .destructive();
        assert!(descriptor.destructive);
    }

    #[test]
    fn medium_moves_contextual_to_overflow() {
        let mut ctx = row_context();
        ctx.has_draft = true;
        let vm = build_action_bar_vm(&ctx);

        let projected = project_for_viewport(vm, ViewportBucket::Medium);

        assert!(projected.contextual.is_empty());
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::DismissDraft));
    }

    #[test]
    fn touch_compact_moves_everything_to_overflow() {
        let mut ctx = row_context();
        ctx.has_draft = true;
        let vm = build_action_bar_vm(&ctx);

        let projected = project_for_viewport(vm, ViewportBucket::TouchCompact);

        assert!(projected.primary.is_empty());
        assert!(projected.contextual.is_empty());
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Expand));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Reduce));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::AddChild));
    }

    #[test]
    fn wide_is_identity() {
        let ctx = row_context();
        let vm = build_action_bar_vm(&ctx);
        let original_count = vm.primary.len();

        let projected = project_for_viewport(vm, ViewportBucket::Wide);

        assert_eq!(projected.primary.len(), original_count);
        assert_eq!(projected.primary.len(), 3);
    }

    #[test]
    fn shortcut_ctrl_dot_expands() {
        let key = Key::Character(".".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Expand));
    }

    #[test]
    fn shortcut_ctrl_comma_reduces() {
        let key = Key::Character(",".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Reduce));
    }

    #[test]
    fn shortcut_ctrl_enter_adds_child() {
        let key = Key::Named(Named::Enter);
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AddChild));
    }

    #[test]
    fn shortcut_ctrl_backspace_archives() {
        let key = Key::Named(Named::Backspace);
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::ArchiveBlock));
    }

    #[test]
    fn shortcut_ctrl_shift_enter_adds_sibling() {
        let key = Key::Named(Named::Enter);
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AddSibling));
    }

    #[test]
    fn shortcut_ctrl_shift_a_accepts_all() {
        let key = Key::Character("a".into());
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AcceptAll));
    }

    #[test]
    fn shortcut_no_modifier_returns_none() {
        let key = Key::Character(".".into());
        let modifiers = Modifiers::empty();
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }

    #[test]
    fn shortcut_unknown_key_returns_none() {
        let key = Key::Character("x".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }
}
