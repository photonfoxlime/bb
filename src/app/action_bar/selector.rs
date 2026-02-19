use super::types::{
    ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, ActionPriority, RowContext,
    RowUiState, StatusChipVm,
};

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
