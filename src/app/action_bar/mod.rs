mod dispatch;
#[allow(dead_code)]
mod keyboard;
mod responsive;
mod selector;
mod types;

pub use dispatch::{action_to_message, action_to_message_by_id};
pub use keyboard::shortcut_to_action;
pub use responsive::{ViewportBucket, project_for_viewport};
pub use selector::build_action_bar_vm;
pub use types::{
    ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, RowContext, StatusChipVm,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::BlockId;
    use uuid::Uuid;

    fn row_context() -> RowContext {
        RowContext {
            block_id: BlockId(Uuid::new_v4()),
            point_text: "hello".to_string(),
            has_draft: false,
            draft_suggestion_count: 0,
            has_expand_error: false,
            has_reduce_error: false,
            is_expanding: false,
            is_reducing: false,
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
}
