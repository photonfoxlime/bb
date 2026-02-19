mod dispatch;
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
    use crate::graph::BlockId;
    use types::{ActionPriority, RowUiState};

    fn row_context() -> RowContext {
        RowContext {
            block_id: BlockId::new(),
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
        assert!(!visible.iter().any(|action| action.id == ActionId::Overflow));
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
        use iced::keyboard::{Key, Modifiers};
        let key = Key::Character(".".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Expand));
    }

    #[test]
    fn shortcut_ctrl_comma_reduces() {
        use iced::keyboard::{Key, Modifiers};
        let key = Key::Character(",".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::Reduce));
    }

    #[test]
    fn shortcut_ctrl_enter_adds_child() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        let key = Key::Named(Named::Enter);
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AddChild));
    }

    #[test]
    fn shortcut_ctrl_backspace_archives() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        let key = Key::Named(Named::Backspace);
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::ArchiveBlock));
    }

    #[test]
    fn shortcut_ctrl_shift_enter_adds_sibling() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        let key = Key::Named(Named::Enter);
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AddSibling));
    }

    #[test]
    fn shortcut_ctrl_shift_a_accepts_all() {
        use iced::keyboard::{Key, Modifiers};
        let key = Key::Character("a".into());
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, Some(ActionId::AcceptAll));
    }

    #[test]
    fn shortcut_no_modifier_returns_none() {
        use iced::keyboard::{Key, Modifiers};
        let key = Key::Character(".".into());
        let modifiers = Modifiers::empty();
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }

    #[test]
    fn shortcut_unknown_key_returns_none() {
        use iced::keyboard::{Key, Modifiers};
        let key = Key::Character("x".into());
        let modifiers = Modifiers::CTRL;
        let action = shortcut_to_action(key, modifiers);
        assert_eq!(action, None);
    }
}
