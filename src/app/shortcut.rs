use super::*;
use crate::store::Direction;

/// Keyboard shortcuts for block focus navigation and structural movement.
///
/// Keymap (Option on macOS, Alt on other platforms):
/// - `Alt+Up` / `Alt+Down`: focus previous/next sibling (wrap at boundaries).
/// - `Alt+Left`: focus parent.
/// - `Alt+Right`: focus first child (if any).
/// - `Alt+Shift+Up` / `Alt+Shift+Down`: move block among siblings (wrap).
/// - `Alt+Shift+Left`: outdent block to be after its parent.
/// - `Alt+Shift+Right`: indent block as first child of previous sibling.
///
/// These shortcuts are document-view operations and are ignored in settings
/// view and pick-friend mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementShortcut {
    FocusSiblingPrevious,
    FocusSiblingNext,
    FocusParent,
    FocusFirstChild,
    MoveSiblingPrevious,
    MoveSiblingNext,
    MoveAfterParent,
    MoveToPreviousSiblingFirstChild,
}

/// Messages for keyboard shortcut dispatch.
#[derive(Debug, Clone)]
pub enum ShortcutMessage {
    Trigger(ActionId),
    ForBlock { block_id: BlockId, action_id: ActionId },
    Movement(MovementShortcut),
}

/// Direction for sibling traversal and reordering helpers.
///
/// Both directions use cyclic (wrap-around) semantics within one sibling
/// slice.
#[derive(Debug, Clone, Copy)]
enum SiblingDirection {
    Previous,
    Next,
}

/// Parse Option/Alt navigation and movement shortcuts from a key press.
///
/// Returns `None` when the key chord is not one of the declared movement
/// shortcuts or when extra command/control modifiers are pressed.
///
/// Design decision: this parser intentionally treats movement shortcuts as
/// global commands, independent of editor widget internals. The edit module
/// filters leaked editor actions so this parser remains the single source
/// of truth for movement dispatch.
pub fn movement_shortcut_from_key(
    key: &keyboard::Key, modifiers: keyboard::Modifiers,
) -> Option<ShortcutMessage> {
    if !modifiers.alt() || modifiers.command() || modifiers.control() {
        return None;
    }

    let shortcut = match key {
        | keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
            if modifiers.shift() {
                MovementShortcut::MoveSiblingPrevious
            } else {
                MovementShortcut::FocusSiblingPrevious
            }
        }
        | keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
            if modifiers.shift() {
                MovementShortcut::MoveSiblingNext
            } else {
                MovementShortcut::FocusSiblingNext
            }
        }
        | keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
            if modifiers.shift() {
                MovementShortcut::MoveAfterParent
            } else {
                MovementShortcut::FocusParent
            }
        }
        | keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
            if modifiers.shift() {
                MovementShortcut::MoveToPreviousSiblingFirstChild
            } else {
                MovementShortcut::FocusFirstChild
            }
        }
        | _ => return None,
    };

    Some(ShortcutMessage::Movement(shortcut))
}

pub fn handle(state: &mut AppState, message: ShortcutMessage) -> Task<Message> {
    match message {
        | ShortcutMessage::Trigger(action_id) => {
            let Some(block_id) = trigger_target_block_id(state) else {
                return Task::none();
            };
            run_shortcut_for_block(state, block_id, action_id)
        }
        | ShortcutMessage::ForBlock { block_id, action_id } => {
            // Don't change focus in PickFriend mode
            if state.ui().document_mode != DocumentMode::PickFriend {
                state.set_focus(block_id);
            }
            run_shortcut_for_block(state, block_id, action_id)
        }
        | ShortcutMessage::Movement(shortcut) => run_movement_shortcut(state, shortcut),
    }
}

/// Resolve the active block target for a global shortcut.
///
/// Priority:
/// 1. Explicit UI focus (`TransientUiState::focus`)
/// 2. Current edit session block (fallback for captured editor paths)
fn trigger_target_block_id(state: &AppState) -> Option<BlockId> {
    state.focus().map(|s| s.block_id).or(state.edit_session)
}

fn sibling_slice<'a>(state: &'a AppState, parent: Option<BlockId>) -> &'a [BlockId] {
    if let Some(parent_id) = parent {
        state.store.children(&parent_id)
    } else {
        state.store.roots()
    }
}

/// Resolve sibling focus target with cyclic wrap-around.
///
/// - Previous from index `0` wraps to the last sibling.
/// - Next from the last sibling wraps to index `0`.
fn sibling_wrap_target(
    state: &AppState, block_id: BlockId, direction: SiblingDirection,
) -> Option<BlockId> {
    let (parent, index) = state.store.parent_and_index_of(&block_id)?;
    let siblings = sibling_slice(state, parent);
    if siblings.is_empty() {
        return None;
    }

    let target_index = match direction {
        | SiblingDirection::Previous => {
            if index == 0 {
                siblings.len().saturating_sub(1)
            } else {
                index - 1
            }
        }
        | SiblingDirection::Next => {
            if index + 1 >= siblings.len() {
                0
            } else {
                index + 1
            }
        }
    };
    siblings.get(target_index).copied()
}

/// Focus a block and keep it visible in both fold and navigation scopes.
///
/// Order matters:
/// 1. unfold collapsed ancestors,
/// 2. reveal navigation path if needed,
/// 3. set focus and request widget focus.
fn focus_block(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    unfold_folded_ancestors_for_focus(state, block_id);

    if !state.navigation.is_in_current_view(&state.store, &block_id) {
        state.navigation.reveal_parent_path(&state.store, &block_id);
    }
    state.set_focus(block_id);
    state.editor_buffers.ensure_block(&state.store, &block_id);
    if let Some(widget_id) = state.editor_buffers.widget_id(&block_id) {
        return widget::operation::focus(widget_id.clone());
    }
    Task::none()
}

/// Ensure the focused target is visible by unfolding collapsed ancestors.
///
/// This is used by movement shortcuts that navigate or move blocks "into"
/// another block. If any ancestor on the target path is folded, it is
/// expanded before focus is applied.
fn unfold_folded_ancestors_for_focus(state: &mut AppState, block_id: BlockId) {
    let mut changed = false;
    let mut cursor = state.store.parent(&block_id);

    while let Some(parent_id) = cursor {
        if state.store.is_collapsed(&parent_id) {
            state.store.toggle_collapsed(&parent_id);
            tracing::info!(
                focused_block_id = ?block_id,
                unfolded_block_id = ?parent_id,
                "unfolded collapsed ancestor for movement shortcut"
            );
            changed = true;
        }
        cursor = state.store.parent(&parent_id);
    }

    if changed {
        state.persist_with_context("after unfolding folded ancestors for movement shortcut");
    }
}

fn focus_sibling(
    state: &mut AppState, block_id: BlockId, direction: SiblingDirection,
) -> Task<Message> {
    let Some(target_id) = sibling_wrap_target(state, block_id, direction) else {
        return Task::none();
    };
    tracing::debug!(from = ?block_id, to = ?target_id, ?direction, "focused sibling by shortcut");
    focus_block(state, target_id)
}

/// Move a block within its sibling list using cyclic semantics.
///
/// Boundary behavior mirrors focus navigation:
/// - Previous on first sibling moves to the end.
/// - Next on last sibling moves to the front.
fn move_block_within_siblings(
    state: &mut AppState, block_id: BlockId, direction: SiblingDirection,
) -> Task<Message> {
    let Some((parent, index)) = state.store.parent_and_index_of(&block_id) else {
        return Task::none();
    };
    let siblings = sibling_slice(state, parent).to_vec();
    if siblings.len() <= 1 {
        return Task::none();
    }

    let (target_id, move_dir) = match direction {
        | SiblingDirection::Previous => {
            if index == 0 {
                (siblings[siblings.len() - 1], Direction::After)
            } else {
                (siblings[index - 1], Direction::Before)
            }
        }
        | SiblingDirection::Next => {
            if index + 1 >= siblings.len() {
                (siblings[0], Direction::Before)
            } else {
                (siblings[index + 1], Direction::After)
            }
        }
    };

    state.mutate_with_undo_and_persist("after moving block within siblings by shortcut", |state| {
        if state.store.move_block(&block_id, &target_id, move_dir).is_some() {
            tracing::info!(block_id = ?block_id, target_id = ?target_id, ?move_dir, ?direction, "moved block within siblings by shortcut");
            true
        } else {
            false
        }
    });
    focus_block(state, block_id)
}

fn move_block_after_parent(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    let Some(parent_id) = state.store.parent(&block_id) else {
        return Task::none();
    };

    state.mutate_with_undo_and_persist("after outdenting block by shortcut", |state| {
        if state.store.move_block(&block_id, &parent_id, Direction::After).is_some() {
            tracing::info!(block_id = ?block_id, parent_id = ?parent_id, "outdented block after parent by shortcut");
            true
        } else {
            false
        }
    });
    focus_block(state, block_id)
}

fn move_block_to_previous_sibling_first_child(
    state: &mut AppState, block_id: BlockId,
) -> Task<Message> {
    let Some((parent, index)) = state.store.parent_and_index_of(&block_id) else {
        return Task::none();
    };
    if index == 0 {
        return Task::none();
    }
    let siblings = sibling_slice(state, parent);
    let previous_sibling_id = siblings[index - 1];
    let first_child_of_previous = state.store.children(&previous_sibling_id).first().copied();

    let (target_id, move_dir) = if let Some(first_child_id) = first_child_of_previous {
        (first_child_id, Direction::Before)
    } else {
        (previous_sibling_id, Direction::Under)
    };

    state.mutate_with_undo_and_persist("after indenting block by shortcut", |state| {
        if state.store.move_block(&block_id, &target_id, move_dir).is_some() {
            tracing::info!(
                block_id = ?block_id,
                target_id = ?target_id,
                previous_sibling_id = ?previous_sibling_id,
                ?move_dir,
                "indented block into previous sibling by shortcut"
            );
            true
        } else {
            false
        }
    });
    focus_block(state, block_id)
}

fn run_movement_shortcut(state: &mut AppState, shortcut: MovementShortcut) -> Task<Message> {
    if state.ui().active_view != ViewMode::Document
        || state.ui().document_mode != DocumentMode::Normal
    {
        return Task::none();
    }

    let Some(block_id) = trigger_target_block_id(state) else {
        return Task::none();
    };

    match shortcut {
        | MovementShortcut::FocusSiblingPrevious => {
            focus_sibling(state, block_id, SiblingDirection::Previous)
        }
        | MovementShortcut::FocusSiblingNext => {
            focus_sibling(state, block_id, SiblingDirection::Next)
        }
        | MovementShortcut::FocusParent => {
            let Some(parent_id) = state.store.parent(&block_id) else {
                return Task::none();
            };
            tracing::debug!(from = ?block_id, to = ?parent_id, "focused parent by shortcut");
            focus_block(state, parent_id)
        }
        | MovementShortcut::FocusFirstChild => {
            let Some(child_id) = state.store.children(&block_id).first().copied() else {
                return Task::none();
            };
            tracing::debug!(from = ?block_id, to = ?child_id, "focused first child by shortcut");
            focus_block(state, child_id)
        }
        | MovementShortcut::MoveSiblingPrevious => {
            move_block_within_siblings(state, block_id, SiblingDirection::Previous)
        }
        | MovementShortcut::MoveSiblingNext => {
            move_block_within_siblings(state, block_id, SiblingDirection::Next)
        }
        | MovementShortcut::MoveAfterParent => move_block_after_parent(state, block_id),
        | MovementShortcut::MoveToPreviousSiblingFirstChild => {
            move_block_to_previous_sibling_first_child(state, block_id)
        }
    }
}

fn run_shortcut_for_block(
    state: &mut AppState, block_id: BlockId, action_id: ActionId,
) -> Task<Message> {
    let point_text =
        state.editor_buffers.get(&block_id).map(text_editor::Content::text).unwrap_or_default();
    let expansion_draft = state.store.expansion_draft(&block_id);
    let atomization_draft = state.store.atomization_draft(&block_id);
    let reduction_draft = state.store.reduction_draft(&block_id);
    let row_context = RowContext {
        block_id,
        point_text,
        has_draft: expansion_draft.is_some()
            || atomization_draft.is_some()
            || reduction_draft.is_some(),
        draft_suggestion_count: expansion_draft.map(|d| d.children.len()).unwrap_or(0)
            + atomization_draft.map(|d| d.points.len()).unwrap_or(0)
            + reduction_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
        has_expand_error: state.llm_requests.has_expand_error(block_id),
        has_reduce_error: state.llm_requests.has_reduce_error(block_id),
        has_atomize_error: state.llm_requests.has_atomize_error(block_id),
        is_expanding: state.llm_requests.is_expanding(block_id),
        is_reducing: state.llm_requests.is_reducing(block_id),
        is_atomizing: state.llm_requests.is_atomizing(block_id),
        is_mounted: state.store.mount_table().entry(block_id).is_some(),
        has_children: !state.store.children(&block_id).is_empty(),
        is_unexpanded_mount: state.store.node(&block_id).is_some_and(|n| n.mount_path().is_some()),
    };
    let vm = project_for_viewport(build_action_bar_vm(&row_context), ViewportBucket::Wide);

    let is_enabled = vm
        .primary
        .iter()
        .chain(vm.contextual.iter())
        .chain(vm.overflow.iter())
        .find(|item| item.id == action_id)
        .is_some_and(|descriptor| descriptor.availability == ActionAvailability::Enabled);

    if is_enabled && let Some(next) = action_to_message_by_id(state, &block_id, action_id) {
        return AppState::update(state, next);
    }

    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_uses_edit_session_when_focus_is_missing() {
        let (mut state, root) = AppState::test_state();
        assert!(state.focus().is_none());
        state.edit_session = Some(root);

        let _ = handle(&mut state, ShortcutMessage::Trigger(ActionId::Expand));

        assert!(state.llm_requests.is_expanding(root));
    }

    #[test]
    fn alt_arrow_shortcuts_map_to_movement_commands() {
        let modifiers = keyboard::Modifiers::ALT;
        let up = movement_shortcut_from_key(
            &keyboard::Key::Named(keyboard::key::Named::ArrowUp),
            modifiers,
        );
        let left = movement_shortcut_from_key(
            &keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            modifiers,
        );
        assert!(matches!(
            up,
            Some(ShortcutMessage::Movement(MovementShortcut::FocusSiblingPrevious))
        ));
        assert!(matches!(left, Some(ShortcutMessage::Movement(MovementShortcut::FocusParent))));
    }

    #[test]
    fn alt_shift_arrow_shortcuts_map_to_move_commands() {
        let modifiers = keyboard::Modifiers::ALT | keyboard::Modifiers::SHIFT;
        let down = movement_shortcut_from_key(
            &keyboard::Key::Named(keyboard::key::Named::ArrowDown),
            modifiers,
        );
        let right = movement_shortcut_from_key(
            &keyboard::Key::Named(keyboard::key::Named::ArrowRight),
            modifiers,
        );
        assert!(matches!(down, Some(ShortcutMessage::Movement(MovementShortcut::MoveSiblingNext))));
        assert!(matches!(
            right,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
    }

    #[test]
    fn focus_sibling_previous_wraps_within_level() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");
        state.set_focus(root);

        let _ =
            handle(&mut state, ShortcutMessage::Movement(MovementShortcut::FocusSiblingPrevious));

        assert_eq!(state.focus().map(|focus| focus.block_id), Some(sibling));
    }

    #[test]
    fn move_sibling_previous_wraps_within_level() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");
        state.set_focus(root);

        let _ =
            handle(&mut state, ShortcutMessage::Movement(MovementShortcut::MoveSiblingPrevious));

        assert_eq!(state.store.roots(), &[sibling, root]);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn move_after_parent_outdents_block() {
        let (mut state, root) = AppState::test_state();
        let child =
            state.store.append_child(&root, "child".to_string()).expect("append child succeeds");
        state.set_focus(child);

        let _ = handle(&mut state, ShortcutMessage::Movement(MovementShortcut::MoveAfterParent));

        assert_eq!(state.store.parent(&child), None);
        assert_eq!(state.store.roots(), &[root, child]);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn move_to_previous_sibling_first_child_inserts_as_first_child() {
        let (mut state, root) = AppState::test_state();
        let first = state
            .store
            .append_child(&root, "first".to_string())
            .expect("append first child succeeds");
        let second = state
            .store
            .append_sibling(&first, "second".to_string())
            .expect("append second child succeeds");
        let existing = state
            .store
            .append_child(&first, "existing".to_string())
            .expect("append existing grandchild succeeds");
        state.set_focus(second);

        let _ = handle(
            &mut state,
            ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild),
        );

        assert_eq!(state.store.parent(&second), Some(first));
        let first_children = state.store.children(&first);
        assert_eq!(first_children.first().copied(), Some(second));
        assert!(first_children.contains(&existing));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(second));
    }

    #[test]
    fn focus_first_child_unfolds_current_block() {
        let (mut state, root) = AppState::test_state();
        let child =
            state.store.append_child(&root, "child".to_string()).expect("append child succeeds");
        state.store.toggle_collapsed(&root);
        state.set_focus(root);

        let _ = handle(&mut state, ShortcutMessage::Movement(MovementShortcut::FocusFirstChild));

        assert!(!state.store.is_collapsed(&root));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn indent_into_previous_sibling_unfolds_target_parent() {
        let (mut state, root) = AppState::test_state();
        let first = state
            .store
            .append_child(&root, "first".to_string())
            .expect("append first child succeeds");
        let second = state
            .store
            .append_sibling(&first, "second".to_string())
            .expect("append second child succeeds");
        state.store.toggle_collapsed(&first);
        state.set_focus(second);

        let _ = handle(
            &mut state,
            ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild),
        );

        assert!(!state.store.is_collapsed(&first));
        assert_eq!(state.store.parent(&second), Some(first));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(second));
    }
}
