//! Point editor mutation and cursor-navigation behavior.
//!
//! This module owns all point-level edit actions emitted by
//! [`PointTextEditor`](super::point_editor::PointTextEditor), including:
//! - text insert/delete/newline handling,
//! - word-wise cursor movement,
//! - multiselect backspace transitions,
//! - cross-block vertical cursor traversal.
//!
//! # Vertical navigation model
//!
//! Cursor state is tracked in two coordinate spaces:
//! - logical position (`line`, `column_byte`) used by iced content APIs,
//! - visual row position inside wrapped lines (runtime layout dependent).
//!
//! `ArrowUp`/`ArrowDown` first delegates to iced (`content.perform(action)`),
//! then detects an edge hit when cursor position is unchanged. At that point we
//! traverse to the adjacent visible block and restore horizontal intent using a
//! sticky preferred char column (`TransientUiState::vertical_cursor_preferred_column`).
//!
//! Note: wrapped lines can report `line_count == 1` while rendering many
//! visual rows. Therefore, cross-block `Up` must optionally seek to the lowest
//! visual row after cursor placement instead of relying on logical line index.
//!
//! Note: focus transfer can override caret state in some runtimes.
//! Cross-block traversal applies cursor placement both immediately and in a
//! deferred `SetCursor` message so the final caret location is deterministic.
//!
//! Invariants:
//! - Preferred vertical column is stored in char units, never raw bytes.
//! - Byte/char conversion always clamps to valid UTF-8 boundaries.
//! - Preferred vertical column is reset on non-vertical edits and focus change.

use super::point_editor::WordCursorDirection;
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
    /// Remove the link at `index` from a block's point.
    ///
    /// Emitted by the remove action in a reference-panel link row.
    RemoveLink {
        block_id: BlockId,
        index: usize,
    },
    /// Apply an explicit cursor position to a point editor.
    ///
    /// This is used after cross-block focus transitions so the target editor
    /// receives focus first, then cursor placement is restored deterministically.
    ///
    /// Note: this message intentionally carries a byte column because
    /// iced cursor APIs are byte-based. Callers are responsible for providing
    /// byte offsets derived from char-clamped conversions.
    SetCursor {
        block_id: BlockId,
        line: usize,
        column_byte: usize,
        /// Whether to continue moving down visually after initial placement.
        ///
        /// Used for cross-block `ArrowUp` traversal so a wrapped logical line
        /// lands on its last visual row, matching user expectation of moving
        /// "up into the row above".
        seek_visual_end: bool,
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
        | EditMessage::RemoveLink { block_id, index } => {
            state.store.remove_link_from_point(&block_id, index);
            // Collapse any expanded link row for this block whose index is now stale.
            state.ui_mut().reference_panel.expanded_links.remove(&block_id);
            if matches!(
                state.ui().reference_panel.editing_perspective,
                Some(super::state::ReferencePerspectiveEditState::Link { target, .. })
                    if target == block_id
            ) {
                state.ui_mut().reference_panel.editing_perspective = None;
            }
            state.clear_expanded_markdown_preview(&block_id);
            state.persist_with_context("remove link");
            Task::none()
        }
        | EditMessage::SetCursor { block_id, line, column_byte, seek_visual_end } => {
            tracing::debug!(
                block_id = ?block_id,
                line,
                column_byte,
                seek_visual_end,
                "received deferred set-cursor edit message"
            );
            set_cursor(state, block_id, line, column_byte, seek_visual_end)
        }
    }
}

/// Direction tag for vertical cursor movement edge-detection.
///
/// Used to defer block traversal until *after* the editor processes
/// the motion, so wrapped (visual) lines are handled correctly.
#[derive(Debug, Clone, Copy)]
enum VerticalDir {
    Up,
    Down,
}

/// Map an editor action to a vertical movement direction.
fn vertical_direction_for_action(action: &text_editor::Action) -> Option<VerticalDir> {
    match action {
        | text_editor::Action::Move(text_editor::Motion::Up) => Some(VerticalDir::Up),
        | text_editor::Action::Move(text_editor::Motion::Down) => Some(VerticalDir::Down),
        | _ => None,
    }
}

/// Convert a UTF-8 byte column to a char column in one line.
///
/// The editor cursor stores byte offsets. This helper clamps invalid byte
/// offsets to the nearest previous char boundary so callers can safely reason
/// in char columns.
fn byte_column_to_char_column(line_text: &str, column_byte: usize) -> usize {
    if column_byte >= line_text.len() {
        return line_text.chars().count();
    }
    // Clamp to the nearest char boundary at or before `column_byte`.
    // Example: for "你" (bytes 0..3), byte 1 maps to char column 0.
    let boundary_count =
        line_text.char_indices().take_while(|(idx, _)| *idx <= column_byte).count();
    boundary_count.saturating_sub(1)
}

/// Convert a char column to a UTF-8 byte column in one line.
fn char_column_to_byte_column(line_text: &str, column_char: usize) -> usize {
    line_text.char_indices().nth(column_char).map(|(idx, _)| idx).unwrap_or(line_text.len())
}

/// Resolve the last line index that can be read via [`text_editor::Content::line`].
///
/// `iced` may report a line count that includes an internal trailing line
/// marker. This helper walks backwards to find a line index that is actually
/// addressable for cursor placement.
///
/// Note: this fallback keeps cross-block Up traversal stable even when backend
/// line accounting and exposed line access diverge.
fn last_addressable_line_index(content: &text_editor::Content) -> usize {
    let mut index = content.line_count().saturating_sub(1);
    loop {
        if content.line(index).is_some() || index == 0 {
            return index;
        }
        index = index.saturating_sub(1);
    }
}

/// Resolve target cursor position for cross-block vertical traversal.
///
/// Returns `(target_line, target_column_byte, target_line_char_len)`.
///
/// Note: when a target point is a single logical line that wraps
/// visually, `line_count` may still be `1`. In that case the caller must use
/// visual-row seeking after this logical placement when moving `Up`.
fn target_cursor_for_vertical_cross_block(
    content: &text_editor::Content, dir: VerticalDir, preferred_char_column: usize,
) -> (usize, usize, usize) {
    let line_count = content.line_count();
    let target_line = match dir {
        | VerticalDir::Up => last_addressable_line_index(content),
        | VerticalDir::Down => 0, // first line
    };
    let target_line_text =
        content.line(target_line).map(|line| line.text.to_string()).unwrap_or_default();
    let target_line_chars = target_line_text.chars().count();
    let target_column_char = preferred_char_column.min(target_line_text.chars().count());
    let target_column_byte = char_column_to_byte_column(&target_line_text, target_column_char);
    tracing::debug!(
        ?dir,
        line_count,
        target_line,
        preferred_char_column,
        target_column_char,
        target_column_byte,
        target_line_chars,
        "resolved cross-block vertical traversal target cursor"
    );
    (target_line, target_column_byte, target_line_chars)
}

/// Move the caret to the lowest reachable visual row in the current logical line.
///
/// Returns the number of successful visual-down steps performed.
///
/// Note: this is intentionally implemented by repeated editor-native
/// `Move(Down)` operations. The editor owns wrap/layout logic, so this avoids
/// duplicating line-wrap calculations in app code.
fn seek_cursor_to_visual_end(content: &mut text_editor::Content, max_steps: usize) -> usize {
    let mut visual_down_steps = 0usize;
    loop {
        let before = content.cursor().position;
        content.perform(text_editor::Action::Move(text_editor::Motion::Down));
        let after = content.cursor().position;
        if after == before {
            break;
        }
        visual_down_steps += 1;
        if visual_down_steps > max_steps {
            break;
        }
    }
    visual_down_steps
}

fn set_cursor(
    state: &mut AppState, block_id: BlockId, line: usize, column_byte: usize, seek_visual_end: bool,
) -> Task<Message> {
    state.editor_buffers.ensure_block(&state.store, &block_id);
    if let Some(content) = state.editor_buffers.get_mut(&block_id) {
        let requested_line = line;
        let requested_column_byte = column_byte;
        let cursor_before = content.cursor().position;
        let line_count = content.line_count();
        let final_line =
            if content.line(line).is_some() { line } else { last_addressable_line_index(content) };
        let final_line_text =
            content.line(final_line).map(|line| line.text.to_string()).unwrap_or_default();
        // Clamp to line length and a valid char boundary before moving.
        // Note: clamped placement avoids backend normalization that can
        // otherwise jump the caret unexpectedly across UTF-8 boundaries.
        let clamped_char = byte_column_to_char_column(&final_line_text, column_byte);
        let final_column_byte = char_column_to_byte_column(&final_line_text, clamped_char);
        content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: final_line, column: final_column_byte },
            selection: None,
        });
        let mut visual_down_steps = 0usize;
        if seek_visual_end {
            // Use editor-native vertical motion to descend wrapped visual rows.
            // This is layout-aware at runtime and avoids homegrown wrap math.
            visual_down_steps = seek_cursor_to_visual_end(
                content,
                final_line_text.chars().count().saturating_add(1),
            );
            if visual_down_steps > final_line_text.chars().count().saturating_add(1) {
                tracing::warn!(
                    block_id = ?block_id,
                    visual_down_steps,
                    "aborting visual seek due to unexpected excessive down steps"
                );
            }
        }
        let cursor_after = content.cursor().position;
        tracing::debug!(
            block_id = ?block_id,
            line_count,
            requested_line,
            requested_column_byte,
            seek_visual_end,
            visual_down_steps,
            final_line,
            final_column_byte,
            final_line_chars = final_line_text.chars().count(),
            before_line = cursor_before.line,
            before_column = cursor_before.column,
            after_line = cursor_after.line,
            after_column = cursor_after.column,
            "applied set-cursor request"
        );
    } else {
        tracing::warn!(block_id = ?block_id, "set-cursor skipped because editor buffer is missing");
    }
    Task::none()
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
    let current_column = byte_column_to_char_column(&line_text, current_column_byte);

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
        let next_column_byte = char_column_to_byte_column(&line_text, next_column);

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
        | text_editor::Action::Edit(text_editor::Edit::Insert('.')) => Some(ActionId::Amplify),
        | text_editor::Action::Edit(text_editor::Edit::Insert('/')) => Some(ActionId::Atomize),
        | text_editor::Action::Edit(text_editor::Edit::Insert(',')) => Some(ActionId::Distill),
        | _ => None,
    }
}

/// Return true when an editor insert should be treated as a leaked global
/// shortcut chord instead of literal text input.
///
/// Note: `[`/`]` are included because movement is resolved globally
/// (`Cmd+[ / ]` on macOS, `Ctrl+[ / ]` on other platforms).
fn is_command_shortcut_editor_insert(
    action: &text_editor::Action, modifiers: keyboard::Modifiers,
) -> bool {
    if !is_shortcut_modifier(modifiers) {
        return false;
    }

    matches!(
        action,
        text_editor::Action::Edit(text_editor::Edit::Insert(c))
            if matches!(c.to_ascii_lowercase(), 'f' | 'g' | 'z' | '.' | '/' | ',' | '[' | ']')
    )
}

/// Detect editor actions leaked from movement shortcut key chords.
///
/// On macOS: `Ctrl+Arrow`; on other platforms: `Alt+Arrow`.
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
    #[cfg(target_os = "macos")]
    let has_movement_modifier = modifiers.control() && !modifiers.command() && !modifiers.alt();
    #[cfg(not(target_os = "macos"))]
    let has_movement_modifier = modifiers.alt() && !modifiers.command() && !modifiers.control();

    if !has_movement_modifier {
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

/// Normalize editor text into persisted point text.
///
/// Iced serializes a single empty line with a trailing newline sentinel, so
/// point-edit helpers must normalize that representation before comparing or
/// persisting text.
fn point_text_from_editor_text(text: &str) -> String {
    text.strip_suffix('\n').filter(|text| text.is_empty()).unwrap_or(text).to_string()
}

/// Return true when the cursor is already at the end of the point editor.
fn is_cursor_at_end_of_editor(content: &text_editor::Content) -> bool {
    if point_text_from_editor_text(&content.text()).is_empty() {
        return true;
    }

    let cursor = content.cursor().position;
    let final_line = last_addressable_line_index(content);
    if cursor.line != final_line {
        return false;
    }

    content
        .line(final_line)
        .map(|line| cursor.column >= line.text.len())
        .unwrap_or(cursor.column == 0)
}

/// Return true when an editor action should enter link mode.
///
/// Link mode is keyed off a direct `Insert('@')` action at the end of the
/// current point editor. This keeps the behavior tied to an actual keystroke
/// instead of any later buffer shape, so programmatic buffer synchronization
/// for the double-`@` escape path cannot immediately re-trigger link mode.
///
/// Note: paste and other non-insert mutations intentionally do not enter link
/// mode, even if they make the buffer text equal to `@`.
fn should_enter_link_mode_from_action(
    content: &text_editor::Content, action: &text_editor::Action,
) -> bool {
    matches!(action, text_editor::Action::Edit(text_editor::Edit::Insert('@')))
        && is_cursor_at_end_of_editor(content)
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

    content.line(0).is_some_and(|line| {
        let cursor_char_column = byte_column_to_char_column(&line.text, cursor.column);
        cursor_char_column >= line.text.chars().count()
    })
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
    state.ui_mut().reference_panel.hovered_friend_block = None;

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

    let scroll = super::scroll::scroll_block_into_view(child_id);
    if let Some(widget_id) = state.editor_buffers.widget_id(&child_id) {
        return Task::batch([widget::operation::focus(widget_id.clone()), scroll]);
    }

    scroll
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
pub fn handle_multiselect_backspace(state: &mut AppState) -> Task<Message> {
    let block_id = state
        .ui()
        .multiselect_selected_blocks
        .iter()
        .next()
        .copied()
        .or_else(|| state.focus().map(|f| f.block_id));

    let Some(block_id) = block_id else {
        tracing::debug!("multiselect backspace with no selection and no focus");
        return Task::none();
    };
    if state.store.node(&block_id).is_none() {
        return Task::none();
    }
    delete_multiselect_selection_on_backspace(state, block_id)
}

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
        let scroll = super::scroll::scroll_block_into_view(next_focus);
        if let Some(widget_id) = state.editor_buffers.widget_id(&next_focus) {
            return Task::batch([widget::operation::focus(widget_id.clone()), scroll]);
        }
        return scroll;
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
///
/// Vertical traversal contract:
/// - Let iced perform the `Up`/`Down` move inside the current editor first.
/// - If the cursor did not move, treat it as a block-edge hit and traverse to
///   previous/next visible block.
/// - Preserve horizontal intent using char-based preferred column state.
/// - For `Up`, seek to the target line's lowest visual row to match user
///   expectation in wrapped single-line editors.
pub fn handle_point_edited(
    state: &mut AppState, block_id: BlockId, action: text_editor::Action,
) -> Task<Message> {
    // Clear friend hover state when editing
    state.ui_mut().reference_panel.hovered_friend_block = None;
    let vertical_direction = vertical_direction_for_action(&action);
    if let Some(dir) = vertical_direction {
        tracing::debug!(
            block_id = ?block_id,
            ?dir,
            document_mode = ?state.ui().document_mode,
            previous_preferred = state.ui().vertical_cursor_preferred_column,
            "handling vertical editor motion"
        );
    }
    if vertical_direction.is_none() {
        state.ui_mut().vertical_cursor_preferred_column = None;
    }

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
        // Ctrl+arrow (macOS) / Alt+arrow (other) shortcuts are handled by
        // the global subscription path. Ignore editor cursor-motion actions
        // here to avoid handling the same key chord twice.
        tracing::debug!("ignored movement-shortcut editor action leak");
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
        state.ui_mut().multiselect_anchor = Some(block_id);
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

    // Preserve a sticky preferred char column while the user repeats vertical
    // motion. This keeps horizontal intent stable when traversing short lines.
    let previous_preferred_char_column = state.ui().vertical_cursor_preferred_column;
    let mut next_preferred_char_column =
        if vertical_direction.is_some() { previous_preferred_char_column } else { None };

    // When crossing block boundaries via Up/Down at edges, traverse to the
    // adjacent visible block and restore the same preferred char column.
    let mut navigate_to: Option<(BlockId, VerticalDir, usize)> = None;
    if let Some(content) = state.editor_buffers.get_mut(&block_id) {
        let point_text_before_action = point_text_from_editor_text(&content.text());
        let should_enter_link_mode = should_enter_link_mode_from_action(content, &action);
        let cursor_before = content.cursor().position;
        let preferred_char_column = vertical_direction.map(|_| {
            let current_char_column = content
                .line(cursor_before.line)
                .map(|line| byte_column_to_char_column(&line.text, cursor_before.column))
                .unwrap_or(0);
            let preferred = previous_preferred_char_column.unwrap_or(current_char_column);
            next_preferred_char_column = Some(preferred);
            preferred
        });
        if let Some(dir) = vertical_direction {
            tracing::debug!(
                block_id = ?block_id,
                ?dir,
                before_line = cursor_before.line,
                before_column = cursor_before.column,
                preferred_char_column = preferred_char_column.unwrap_or(0),
                "vertical move before editor perform"
            );
        }

        content.perform(action);
        let cursor_after = content.cursor().position;
        if let Some(dir) = vertical_direction {
            tracing::debug!(
                block_id = ?block_id,
                ?dir,
                after_line = cursor_after.line,
                after_column = cursor_after.column,
                "vertical move after editor perform"
            );
        }

        if let Some(dir) = vertical_direction
            && cursor_before == cursor_after
        {
            let target = match dir {
                | VerticalDir::Up => state.store.prev_visible_in_dfs(&block_id),
                | VerticalDir::Down => state.store.next_visible_in_dfs(&block_id),
            };
            if let Some(target_id) = target {
                let preferred_char = preferred_char_column.unwrap_or(0);
                tracing::debug!(
                    from = ?block_id,
                    to = ?target_id,
                    ?dir,
                    preferred_char_column = preferred_char,
                    "detected vertical edge hit; scheduling cross-block traversal"
                );
                navigate_to = Some((target_id, dir, preferred_char));
            } else {
                tracing::debug!(
                    from = ?block_id,
                    ?dir,
                    "vertical edge hit with no adjacent visible block"
                );
            }
        }

        if navigate_to.is_none() {
            let next_text = content.text();

            // Detect a direct `@` keystroke in an empty point: enter link mode.
            //
            // Note: the predicate is computed from the pre-edit text and the
            // concrete action, then applied here after `content.perform(action)`
            // so the editor can still be cleared in the same event-loop turn
            // with no visible flash.
            if should_enter_link_mode {
                // Restore the point text before the trigger keystroke so the
                // inserted `@` is consumed purely as a link-mode shortcut.
                content.perform(iced::widget::text_editor::Action::SelectAll);
                content.perform(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Paste(point_text_before_action.clone().into()),
                ));
                state.store.update_point(&block_id, point_text_before_action);
                state.persist_with_context("remove trigger @ for link mode");
                return Task::done(Message::LinkMode(LinkModeMessage::Enter(block_id)));
            }

            tracing::debug!(block_id = ?block_id, chars = next_text.len(), "point edited");
            state.store.update_point(&block_id, next_text);
            state.persist_with_context("after edit");
            state.editor_buffers.invalidate_token_cache(&block_id);
        }
    }
    state.ui_mut().vertical_cursor_preferred_column = next_preferred_char_column;

    if let Some((target_id, dir, preferred_char_column)) = navigate_to {
        let wid = state.editor_buffers.widget_id(&target_id).cloned();
        if let (Some(wid), DocumentMode::Normal) = (wid, state.ui().document_mode) {
            state.editor_buffers.ensure_block(&state.store, &target_id);
            let (target_line, target_column_byte, target_line_chars) = state
                .editor_buffers
                .get(&target_id)
                .map(|content| {
                    target_cursor_for_vertical_cross_block(content, dir, preferred_char_column)
                })
                .unwrap_or((0, 0, 0));
            if let Some(target_content) = state.editor_buffers.get_mut(&target_id) {
                let before = target_content.cursor().position;
                target_content.move_to(text_editor::Cursor {
                    position: text_editor::Position {
                        line: target_line,
                        column: target_column_byte,
                    },
                    selection: None,
                });
                let immediate_visual_down_steps = if matches!(dir, VerticalDir::Up) {
                    seek_cursor_to_visual_end(target_content, target_line_chars.saturating_add(1))
                } else {
                    0
                };
                let after = target_content.cursor().position;
                tracing::debug!(
                    target = ?target_id,
                    ?dir,
                    immediate_target_line = target_line,
                    immediate_target_column = target_column_byte,
                    immediate_visual_down_steps,
                    before_line = before.line,
                    before_column = before.column,
                    after_line = after.line,
                    after_column = after.column,
                    "applied immediate cross-block cursor placement before focus"
                );
            }
            state.set_focus(target_id);
            state.ui_mut().vertical_cursor_preferred_column = Some(preferred_char_column);
            tracing::debug!(
                from = ?block_id,
                to = ?target_id,
                ?dir,
                preferred_char_column,
                deferred_target_line = target_line,
                deferred_target_column = target_column_byte,
                "keyboard traversal"
            );
            return Task::batch([
                widget::operation::focus(wid),
                Task::done(Message::Edit(EditMessage::SetCursor {
                    block_id: target_id,
                    line: target_line,
                    column_byte: target_column_byte,
                    seek_visual_end: matches!(dir, VerticalDir::Up),
                })),
                super::scroll::scroll_block_into_view(target_id),
            ]);
        }
    }
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_at_into_empty_point_is_link_mode_trigger() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, String::new());
        state.editor_buffers.set_text(&root, "");
        let content = state.editor_buffers.get(&root).expect("editor content exists");

        assert!(should_enter_link_mode_from_action(
            content,
            &text_editor::Action::Edit(text_editor::Edit::Insert('@')),
        ));
    }

    #[test]
    fn insert_at_at_editor_end_is_link_mode_trigger() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "existing".to_string());
        state.editor_buffers.set_text(&root, "existing");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 8 },
                selection: None,
            });
        }
        let content = state.editor_buffers.get(&root).expect("editor content exists");

        assert!(should_enter_link_mode_from_action(
            content,
            &text_editor::Action::Edit(text_editor::Edit::Insert('@')),
        ));
    }

    #[test]
    fn insert_at_in_middle_of_point_does_not_trigger_link_mode() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "existing".to_string());
        state.editor_buffers.set_text(&root, "existing");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 3 },
                selection: None,
            });
        }
        let content = state.editor_buffers.get(&root).expect("editor content exists");

        assert!(!should_enter_link_mode_from_action(
            content,
            &text_editor::Action::Edit(text_editor::Edit::Insert('@')),
        ));
    }

    #[test]
    fn paste_at_into_empty_point_does_not_trigger_link_mode() {
        let (mut state, root) = AppState::test_state();
        state.editor_buffers.ensure_block(&state.store, &root);
        let content = state.editor_buffers.get(&root).expect("editor content exists");

        assert!(!should_enter_link_mode_from_action(
            content,
            &text_editor::Action::Edit(text_editor::Edit::Paste(String::from("@").into())),
        ));
    }

    #[test]
    fn at_trigger_preserves_existing_point_text() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "existing".to_string());
        state.editor_buffers.set_text(&root, "existing");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 8 },
                selection: None,
            });
        }

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('@')),
        );

        assert_eq!(state.store.point(&root).as_deref(), Some("existing"));
        let text = state.editor_buffers.get(&root).expect("editor content exists").text();
        assert_eq!(text, "existing");
    }

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

        assert!(state.llm_requests.is_amplifying(root));
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

        assert!(state.llm_requests.is_distilling(root));
    }

    #[test]
    fn command_slash_insert_triggers_atomize_for_block() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;
        state.store.update_point(&root, "needs atomize".to_string());
        state.editor_buffers.set_text(&root, "needs atomize");

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('/')),
        );

        assert!(state.llm_requests.is_atomizing(root));
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

        assert!(state.llm_requests.is_amplifying(root));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_left_bracket_insert_is_ignored_as_shortcut_leak() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::CTRL;
        state.store.update_point(&root, "hello".to_string());
        state.editor_buffers.set_text(&root, "hello");

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('[')),
        );

        let text = state.editor_buffers.get(&root).expect("editor content exists").text();
        assert_eq!(text, "hello");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn command_left_bracket_insert_is_ignored_as_shortcut_leak() {
        let (mut state, root) = AppState::test_state();
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;
        state.store.update_point(&root, "hello".to_string());
        state.editor_buffers.set_text(&root, "hello");

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Edit(text_editor::Edit::Insert('[')),
        );

        let text = state.editor_buffers.get(&root).expect("editor content exists").text();
        assert_eq!(text, "hello");
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

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(target_os = "macos")]
    #[test]
    fn ctrl_up_editor_motion_is_ignored_to_prevent_double_navigation() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");
        state.set_focus(sibling);
        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::CTRL;

        let _ = handle_point_edited(
            &mut state,
            sibling,
            text_editor::Action::Move(text_editor::Motion::Up),
        );

        assert_eq!(state.focus().map(|focus| focus.block_id), Some(sibling));
    }

    #[test]
    fn vertical_navigation_keeps_preferred_column_across_blocks() {
        let (mut state, root) = AppState::test_state();
        let middle = state
            .store
            .append_sibling(&root, "short".to_string())
            .expect("append sibling succeeds");
        let tail = state
            .store
            .append_sibling(&middle, "0123456789".to_string())
            .expect("append sibling succeeds");
        state.store.update_point(&root, "0123456789\nabcdefghij".to_string());
        state.editor_buffers.set_text(&root, "0123456789\nabcdefghij");
        state.editor_buffers.set_text(&middle, "short");
        state.editor_buffers.set_text(&tail, "0123456789");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 8 },
                selection: None,
            });
        }
        state.set_focus(root);

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Move(text_editor::Motion::Down),
        );
        let middle_cursor =
            state.editor_buffers.get(&middle).expect("middle editor exists").cursor().position;
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(middle));
        assert_eq!(middle_cursor.column, 5);

        let _ = handle_point_edited(
            &mut state,
            middle,
            text_editor::Action::Move(text_editor::Motion::Down),
        );
        let tail_cursor =
            state.editor_buffers.get(&tail).expect("tail editor exists").cursor().position;
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(tail));
        assert_eq!(tail_cursor.column, 8);
    }

    #[test]
    fn non_vertical_motion_resets_preferred_vertical_column() {
        let (mut state, root) = AppState::test_state();
        let middle = state
            .store
            .append_sibling(&root, "short".to_string())
            .expect("append sibling succeeds");
        let tail = state
            .store
            .append_sibling(&middle, "0123456789".to_string())
            .expect("append sibling succeeds");
        state.store.update_point(&root, "0123456789\nabcdefghij".to_string());
        state.editor_buffers.set_text(&root, "0123456789\nabcdefghij");
        state.editor_buffers.set_text(&middle, "short");
        state.editor_buffers.set_text(&tail, "0123456789");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 8 },
                selection: None,
            });
        }
        state.set_focus(root);

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Move(text_editor::Motion::Down),
        );
        let _ = handle_point_edited(
            &mut state,
            middle,
            text_editor::Action::Move(text_editor::Motion::Left),
        );
        let _ = handle_point_edited(
            &mut state,
            middle,
            text_editor::Action::Move(text_editor::Motion::Down),
        );

        let tail_cursor =
            state.editor_buffers.get(&tail).expect("tail editor exists").cursor().position;
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(tail));
        assert_eq!(tail_cursor.column, 4);
    }

    #[test]
    fn vertical_navigation_clamps_to_valid_utf8_char_boundaries() {
        let (mut state, root) = AppState::test_state();
        let sibling =
            state.store.append_sibling(&root, "你好".to_string()).expect("append sibling succeeds");
        state.store.update_point(&root, "abc\ndef".to_string());
        state.editor_buffers.set_text(&root, "abc\ndef");
        state.editor_buffers.set_text(&sibling, "你好");
        if let Some(content) = state.editor_buffers.get_mut(&root) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 1 },
                selection: None,
            });
        }
        state.set_focus(root);

        let _ = handle_point_edited(
            &mut state,
            root,
            text_editor::Action::Move(text_editor::Motion::Down),
        );

        let sibling_cursor =
            state.editor_buffers.get(&sibling).expect("sibling editor exists").cursor().position;
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(sibling));
        assert_eq!(sibling_cursor.column, 3);
    }

    #[test]
    fn vertical_navigation_up_crosses_to_last_line_of_previous_block() {
        let (mut state, root) = AppState::test_state();
        let below = state
            .store
            .append_sibling(&root, "bottom".to_string())
            .expect("append sibling succeeds");
        state.store.update_point(&root, "abc\ndefgh".to_string());
        state.editor_buffers.set_text(&root, "abc\ndefgh");
        state.editor_buffers.set_text(&below, "bottom");
        if let Some(content) = state.editor_buffers.get_mut(&below) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 4 },
                selection: None,
            });
        }
        state.set_focus(below);

        let _ = handle_point_edited(
            &mut state,
            below,
            text_editor::Action::Move(text_editor::Motion::Up),
        );

        let root_cursor =
            state.editor_buffers.get(&root).expect("root editor exists").cursor().position;
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
        assert_eq!(root_cursor.line, 1);
        assert_eq!(root_cursor.column, 4);
    }

    #[test]
    fn set_cursor_clamps_to_last_addressable_line() {
        let (mut state, root) = AppState::test_state();
        state.editor_buffers.set_text(&root, "aaa\nbbbb");

        let _ = handle(
            &mut state,
            EditMessage::SetCursor {
                block_id: root,
                line: usize::MAX,
                column_byte: 2,
                seek_visual_end: false,
            },
        );

        let cursor = state.editor_buffers.get(&root).expect("root editor exists").cursor().position;
        assert_eq!(cursor.line, 1);
        assert_eq!(cursor.column, 2);
    }

    #[test]
    fn set_cursor_clamps_to_utf8_char_boundary() {
        let (mut state, root) = AppState::test_state();
        state.editor_buffers.set_text(&root, "你好");

        let _ = handle(
            &mut state,
            EditMessage::SetCursor {
                block_id: root,
                line: 0,
                column_byte: 1,
                seek_visual_end: false,
            },
        );

        let cursor = state.editor_buffers.get(&root).expect("root editor exists").cursor().position;
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 0);
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
