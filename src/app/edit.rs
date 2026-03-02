use super::*;

/// Messages for point text editing.
#[derive(Debug, Clone)]
pub enum EditMessage {
    PointEdited {
        block_id: BlockId,
        action: text_editor::Action,
    },
    /// Move cursor by one token in the current line.
    ///
    /// This powers `Cmd/Ctrl+ArrowLeft/ArrowRight` behavior in point
    /// editors, using cached tokenizer spans for mixed-language text.
    MoveCursorByWord {
        block_id: BlockId,
        direction: WordCursorDirection,
    },
    /// Insert an empty first child for `block_id`.
    ///
    /// Used by `Cmd/Ctrl+Enter` key binding so shortcut behavior does not
    /// depend on the async keyboard-modifier subscription timing.
    AddEmptyFirstChild {
        block_id: BlockId,
    },
}

/// Handle a point-editing message.
pub fn handle(state: &mut AppState, message: EditMessage) -> Task<Message> {
    match message {
        | EditMessage::PointEdited { block_id, action } => {
            handle_point_edited(state, block_id, action)
        }
        | EditMessage::MoveCursorByWord { block_id, direction } => {
            move_cursor_by_word(state, block_id, direction)
        }
        | EditMessage::AddEmptyFirstChild { block_id } => {
            add_empty_first_child_from_enter(state, block_id)
        }
    }
}

/// Direction tag for vertical cursor movement edge-detection.
///
/// Used to defer block traversal until *after* the editor processes
/// the motion, so wrapped (visual) lines are handled correctly.
enum VerticalDir {
    Up,
    Down,
}

/// Horizontal cursor movement direction for word-step shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordCursorDirection {
    Left,
    Right,
}

fn move_cursor_by_word(
    state: &mut AppState, block_id: BlockId, direction: WordCursorDirection,
) -> Task<Message> {
    if state.ui().document_mode == DocumentMode::PickFriend {
        return Task::none();
    }

    state.set_focus(block_id);
    state.editor_buffers.ensure_block(&state.store, &block_id);

    let Some((line_index, current_column_byte, line_text)) =
        state.editor_buffers.get(&block_id).and_then(|content| {
            let cursor = content.cursor().position;
            content
                .line(cursor.line)
                .map(|line| (cursor.line, cursor.column, line.text.to_string()))
        })
    else {
        return Task::none();
    };

    // Convert cursor.column from byte offset to char offset.
    // iced's text_editor uses byte offsets for cursor position,
    // but our token spans use char offsets.
    let current_column = line_text[..current_column_byte.min(line_text.len())].chars().count();

    let spans = state.editor_buffers.word_token_spans_for_line(&block_id, &line_text);
    let line_char_count = line_text.chars().count();
    let next_column = next_word_cursor_column(current_column, line_char_count, &spans, direction);

    if next_column == current_column {
        tracing::debug!(
            block_id = ?block_id,
            current_column = current_column,
            ?direction,
            "no word boundary to move to"
        );
        return Task::none();
    }

    if let Some(content) = state.editor_buffers.get_mut(&block_id) {
        // Convert char offset back to byte offset for the editor.
        let next_column_byte =
            line_text.char_indices().nth(next_column).map(|(i, _)| i).unwrap_or(line_text.len());

        content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: line_index, column: next_column_byte },
            selection: None,
        });
    }
    Task::none()
}

fn next_word_cursor_column(
    current_column: usize, line_char_count: usize, spans: &[crate::text::WordTokenSpan],
    direction: WordCursorDirection,
) -> usize {
    match direction {
        | WordCursorDirection::Left => {
            let mut target = 0usize;
            for span in spans {
                if current_column <= span.start {
                    break;
                }
                if current_column <= span.end {
                    return span.start;
                }
                target = span.start;
            }
            target
        }
        | WordCursorDirection::Right => {
            for span in spans {
                if current_column < span.start {
                    return span.start;
                }
                if current_column < span.end {
                    return span.end;
                }
            }
            line_char_count
        }
    }
}

fn is_shortcut_modifier(modifiers: keyboard::Modifiers) -> bool {
    // Keep this aligned with `action_bar::shortcut_to_action`: some
    // text-editor input paths may surface the Command key via `control()`.
    modifiers.command() || modifiers.control()
}

fn command_shortcut_action_from_editor_insert(
    action: &text_editor::Action, modifiers: keyboard::Modifiers,
) -> Option<ActionId> {
    if !is_shortcut_modifier(modifiers) {
        return None;
    }

    match action {
        | text_editor::Action::Edit(text_editor::Edit::Insert('.')) => Some(ActionId::Expand),
        | text_editor::Action::Edit(text_editor::Edit::Insert(',')) => Some(ActionId::Reduce),
        | _ => None,
    }
}

fn is_command_shortcut_editor_insert(
    action: &text_editor::Action, modifiers: keyboard::Modifiers,
) -> bool {
    if !is_shortcut_modifier(modifiers) {
        return false;
    }

    matches!(
        action,
        text_editor::Action::Edit(text_editor::Edit::Insert(c))
            if matches!(c.to_ascii_lowercase(), 'f' | 'g' | 'z' | '.' | ',')
    )
}

/// Detect editor actions leaked from `Alt/Option + Arrow` key chords.
///
/// Design decision: movement shortcuts are handled in the global keyboard
/// subscription path so behavior is consistent across focused widgets. Some
/// backends still emit editor `Move`/`Select` actions for the same key
/// press; those leaked actions must be ignored here to avoid double
/// execution (for example, sibling focus wrapping then immediately moving
/// again).
fn is_alt_movement_shortcut_editor_action(
    action: &text_editor::Action, modifiers: keyboard::Modifiers,
) -> bool {
    if !modifiers.alt() || modifiers.command() || modifiers.control() {
        return false;
    }

    matches!(
        action,
        text_editor::Action::Move(
            text_editor::Motion::Up
                | text_editor::Motion::Down
                | text_editor::Motion::Left
                | text_editor::Motion::Right
                | text_editor::Motion::WordLeft
                | text_editor::Motion::WordRight
        ) | text_editor::Action::Select(
            text_editor::Motion::Up
                | text_editor::Motion::Down
                | text_editor::Motion::Left
                | text_editor::Motion::Right
                | text_editor::Motion::WordLeft
                | text_editor::Motion::WordRight
        )
    )
}

/// Returns whether the cursor is at the end of a one-line point.
fn is_cursor_at_end_of_only_line(content: &text_editor::Content) -> bool {
    if content.line_count() != 1 {
        return false;
    }

    let cursor = content.cursor().position;
    if cursor.line != 0 {
        return false;
    }

    content.line(0).is_some_and(|line| cursor.column >= line.text.chars().count())
}

/// Whether plain Enter should create a new child at index 0.
///
/// Design decision:
/// - `Cmd/Ctrl+Enter` is handled by a dedicated custom edit message in the
///   key-binding layer.
/// - Plain `Enter` keeps normal multiline editing semantics by default, and
///   only inserts a child at index 0 when
///   `AppConfig::first_line_enter_add_child` is enabled and the cursor is
///   at the end of the only line.
fn should_add_first_child_on_enter(
    state: &AppState, block_id: BlockId, action: &text_editor::Action,
) -> bool {
    if !matches!(action, text_editor::Action::Edit(text_editor::Edit::Enter)) {
        return false;
    }

    let modifiers = state.ui().keyboard_modifiers;
    if modifiers.shift() || modifiers.alt() {
        return false;
    }

    if modifiers.command() || modifiers.control() {
        return false;
    }

    if !state.config.first_line_enter_add_child {
        return false;
    }

    let Some(content) = state.editor_buffers.get(&block_id) else {
        return false;
    };

    is_cursor_at_end_of_only_line(content)
}

/// Insert an empty child block at index 0 for `block_id`.
///
/// Existing point text is left unchanged; the new child is focused with the
/// cursor at the start of its empty text.
fn add_empty_first_child_from_enter(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    state.ui_mut().hovered_friend_block = None;

    if state.ui().document_mode == DocumentMode::PickFriend {
        return Task::none();
    }

    state.set_focus(block_id);
    state.editor_buffers.ensure_block(&state.store, &block_id);

    if state.edit_session.as_ref() != Some(&block_id) {
        state.snapshot_for_undo();
        state.edit_session = Some(block_id);
    }

    let previous_first_child = state.store.children(&block_id).first().copied();

    let Some(child_id) = state.store.append_child(&block_id, String::new()) else {
        tracing::error!(block_id = ?block_id, "failed to append child while handling enter");
        return Task::none();
    };

    if let Some(first_child_id) = previous_first_child {
        let moved =
            state.store.move_block(&child_id, &first_child_id, crate::store::Direction::Before);
        if moved.is_none() {
            tracing::error!(
                block_id = ?block_id,
                child_id = ?child_id,
                first_child_id = ?first_child_id,
                "failed to move enter-created child to index 0"
            );
        }
    }

    state.editor_buffers.set_text(&child_id, "");
    if let Some(child_content) = state.editor_buffers.get_mut(&child_id) {
        child_content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 0, column: 0 },
            selection: None,
        });
    }

    state.set_overflow_open(false);
    state.persist_with_context("after adding first child from enter");
    tracing::info!(
        block_id = ?block_id,
        child_id = ?child_id,
        command_shortcut = is_shortcut_modifier(state.ui().keyboard_modifiers),
        "inserted empty first child from enter"
    );

    state.set_focus(child_id);
    state.edit_session = None;

    if let Some(widget_id) = state.editor_buffers.widget_id(&child_id) {
        return widget::operation::focus(widget_id.clone());
    }

    Task::none()
}

fn is_plain_backspace_action(action: &text_editor::Action, modifiers: keyboard::Modifiers) -> bool {
    if modifiers.shift() || modifiers.alt() || modifiers.command() || modifiers.control() {
        return false;
    }

    matches!(action, text_editor::Action::Edit(text_editor::Edit::Backspace))
}

fn should_enter_multiselect_on_backspace(
    state: &AppState, block_id: BlockId, action: &text_editor::Action,
) -> bool {
    if state.ui().document_mode != DocumentMode::Normal {
        return false;
    }

    if !is_plain_backspace_action(action, state.ui().keyboard_modifiers) {
        return false;
    }

    let text = state
        .editor_buffers
        .get(&block_id)
        .map(text_editor::Content::text)
        .or_else(|| state.store.point(&block_id))
        .unwrap_or_default();

    text.is_empty()
}

fn previous_visible_in_current_navigation_view(
    state: &AppState, block_id: BlockId,
) -> Option<BlockId> {
    let mut current = Some(block_id);

    while let Some(cursor) = current {
        let previous = state.store.prev_visible_in_dfs(&cursor)?;
        if state.navigation.is_in_current_view(&state.store, &previous) {
            return Some(previous);
        }
        current = Some(previous);
    }

    None
}

fn selected_blocks_without_selected_ancestors(
    state: &AppState, selected: &BTreeSet<BlockId>,
) -> Vec<BlockId> {
    selected
        .iter()
        .copied()
        .filter(|block_id| state.store.node(block_id).is_some())
        .filter(|block_id| {
            let mut parent = state.store.parent(block_id);
            while let Some(parent_id) = parent {
                if selected.contains(&parent_id) {
                    return false;
                }
                parent = state.store.parent(&parent_id);
            }
            true
        })
        .collect()
}

/// Delete multiselect targets when Backspace is pressed in multiselect mode.
///
/// Design decision:
/// - The selection is normalized to top-most blocks (selected descendants of
///   already-selected ancestors are ignored) so each subtree is deleted once.
/// - For single-block delete, focus moves to the previous visible block in
///   the current navigation view, matching the keyboard traversal direction.
/// - The app exits multiselect mode after deletion completes.
fn delete_multiselect_selection_on_backspace(
    state: &mut AppState, block_id: BlockId,
) -> Task<Message> {
    let mut selected = state.ui().multiselect_selected_blocks.clone();
    if selected.is_empty() {
        selected.insert(block_id);
    }

    let selected_roots = selected_blocks_without_selected_ancestors(state, &selected);
    if selected_roots.is_empty() {
        tracing::error!("multiselect delete requested without valid selection");
        return Task::none();
    }

    let focus_after_delete = if selected_roots.len() == 1 {
        previous_visible_in_current_navigation_view(state, selected_roots[0])
    } else {
        None
    };

    state.snapshot_for_undo();

    let mut removed_ids = Vec::new();
    for selected_id in selected_roots {
        if let Some(removed) = state.store.remove_block_subtree(&selected_id) {
            for id in &removed {
                state.llm_requests.remove_block(*id);
            }
            removed_ids.extend(removed);
        }
    }

    if removed_ids.is_empty() {
        tracing::error!("multiselect delete removed no blocks");
        return Task::none();
    }

    state.editor_buffers.remove_blocks(&removed_ids);
    for root_id in state.store.roots() {
        state.editor_buffers.ensure_block(&state.store, root_id);
    }
    state.persist_with_context("after deleting multiselect selection");
    tracing::info!(removed = removed_ids.len(), "deleted multiselect selection");

    state.ui_mut().document_mode = DocumentMode::Normal;
    state.ui_mut().multiselect_selected_blocks.clear();
    state.edit_session = None;

    if let Some(next_focus) = focus_after_delete
        && state.store.node(&next_focus).is_some()
    {
        state.set_focus(next_focus);
        if let Some(widget_id) = state.editor_buffers.widget_id(&next_focus) {
            return widget::operation::focus(widget_id.clone());
        }
        return Task::none();
    }

    state.clear_focus();
    Task::none()
}

/// Handle one text-editor action for a point block.
///
/// Enter behavior contract:
/// - Plain `Enter` uses normal editor newline behavior, except when
///   `first-line-enter-add-child` is enabled and the cursor is at the end
///   of the only line (then insert empty first child).
/// - `Cmd/Ctrl+Enter` is handled by
///   [`EditMessage::AddEmptyFirstChild`], dispatched from document key
///   binding.
/// - `Cmd/Ctrl+Shift+Enter` is dispatched as `ActionId::AddSibling` from
///   document key binding.
/// - Plain `Backspace` on an empty point enters multiselect mode and selects
///   the focused block.
/// - Plain `Backspace` in multiselect mode deletes current selection and
///   returns to normal mode.
pub fn handle_point_edited(
    state: &mut AppState, block_id: BlockId, action: text_editor::Action,
) -> Task<Message> {
    // Clear friend hover state when editing
    state.ui_mut().hovered_friend_block = None;

    if let Some(action_id) =
        command_shortcut_action_from_editor_insert(&action, state.ui().keyboard_modifiers)
    {
        // Keep app-level block focus aligned with the active editor and run
        // the shortcut with an explicit block target. This avoids reliance
        // on global focus synchronization order for command+punctuation.
        if state.ui().document_mode == DocumentMode::Normal {
            state.set_focus(block_id);
        }
        return AppState::update(
            state,
            Message::Shortcut(ShortcutMessage::ForBlock { block_id, action_id }),
        );
    }

    if is_command_shortcut_editor_insert(&action, state.ui().keyboard_modifiers) {
        // Keep app-level block focus aligned with the active editor even when
        // the insert action is ignored as a leaked command shortcut.
        if state.ui().document_mode == DocumentMode::Normal {
            state.set_focus(block_id);
        }
        tracing::debug!("ignored command-shortcut editor insert leak");
        return Task::none();
    }

    if is_alt_movement_shortcut_editor_action(&action, state.ui().keyboard_modifiers) {
        // Option/Alt arrow shortcuts are handled by the global subscription
        // path. Ignore editor cursor-motion actions here to avoid handling
        // the same key chord twice.
        tracing::debug!("ignored alt-movement editor action leak");
        return Task::none();
    }

    if state.ui().document_mode == DocumentMode::Multiselect
        && is_plain_backspace_action(&action, state.ui().keyboard_modifiers)
    {
        return delete_multiselect_selection_on_backspace(state, block_id);
    }

    if should_enter_multiselect_on_backspace(state, block_id, &action) {
        state.ui_mut().document_mode = DocumentMode::Multiselect;
        state.set_focus(block_id);
        tracing::info!(block_id = ?block_id, "entered multiselect mode from empty backspace");
        return Task::none();
    }

    // Don't change focus in PickFriend mode
    if state.ui().document_mode == DocumentMode::PickFriend {
        return Task::none();
    }

    if should_add_first_child_on_enter(state, block_id, &action) {
        return add_empty_first_child_from_enter(state, block_id);
    }

    state.set_focus(block_id);
    if state.edit_session.as_ref() != Some(&block_id) {
        state.snapshot_for_undo();
        state.edit_session = Some(block_id);
    }
    state.editor_buffers.ensure_block(&state.store, &block_id);

    let vertical_direction = match &action {
        | text_editor::Action::Move(text_editor::Motion::Up) => Some(VerticalDir::Up),
        | text_editor::Action::Move(text_editor::Motion::Down) => Some(VerticalDir::Down),
        | _ => None,
    };

    let mut navigate_to: Option<BlockId> = None;
    if let Some(content) = state.editor_buffers.get_mut(&block_id) {
        let cursor_before = content.cursor().position;
        content.perform(action);
        let cursor_after = content.cursor().position;

        if let Some(dir) = vertical_direction
            && cursor_before == cursor_after
        {
            navigate_to = match dir {
                | VerticalDir::Up => state.store.prev_visible_in_dfs(&block_id),
                | VerticalDir::Down => state.store.next_visible_in_dfs(&block_id),
            };
        }

        if navigate_to.is_none() {
            let next_text = content.text();
            tracing::debug!(block_id = ?block_id, chars = next_text.len(), "point edited");
            state.store.update_point(&block_id, next_text);
            state.persist_with_context("after edit");
            state.editor_buffers.invalidate_token_cache(&block_id);
        }
    }

    if let Some(target_id) = navigate_to
        && let Some(wid) = state.editor_buffers.widget_id(&target_id)
    {
        // Only change focus in Normal mode
        if state.ui().document_mode == DocumentMode::Normal {
            let wid_clone = wid.clone();
            state.set_focus(target_id);
            tracing::debug!(
                from = ?block_id,
                to = ?target_id,
                "keyboard traversal"
            );
            return widget::operation::focus(wid_clone);
        }
    }
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_shortcut_insert_keeps_focus_in_sync() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('.')),
        );

        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn command_dot_insert_triggers_expand_for_block() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('.')),
        );

        assert!(state.llm_requests.is_expanding(root));
    }

    #[test]
    fn command_comma_insert_triggers_reduce_for_block() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;
        state.store.update_point(&root, "needs reduce".to_string());
        state.editor_buffers.set_text(&root, "needs reduce");

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert(',')),
        );

        assert!(state.llm_requests.is_reducing(root));
    }

    #[test]
    fn ctrl_dot_insert_triggers_expand_for_block() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::CTRL;

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('.')),
        );

        assert!(state.llm_requests.is_expanding(root));
    }

    #[test]
    fn move_cursor_by_word_uses_latin_token_boundaries() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "alpha,beta".to_string());
        state.editor_buffers.set_text(&root, "alpha,beta");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 0 },
                selection: None,
            });
        }

        let _ = handle(
            &mut state,
            EditMessage::MoveCursorByWord { block_id: root, direction: WordCursorDirection::Right },
        );

        let cursor =
            state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
        assert_eq!(cursor.column, 5);
    }

    #[test]
    fn move_cursor_by_word_splits_han_characters() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "中文 ab".to_string());
        state.editor_buffers.set_text(&root, "中文 ab");
        // "中" (0-3), "文" (3-6), "a" (6-7), "b" (7-8)
        // Set cursor at byte 8 (after "b", which is char position 4)
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 8 },
                selection: None,
            });
        }

        let _ = handle(
            &mut state,
            EditMessage::MoveCursorByWord { block_id: root, direction: WordCursorDirection::Left },
        );

        // Han characters are tokenized individually, so moving left from char 4 ("b")
        // goes to char 3 ("a"), which is byte 7.
        let cursor =
            state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
        assert_eq!(cursor.column, 7);
    }

    #[test]
    fn enter_at_end_of_only_line_inserts_empty_first_child_when_enabled() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "hello".to_string());
        let existing =
            state.store.append_child(&root, "existing".to_string()).expect("append child succeeds");
        state.editor_buffers.set_text(&root, "hello");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 5 },
                selection: None,
            });
        }

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Enter),
        );

        let children = state.store.children(&root);
        assert_eq!(children.len(), 2);
        let child = children[0];
        assert_eq!(state.store.point(&root).as_deref(), Some("hello"));
        assert_eq!(state.store.point(&child).as_deref(), Some(""));
        assert_eq!(children[1], existing);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn enter_in_middle_of_line_keeps_edit_in_place() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "abcd".to_string());
        state.editor_buffers.set_text(&root, "abcd");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 2 },
                selection: None,
            });
        }

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Enter),
        );

        assert!(state.store.children(&root).is_empty());
        assert_eq!(state.store.point(&root).as_deref(), Some("ab\ncd"));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn enter_on_multi_line_point_inserts_newline() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "ab\ncd".to_string());
        state.editor_buffers.set_text(&root, "ab\ncd");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 1 },
                selection: None,
            });
        }

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Enter),
        );

        assert!(state.store.children(&root).is_empty());
        assert_eq!(state.store.point(&root).as_deref(), Some("ab\nc\nd"));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn enter_at_end_of_only_line_inserts_newline_when_disabled() {
        let (mut state, root) = AppState::test_state();
        state.config.first_line_enter_add_child = false;
        state.store.update_point(&root, "hello".to_string());
        state.editor_buffers.set_text(&root, "hello");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 5 },
                selection: None,
            });
        }

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Enter),
        );

        assert!(state.store.children(&root).is_empty());
        assert_eq!(state.store.point(&root).as_deref(), Some("hello\n"));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn enter_in_middle_of_only_line_ignores_stale_command_modifier() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "abcd".to_string());
        state.editor_buffers.set_text(&root, "abcd");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 2 },
                selection: None,
            });
        }
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Enter),
        );

        assert!(state.store.children(&root).is_empty());
        assert_eq!(state.store.point(&root).as_deref(), Some("ab\ncd"));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn command_enter_inserts_empty_first_child_without_splitting_point() {
        let (mut state, root) = AppState::test_state();
        state.config.first_line_enter_add_child = false;
        state.store.update_point(&root, "abcdef".to_string());
        let existing =
            state.store.append_child(&root, "existing".to_string()).expect("append child succeeds");
        state.editor_buffers.set_text(&root, "abcdef");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 2 },
                selection: None,
            });
        }

        let _ = handle(&mut state, EditMessage::AddEmptyFirstChild { block_id: root });

        let children = state.store.children(&root);
        assert_eq!(children.len(), 2);
        let child = children[0];
        assert_eq!(state.store.point(&root).as_deref(), Some("abcdef"));
        assert_eq!(state.store.point(&child).as_deref(), Some(""));
        assert_eq!(children[1], existing);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn command_enter_inserts_empty_child_for_empty_point() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, String::new());
        state.editor_buffers.set_text(&root, "");

        let _ = handle(&mut state, EditMessage::AddEmptyFirstChild { block_id: root });

        let children = state.store.children(&root);
        assert_eq!(children.len(), 1);
        let child = children[0];
        assert_eq!(state.store.point(&root).as_deref(), Some(""));
        assert_eq!(state.store.point(&child).as_deref(), Some(""));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn empty_backspace_enters_multiselect_and_selects_block() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, String::new());
        state.editor_buffers.set_text(&root, "");

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Backspace),
        );

        assert_eq!(state.ui().document_mode, DocumentMode::Multiselect);
        assert!(state.ui().multiselect_selected_blocks.contains(&root));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn multiselect_backspace_single_delete_focuses_previous_visible_block() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "first".to_string());
        let sibling = state
            .store
            .append_sibling(&root, "second".to_string())
            .expect("append sibling succeeds");
        state.editor_buffers.ensure_subtree(&state.store, &root);

        state.ui_mut().document_mode = DocumentMode::Multiselect;
        state.set_focus(sibling);

        let _ = handle_point_edited(
            &mut state,
            sibling,
            text_editor::Action::Edit(text_editor::Edit::Backspace),
        );

        assert!(state.store.node(&sibling).is_none());
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
        assert_eq!(state.ui().document_mode, DocumentMode::Normal);
        assert!(state.ui().multiselect_selected_blocks.is_empty());
    }

    #[test]
    fn multiselect_backspace_deletes_all_selected_blocks() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "second".to_string())
            .expect("append sibling succeeds");
        state.editor_buffers.ensure_subtree(&state.store, &root);

        state.ui_mut().document_mode = DocumentMode::Multiselect;
        state.ui_mut().multiselect_selected_blocks.insert(root);
        state.ui_mut().multiselect_selected_blocks.insert(sibling);
        state.set_focus(sibling);
        state.ui_mut().multiselect_selected_blocks.insert(root);

        let _ = handle_point_edited(
            &mut state,
            sibling,
            text_editor::Action::Edit(text_editor::Edit::Backspace),
        );

        assert!(state.store.node(&root).is_none());
        assert!(state.store.node(&sibling).is_none());
        assert_eq!(state.ui().document_mode, DocumentMode::Normal);
        assert!(state.focus().is_none());
        assert!(state.ui().multiselect_selected_blocks.is_empty());
    }

    #[test]
    fn alt_up_editor_motion_is_ignored_to_prevent_double_navigation() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");
        state.set_focus(sibling);
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::ALT;

        let _ = handle_point_edited(
            &mut state,
            sibling,
            text_editor::Action::Move(text_editor::Motion::Up),
        );

        assert_eq!(state.focus().map(|focus| focus.block_id), Some(sibling));
    }

    #[test]
    fn move_cursor_by_word_long_latin_line() {
        let (mut state, root) = AppState::test_state();
        let long_text = "hello world foo bar baz qux quux corge";
        state.store.update_point(&root, long_text.to_string());
        state.editor_buffers.set_text(&root, long_text);
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 0 },
                selection: None,
            });
        }

        // Move right: should visit each word boundary
        // hello(0-5) world(6-11) foo(12-15) bar(16-19) baz(20-23) qux(24-27) quux(28-32) corge(33-38)
        let expected_positions = vec![5, 6, 11, 12, 15, 16, 19, 20, 23, 24, 27, 28, 32, 33, 38];
        for expected_column in expected_positions {
            let _ = handle(
                &mut state,
                EditMessage::MoveCursorByWord {
                    block_id: root,
                    direction: WordCursorDirection::Right,
                },
            );
            let cursor =
                state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
            assert_eq!(
                cursor.column, expected_column,
                "Failed at expected column {}",
                expected_column
            );
        }

        // Should stay at end when already at end
        let _ = handle(
            &mut state,
            EditMessage::MoveCursorByWord { block_id: root, direction: WordCursorDirection::Right },
        );
        let cursor =
            state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
        assert_eq!(cursor.column, long_text.chars().count());
    }

    #[test]
    fn move_cursor_by_word_mixed_han_and_latin() {
        let (mut state, root) = AppState::test_state();
        // "你好 hello 世界 world"
        // Char:  你  好     h     e     l     l     o          世     界          w     o     r     l     d
        // Byte:  0-3 3-6 6-7 7-8 8-9 9-10 10-11 11-12 12-13 13-16 16-19 19-20 20-21 21-22 22-23 23-24 24-25
        let text = "你好 hello 世界 world";
        state.store.update_point(&root, text.to_string());
        state.editor_buffers.set_text(&root, text);
        // Start at byte 0 (char 0)
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 0 },
                selection: None,
            });
        }

        // Move right through mixed script - expected BYTE positions
        // Char positions: 1, 2, 3, 8, 9, 10, 11, 12, 17
        // Byte positions: 3, 6, 7, 12, 13, 16, 19, 20, 25
        let expected_bytes = vec![3, 6, 7, 12, 13, 16, 19, 20, 25];
        for expected_byte in expected_bytes {
            let _ = handle(
                &mut state,
                EditMessage::MoveCursorByWord {
                    block_id: root,
                    direction: WordCursorDirection::Right,
                },
            );
            let cursor =
                state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
            assert_eq!(cursor.column, expected_byte, "Failed at expected byte {}", expected_byte);
        }
    }
}
