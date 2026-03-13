//! Action bar: types, view-model construction, responsive projection,
//! keyboard shortcuts, and dispatch for the per-block toolbar.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Keep module docs broad; pipeline and interaction semantics are documented on
//! owning VM types and builder/projection functions below.

use super::{
    AppState, LinkModeMessage, Message, MountFileMessage, StructureMessage,
    patch::{PatchKind, PatchMessage},
};
use crate::{store::BlockId, theme};
use iced::{
    Element,
    keyboard::{Key, Modifiers},
};
use lucide_icons::iced as icons;

/// Identifier for a user-visible action in the action bar.
///
/// Each variant corresponds to one button in the per-block toolbar.
/// Actions are categorized by priority (pinned, contextual, overflow-only)
/// and availability (enabled, disabled-busy, disabled-empty-point).
///
/// # LLM structure actions (Cmd+. , Cmd+, , Cmd+/)
///
/// - **Amplify**: Add detail, examples, context; produces rewrite + children.
/// - **Distill**: Summarize; may mark children redundant.
/// - **Atomize**: Break into distinct information points.
/// - **Probe**: Open the inline probe panel for instruction-driven LLM actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    /// Amplify: add detail, examples, context; rewrite + child suggestions.
    Amplify,
    /// Distill: summarize; may mark children redundant for removal.
    Distill,
    /// Atomize: break text into distinct information points.
    Atomize,
    /// Probe: open the inline probe panel.
    Probe,
    Cancel,
    /// Append a link to the block's point via the link-input panel.
    AddLink,
    AddChild,
    AddParent,
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
    /// Navigate into a block's subtree.
    ///
    /// Shows the block's children as the new root view. Only available
    /// when the block has children. The action is placed in overflow
    /// to reduce toolbar clutter, as drill-down is a secondary workflow.
    EnterBlock,
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

/// Complete description of one toolbar button: identity, availability state,
/// display tier, and whether it requires confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub availability: ActionAvailability,
    pub priority: ActionPriority,
    /// Destructive actions are rendered with a warning style.
    pub destructive: bool,
}

impl ActionDescriptor {
    /// Create a non-destructive action descriptor.
    pub fn new(id: ActionId, availability: ActionAvailability, priority: ActionPriority) -> Self {
        Self { id, availability, priority, destructive: false }
    }

    /// Builder method: mark this action as destructive.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
}

/// Get the i18n translation key for an action.
pub fn action_i18n_key(id: ActionId) -> &'static str {
    match id {
        | ActionId::Amplify => "action_amplify",
        | ActionId::Distill => "action_distill",
        | ActionId::Atomize => "action_atomize",
        | ActionId::Probe => "action_probe",
        | ActionId::AddLink => "action_add_link",
        | ActionId::AddChild => "action_add_child",
        | ActionId::AddParent => "action_add_parent",
        | ActionId::AcceptAll => "action_accept_all",
        | ActionId::Retry => "action_retry",
        | ActionId::DismissDraft => "action_dismiss",
        | ActionId::Cancel => "action_cancel",
        | ActionId::CollapseBranch => "action_collapse_branch",
        | ActionId::ExpandBranch => "action_expand_branch",
        | ActionId::AddSibling => "action_add_sibling",
        | ActionId::DuplicateBlock => "action_duplicate",
        | ActionId::ArchiveBlock => "action_archive",
        | ActionId::SaveToFile => "action_save_to_file",
        | ActionId::LoadFromFile => "action_load_from_file",
        | ActionId::EnterBlock => "action_enter_block",
    }
}

/// Get the i18n translation key for a status error message based on the action that failed.
pub fn status_error_i18n_key(op: ActionId) -> &'static str {
    match op {
        | ActionId::Amplify => "status_amplify_failed",
        | ActionId::Distill => "status_distill_failed",
        | ActionId::Atomize => "status_atomize_failed",
        | _ => "status_amplify_failed",
    }
}

/// Exactly one chip is shown per block, determined by [`RowContext::ui_state`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusChipVm {
    Loading { op: ActionId },
    Error { op: ActionId, retry_action: ActionId },
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
    pub has_amplify_error: bool,
    pub has_distill_error: bool,
    pub has_atomize_error: bool,
    pub is_amplifying: bool,
    pub is_distilling: bool,
    pub is_atomizing: bool,
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
    BusyAmplify,
    BusyDistill,
    BusyAtomize,
    DraftActive,
    ErrorAmplify,
    ErrorDistill,
    ErrorAtomize,
}

impl RowContext {
    pub fn ui_state(&self) -> RowUiState {
        if self.is_amplifying {
            return RowUiState::BusyAmplify;
        }
        if self.is_distilling {
            return RowUiState::BusyDistill;
        }
        if self.is_atomizing {
            return RowUiState::BusyAtomize;
        }
        if self.has_amplify_error {
            return RowUiState::ErrorAmplify;
        }
        if self.has_distill_error {
            return RowUiState::ErrorDistill;
        }
        if self.has_atomize_error {
            return RowUiState::ErrorAtomize;
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
/// Pipeline stage 1:
/// `RowContext` -> `build_action_bar_vm` -> [`project_for_viewport`] -> `ActionBarVm`.
///
/// Interaction policy encoded here:
/// - busy states disable conflicting actions and expose cancel/retry,
/// - contextual actions appear only when row state demands them,
/// - overflow actions remain available but hidden behind menu toggle.
pub fn build_action_bar_vm(ctx: &RowContext) -> ActionBarVm {
    let row_state = ctx.ui_state();
    let mut vm = ActionBarVm::empty();

    let distill_availability = if ctx.is_empty_point() {
        ActionAvailability::DisabledEmptyPoint
    } else if matches!(
        row_state,
        RowUiState::BusyAmplify | RowUiState::BusyAtomize | RowUiState::BusyDistill
    ) {
        ActionAvailability::DisabledBusy
    } else {
        ActionAvailability::Enabled
    };

    let amplify_availability = if matches!(
        row_state,
        RowUiState::BusyAmplify | RowUiState::BusyAtomize | RowUiState::BusyDistill
    ) {
        ActionAvailability::DisabledBusy
    } else {
        ActionAvailability::Enabled
    };

    let atomize_availability = if ctx.is_empty_point() {
        ActionAvailability::DisabledEmptyPoint
    } else if matches!(
        row_state,
        RowUiState::BusyAmplify | RowUiState::BusyAtomize | RowUiState::BusyDistill
    ) {
        ActionAvailability::DisabledBusy
    } else {
        ActionAvailability::Enabled
    };

    let add_child_availability =
        if matches!(row_state, RowUiState::BusyAmplify | RowUiState::BusyAtomize) {
            ActionAvailability::DisabledBusy
        } else {
            ActionAvailability::Enabled
        };

    vm.primary.push(ActionDescriptor::new(
        ActionId::Amplify,
        amplify_availability,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::Distill,
        distill_availability,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::Atomize,
        atomize_availability,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::Probe,
        ActionAvailability::Enabled,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::AddLink,
        ActionAvailability::Enabled,
        ActionPriority::Pinned,
    ));
    vm.primary.push(ActionDescriptor::new(
        ActionId::AddChild,
        add_child_availability,
        ActionPriority::Pinned,
    ));

    if ctx.draft_suggestion_count > 0 {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::AcceptAll,
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    if matches!(
        row_state,
        RowUiState::ErrorAmplify | RowUiState::ErrorAtomize | RowUiState::ErrorDistill
    ) {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::Retry,
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    if matches!(
        row_state,
        RowUiState::BusyAmplify | RowUiState::BusyAtomize | RowUiState::BusyDistill
    ) && !ctx.has_draft
    {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::Cancel,
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    if ctx.has_draft {
        vm.contextual.push(ActionDescriptor::new(
            ActionId::DismissDraft,
            ActionAvailability::Enabled,
            ActionPriority::Contextual,
        ));
    }

    vm.overflow.push(ActionDescriptor::new(
        ActionId::AddSibling,
        ActionAvailability::Enabled,
        ActionPriority::OverflowOnly,
    ));
    vm.overflow.push(ActionDescriptor::new(
        ActionId::AddParent,
        ActionAvailability::Enabled,
        ActionPriority::OverflowOnly,
    ));
    vm.overflow.push(ActionDescriptor::new(
        ActionId::DuplicateBlock,
        ActionAvailability::Enabled,
        ActionPriority::OverflowOnly,
    ));
    if !ctx.is_mounted && !ctx.is_unexpanded_mount {
        vm.overflow.push(ActionDescriptor::new(
            ActionId::SaveToFile,
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        ));
    }
    if !ctx.has_children && !ctx.is_mounted && !ctx.is_unexpanded_mount {
        vm.overflow.push(ActionDescriptor::new(
            ActionId::LoadFromFile,
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        ));
    }
    // EnterBlock: only shown when block has children (drill-down available)
    // Placed in overflow to keep primary toolbar minimal
    if ctx.has_children {
        vm.overflow.push(ActionDescriptor::new(
            ActionId::EnterBlock,
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        ));
    }
    vm.overflow.push(
        ActionDescriptor::new(
            ActionId::ArchiveBlock,
            ActionAvailability::Enabled,
            ActionPriority::OverflowOnly,
        )
        .destructive(),
    );

    vm.status_chip = match row_state {
        | RowUiState::BusyAmplify => Some(StatusChipVm::Loading { op: ActionId::Amplify }),
        | RowUiState::BusyAtomize => Some(StatusChipVm::Loading { op: ActionId::Atomize }),
        | RowUiState::BusyDistill => Some(StatusChipVm::Loading { op: ActionId::Distill }),
        | RowUiState::ErrorAmplify => {
            Some(StatusChipVm::Error { op: ActionId::Amplify, retry_action: ActionId::Retry })
        }
        | RowUiState::ErrorAtomize => {
            Some(StatusChipVm::Error { op: ActionId::Atomize, retry_action: ActionId::Retry })
        }
        | RowUiState::ErrorDistill => {
            Some(StatusChipVm::Error { op: ActionId::Distill, retry_action: ActionId::Retry })
        }
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
            if let Some(index) = vm.primary.iter().position(|action| action.id == ActionId::Distill)
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
///
/// Primary modifier policy uses `Modifiers::command()` so the same shortcut set
/// works as `Cmd+...` on macOS and `Ctrl+...` on other platforms.
///
/// The resolver also accepts `control()` as a compatibility fallback for event
/// paths that surface Command through the control bit.
///
/// Design decision: destructive archive is intentionally excluded from keyboard
/// shortcuts. `Cmd/Ctrl+Backspace` is left to editor-native deletion semantics
/// to avoid accidental structural mutation.
///
/// Note: this function is now a compatibility wrapper. The canonical shortcut
/// inventory and matching rules live in `shortcut.rs`, but action bar tests and
/// editor call sites still use this entry point.
pub fn shortcut_to_action(key: Key, modifiers: Modifiers) -> Option<ActionId> {
    super::shortcut::action_shortcut_from_key(key, modifiers)
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
        | ActionId::Amplify => Some(Message::Patch(PatchMessage::Start {
            kind: PatchKind::Amplify,
            block_id: *block_id,
        })),
        | ActionId::Atomize => Some(Message::Patch(PatchMessage::Start {
            kind: PatchKind::Atomize,
            block_id: *block_id,
        })),
        | ActionId::Distill => Some(Message::Patch(PatchMessage::Start {
            kind: PatchKind::Distill,
            block_id: *block_id,
        })),
        | ActionId::Probe => Some(Message::InstructionPanel(
            *block_id,
            super::instruction_panel::InstructionPanelMessage::OpenPanel,
        )),
        | ActionId::AddLink => Some(Message::LinkMode(LinkModeMessage::Enter(*block_id))),
        | ActionId::AddChild => Some(Message::Structure(StructureMessage::AddChild(*block_id))),
        | ActionId::AddParent => Some(Message::Structure(StructureMessage::AddParent(*block_id))),
        | ActionId::AcceptAll => accept_all_message_for_block(state, block_id),
        | ActionId::Cancel => cancel_message_for_block(state, block_id),
        | ActionId::Retry => retry_message_for_block(state, block_id),
        | ActionId::DismissDraft => {
            if state.store.distillation_draft(block_id).is_some() {
                Some(Message::Patch(PatchMessage::RejectRewrite(*block_id)))
            } else if let Some(atomization_draft) = state.store.atomization_draft(block_id) {
                if atomization_draft.rewrite.is_some() && atomization_draft.points.is_empty() {
                    Some(Message::Patch(PatchMessage::RejectRewrite(*block_id)))
                } else {
                    Some(Message::Patch(PatchMessage::DiscardAllChildren(*block_id)))
                }
            } else if let Some(amplification_draft) = state.store.amplification_draft(block_id) {
                if !amplification_draft.children.is_empty() {
                    Some(Message::Patch(PatchMessage::DiscardAllChildren(*block_id)))
                } else if amplification_draft.rewrite.is_some() {
                    Some(Message::Patch(PatchMessage::RejectRewrite(*block_id)))
                } else {
                    None
                }
            } else {
                None
            }
        }
        | ActionId::AddSibling => Some(Message::Structure(StructureMessage::AddSibling(*block_id))),
        | ActionId::DuplicateBlock => {
            Some(Message::Structure(StructureMessage::DuplicateBlock(*block_id)))
        }
        | ActionId::ArchiveBlock => {
            Some(Message::Structure(StructureMessage::ArchiveBlock(*block_id)))
        }
        | ActionId::SaveToFile => Some(Message::MountFile(MountFileMessage::SaveToFile(*block_id))),
        | ActionId::LoadFromFile => {
            Some(Message::MountFile(MountFileMessage::LoadFromFile(*block_id)))
        }
        | ActionId::EnterBlock => {
            Some(Message::Navigation(crate::app::navigation::NavigationMessage::Enter(*block_id)))
        }
        | ActionId::CollapseBranch | ActionId::ExpandBranch => None,
    }
}

fn accept_all_message_for_block(state: &AppState, block_id: &BlockId) -> Option<Message> {
    if state.store.atomization_draft(block_id).is_some() {
        return Some(Message::Patch(PatchMessage::AcceptAllChildren(*block_id)));
    }
    Some(Message::Patch(PatchMessage::AcceptAllChildren(*block_id)))
}

fn cancel_message_for_block(state: &AppState, block_id: &BlockId) -> Option<Message> {
    if state.llm_requests.is_amplifying(*block_id) {
        return Some(Message::Patch(PatchMessage::Cancel {
            kind: PatchKind::Amplify,
            block_id: *block_id,
        }));
    }
    if state.llm_requests.is_distilling(*block_id) {
        return Some(Message::Patch(PatchMessage::Cancel {
            kind: PatchKind::Distill,
            block_id: *block_id,
        }));
    }
    if state.llm_requests.is_atomizing(*block_id) {
        return Some(Message::Patch(PatchMessage::Cancel {
            kind: PatchKind::Atomize,
            block_id: *block_id,
        }));
    }
    None
}

fn retry_message_for_block(state: &AppState, block_id: &BlockId) -> Option<Message> {
    if state.llm_requests.has_amplify_error(*block_id) {
        return Some(Message::Patch(PatchMessage::Start {
            kind: PatchKind::Amplify,
            block_id: *block_id,
        }));
    }
    if state.llm_requests.has_distill_error(*block_id) {
        return Some(Message::Patch(PatchMessage::Start {
            kind: PatchKind::Distill,
            block_id: *block_id,
        }));
    }
    if state.llm_requests.has_atomize_error(*block_id) {
        return Some(Message::Patch(PatchMessage::Start {
            kind: PatchKind::Atomize,
            block_id: *block_id,
        }));
    }
    None
}

/// Icon element for a toolbar action.
pub fn action_icon<'a>(id: ActionId) -> Element<'a, Message> {
    let icon = match id {
        | ActionId::Amplify => icons::icon_maximize_2(),
        | ActionId::Distill => icons::icon_minimize_2(),
        | ActionId::Atomize => icons::icon_maximize(),
        | ActionId::Probe => icons::icon_message_circle(),
        | ActionId::Cancel => icons::icon_circle_x(),
        | ActionId::AddLink => icons::icon_link_2(),
        | ActionId::AddChild => icons::icon_corner_down_right(),
        | ActionId::AddParent => icons::icon_corner_up_left(),
        | ActionId::AcceptAll => icons::icon_check_check(),
        | ActionId::Retry => icons::icon_refresh_cw(),
        | ActionId::DismissDraft => icons::icon_x(),
        | ActionId::CollapseBranch => icons::icon_chevron_down(),
        | ActionId::ExpandBranch => icons::icon_chevron_right(),
        | ActionId::AddSibling => icons::icon_plus(),
        | ActionId::DuplicateBlock => icons::icon_copy(),
        | ActionId::ArchiveBlock => icons::icon_archive(),
        | ActionId::SaveToFile => icons::icon_hard_drive_download(),
        | ActionId::LoadFromFile => icons::icon_hard_drive_upload(),
        | ActionId::EnterBlock => icons::icon_log_in(),
    };
    icon.size(theme::TOOLBAR_ICON_SIZE)
        .line_height(iced::widget::text::LineHeight::Relative(1.0))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::Named;

    fn row_context() -> RowContext {
        RowContext {
            block_id: BlockId::default(),
            point_text: "hello".to_string(),
            has_draft: false,
            draft_suggestion_count: 0,
            has_amplify_error: false,
            has_distill_error: false,
            has_atomize_error: false,
            is_amplifying: false,
            is_distilling: false,
            is_atomizing: false,
            is_mounted: false,
            has_children: true,
            is_unexpanded_mount: false,
        }
    }

    #[test]
    fn shows_primary_actions_by_default() {
        let vm = build_action_bar_vm(&row_context());
        let ids = vm.primary.into_iter().map(|action| action.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                ActionId::Amplify,
                ActionId::Distill,
                ActionId::Atomize,
                ActionId::Probe,
                ActionId::AddLink,
                ActionId::AddChild,
            ]
        );
    }

    #[test]
    fn compact_moves_reduce_to_overflow() {
        let vm = build_action_bar_vm(&row_context());
        let projected = project_for_viewport(vm, ViewportBucket::Compact);
        assert!(projected.primary.iter().all(|action| action.id != ActionId::Distill));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Distill));
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
        ctx.is_amplifying = true;
        let vm = build_action_bar_vm(&ctx);
        assert!(vm.contextual.iter().any(|action| action.id == ActionId::Cancel));
    }

    #[test]
    fn shows_cancel_when_busy_reduce() {
        let mut ctx = row_context();
        ctx.is_distilling = true;
        let vm = build_action_bar_vm(&ctx);
        assert!(vm.contextual.iter().any(|action| action.id == ActionId::Cancel));
    }

    #[test]
    fn hides_cancel_when_draft_active_after_apply() {
        let mut ctx = row_context();
        ctx.has_draft = true;
        ctx.is_distilling = false;
        let vm = build_action_bar_vm(&ctx);
        assert!(!vm.contextual.iter().any(|action| action.id == ActionId::Cancel));
    }

    #[test]
    fn ui_state_expanding_takes_priority() {
        let mut ctx = row_context();
        ctx.is_amplifying = true;
        ctx.has_amplify_error = true;
        assert_eq!(ctx.ui_state(), RowUiState::BusyAmplify);
    }

    #[test]
    fn ui_state_reducing_takes_priority_over_draft() {
        let mut ctx = row_context();
        ctx.is_distilling = true;
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::BusyDistill);
    }

    #[test]
    fn ui_state_error_expand_over_draft() {
        let mut ctx = row_context();
        ctx.has_amplify_error = true;
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::ErrorAmplify);
    }

    #[test]
    fn ui_state_error_reduce_over_draft() {
        let mut ctx = row_context();
        ctx.has_distill_error = true;
        ctx.has_draft = true;
        assert_eq!(ctx.ui_state(), RowUiState::ErrorDistill);
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

        assert!(visible.iter().any(|action| action.id == ActionId::Amplify));
        assert!(visible.iter().any(|action| action.id == ActionId::Atomize));
        assert!(visible.iter().any(|action| action.id == ActionId::Distill));
        assert!(visible.iter().any(|action| action.id == ActionId::Probe));
        assert!(visible.iter().any(|action| action.id == ActionId::AddChild));
        assert!(visible.iter().any(|action| action.id == ActionId::DismissDraft));
        assert!(visible.iter().all(|action| action.priority != ActionPriority::OverflowOnly));
    }

    #[test]
    fn destructive_sets_flag() {
        let descriptor = ActionDescriptor::new(
            ActionId::ArchiveBlock,
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
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Amplify));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Atomize));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Distill));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::Probe));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::AddChild));
        assert!(projected.overflow.iter().any(|action| action.id == ActionId::AddParent));
    }

    #[test]
    fn overflow_includes_add_parent_action() {
        let vm = build_action_bar_vm(&row_context());
        assert!(vm.overflow.iter().any(|action| action.id == ActionId::AddParent));
    }

    #[test]
    fn wide_is_identity() {
        let ctx = row_context();
        let vm = build_action_bar_vm(&ctx);
        let original_count = vm.primary.len();

        let projected = project_for_viewport(vm, ViewportBucket::Wide);

        assert_eq!(projected.primary.len(), original_count);
        assert_eq!(projected.primary.len(), 6);
    }

    #[test]
    fn shortcut_command_dot_expands() {
        let key = Key::Character(".".into());
        let modifiers = Modifiers::COMMAND;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Amplify));
    }

    #[test]
    fn shortcut_command_slash_atomizes() {
        let key = Key::Character("/".into());
        let modifiers = Modifiers::COMMAND;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Atomize));
    }

    #[test]
    fn shortcut_command_comma_reduces() {
        let key = Key::Character(",".into());
        let modifiers = Modifiers::COMMAND;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Distill));
    }

    #[test]
    fn shortcut_command_enter_adds_child() {
        let key = Key::Named(Named::Enter);
        let modifiers = Modifiers::COMMAND;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AddChild));
    }

    #[test]
    fn shortcut_command_backspace_is_unbound() {
        let key = Key::Named(Named::Backspace);
        let modifiers = Modifiers::COMMAND;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }

    #[test]
    fn shortcut_ctrl_backspace_is_unbound() {
        let key = Key::Named(Named::Backspace);
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }

    #[test]
    fn shortcut_command_shift_enter_adds_sibling() {
        let key = Key::Named(Named::Enter);
        let modifiers = Modifiers::COMMAND | Modifiers::SHIFT;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AddSibling));
    }

    #[test]
    fn shortcut_command_shift_a_accepts_all() {
        let key = Key::Character("a".into());
        let modifiers = Modifiers::COMMAND | Modifiers::SHIFT;
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
    fn shortcut_ctrl_dot_is_supported() {
        let key = Key::Character(".".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Amplify));
    }

    #[test]
    fn shortcut_unknown_key_returns_none() {
        let key = Key::Character("x".into());
        let modifiers = Modifiers::COMMAND;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }
}
