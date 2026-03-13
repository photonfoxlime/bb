//! Global find overlay for searching and jumping to block points.
//!
//! This panel is transient UI state (not persisted). It provides phrase-aware
//! search via [`crate::store::BlockStore::find_block_point`] and fast keyboard
//! navigation (`Cmd/Ctrl+F`, `Cmd/Ctrl+G`, `Esc`). Query updates are debounced
//! to avoid running expensive searches while users are still typing.

use crate::app::{AppState, DocumentMode, Message, reference_panel::ReferencePanelMessage};
use crate::component::floating_panel::{self, PanelHeader, SelectableRow};
use crate::component::icon_button::IconButton;
use crate::store::BlockId;
use crate::text::truncate_for_display;
use crate::theme;
use iced::widget::{Id, column, operation::focus, row, scrollable, text, text_input, tooltip};
use iced::{Element, Length, Task, keyboard};
use lucide_icons::iced as icons;
use rust_i18n::t;
use std::time::Duration;

const FIND_QUERY_INPUT_ID: &str = "find-query-input";
/// Delay after the last query edit before running search.
const FIND_QUERY_DEBOUNCE_MS: u64 = 300;

/// Transient state for the global find overlay.
///
/// # Invariant
///
/// When `selected` is `Some(i)`, `i` is always a valid index into `matches`.
#[derive(Debug, Clone, Default)]
pub struct FindUiState {
    query: String,
    matches: Vec<BlockId>,
    selected: Option<usize>,
    query_revision: u64,
}

impl FindUiState {
    /// Current user query text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Current search matches in DFS order.
    pub fn matches(&self) -> &[BlockId] {
        &self.matches
    }

    /// Current selected match index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Replace the query text and advance the debounce revision.
    ///
    /// The returned revision is attached to delayed refresh tasks so stale
    /// tasks can be ignored after subsequent edits.
    pub fn set_query(&mut self, query: String) -> u64 {
        self.query = query;
        self.query_revision = self.query_revision.wrapping_add(1);
        self.query_revision
    }

    /// Whether a debounced task revision is still current.
    pub fn is_current_revision(&self, revision: u64) -> bool {
        self.query_revision == revision
    }

    /// Replace matches and keep selection stable when possible.
    pub fn replace_matches(&mut self, matches: Vec<BlockId>) {
        let selected_block = self.selected_block_id();
        self.matches = matches;
        self.selected = selected_block
            .and_then(|id| self.matches.iter().position(|candidate| *candidate == id))
            .or_else(|| (!self.matches.is_empty()).then_some(0));
    }

    /// Select the next match, wrapping at the end.
    pub fn select_next(&mut self) {
        if self.matches.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            | Some(index) => (index + 1) % self.matches.len(),
            | None => 0,
        });
    }

    /// Select the previous match, wrapping at the start.
    pub fn select_previous(&mut self) {
        if self.matches.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            | Some(0) | None => self.matches.len() - 1,
            | Some(index) => index - 1,
        });
    }

    /// Select a concrete match index if it exists.
    pub fn select_index(&mut self, index: usize) {
        if index < self.matches.len() {
            self.selected = Some(index);
        }
    }

    /// Selected block id, if any.
    pub fn selected_block_id(&self) -> Option<BlockId> {
        self.selected.and_then(|index| self.matches.get(index).copied())
    }
}

/// Messages for find overlay interactions.
#[derive(Debug, Clone)]
pub enum FindMessage {
    /// Toggle the find overlay visibility.
    Toggle,
    /// Open the find overlay.
    Open,
    /// Close the find overlay.
    Close,
    /// Escape key behavior: close find if open, otherwise run fallback chain.
    Escape,
    /// Update query text.
    QueryChanged(String),
    /// Debounce timer elapsed for one query revision.
    DebounceElapsed(u64),
    /// Jump to the selected match.
    JumpSelected,
    /// Select and jump to the next match.
    JumpNext,
    /// Select and jump to the previous match.
    JumpPrevious,
    /// Select and jump to one match by index.
    JumpToIndex(usize),
}

/// Handle one find-overlay message.
pub fn handle(state: &mut AppState, message: FindMessage) -> Task<Message> {
    match message {
        | FindMessage::Toggle => {
            if state.ui().document_mode == DocumentMode::Find {
                state.ui_mut().document_mode = DocumentMode::Normal;
                Task::none()
            } else {
                state.ui_mut().document_mode = DocumentMode::Find;
                refresh_matches(state);
                focus(find_query_input_id())
            }
        }
        | FindMessage::Open => {
            state.ui_mut().document_mode = DocumentMode::Find;
            refresh_matches(state);
            focus(find_query_input_id())
        }
        | FindMessage::Close => {
            state.ui_mut().document_mode = DocumentMode::Normal;
            Task::none()
        }
        | FindMessage::Escape => {
            if state.ui().document_mode == DocumentMode::Find {
                state.ui_mut().document_mode = DocumentMode::Normal;
                return Task::none();
            }

            let reference_escape_active = state.ui().reference_panel.editing_perspective.is_some()
                || state.ui().document_mode == DocumentMode::PickFriend;
            if reference_escape_active {
                return AppState::update(
                    state,
                    Message::ReferencePanel(ReferencePanelMessage::CancelEditingPerspective),
                );
            }

            let _ = state.close_focused_block_panel();
            Task::none()
        }
        | FindMessage::QueryChanged(query) => {
            let previous_query = state.ui().find_ui.query();
            if is_command_shortcut_query_leak(previous_query, &query, state.ui().keyboard_modifiers)
            {
                tracing::debug!("ignored command-shortcut query leak");
                return Task::none();
            }

            // Ignore late text-input events when the panel is already closed.
            // This prevents shortcut-driven close (Cmd/Ctrl+F) from leaking a
            // trailing character into the stored query.
            if state.ui().document_mode != DocumentMode::Find {
                return Task::none();
            }
            let query_revision = state.ui_mut().find_ui.set_query(query);
            if state.ui().find_ui.query().trim().is_empty() {
                refresh_matches(state);
                return Task::none();
            }
            Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_millis(FIND_QUERY_DEBOUNCE_MS)).await;
                    query_revision
                },
                |revision| Message::Find(FindMessage::DebounceElapsed(revision)),
            )
        }
        | FindMessage::DebounceElapsed(revision) => {
            if state.ui().document_mode != DocumentMode::Find
                || !state.ui().find_ui.is_current_revision(revision)
            {
                return Task::none();
            }
            refresh_matches(state);
            Task::none()
        }
        | FindMessage::JumpSelected => jump_to_selected(state),
        | FindMessage::JumpNext => {
            if state.ui().document_mode != DocumentMode::Find {
                return Task::none();
            }
            state.ui_mut().find_ui.select_next();
            jump_to_selected(state)
        }
        | FindMessage::JumpPrevious => {
            if state.ui().document_mode != DocumentMode::Find {
                return Task::none();
            }
            state.ui_mut().find_ui.select_previous();
            jump_to_selected(state)
        }
        | FindMessage::JumpToIndex(index) => {
            if state.ui().document_mode != DocumentMode::Find {
                return Task::none();
            }
            state.ui_mut().find_ui.select_index(index);
            jump_to_selected(state)
        }
    }
}

/// Render the floating find overlay.
pub fn floating_overlay<'a>(state: &'a AppState) -> Element<'a, Message> {
    if state.ui().document_mode != DocumentMode::Find {
        return floating_panel::invisible_spacer();
    }

    let title = text(t!("ui_find").to_string()).font(theme::INTER).size(theme::FIND_TITLE_SIZE);
    let count_label = if state.ui().find_ui.query().trim().is_empty() {
        t!("find_hint_type").to_string()
    } else {
        t!("find_results_count", count = state.ui().find_ui.matches().len()).to_string()
    };

    let prev_btn = tooltip(
        IconButton::action_with_size(
            icons::icon_chevron_up().size(theme::FIND_CONTROL_ICON_SIZE).into(),
            theme::FIND_CONTROL_BUTTON_SIZE,
            theme::FIND_CONTROL_BUTTON_PAD,
        )
        .on_press(Message::Find(FindMessage::JumpPrevious)),
        text(t!("find_prev").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
        tooltip::Position::Bottom,
    )
    .style(theme::tooltip)
    .padding(theme::TOOLTIP_PAD)
    .gap(theme::TOOLTIP_GAP);

    let next_btn = tooltip(
        IconButton::action_with_size(
            icons::icon_chevron_down().size(theme::FIND_CONTROL_ICON_SIZE).into(),
            theme::FIND_CONTROL_BUTTON_SIZE,
            theme::FIND_CONTROL_BUTTON_PAD,
        )
        .on_press(Message::Find(FindMessage::JumpNext)),
        text(t!("find_next").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
        tooltip::Position::Bottom,
    )
    .style(theme::tooltip)
    .padding(theme::TOOLTIP_PAD)
    .gap(theme::TOOLTIP_GAP);

    let close_btn = tooltip(
        IconButton::action_with_size(
            icons::icon_x().size(theme::FIND_CONTROL_ICON_SIZE).into(),
            theme::FIND_CONTROL_BUTTON_SIZE,
            theme::FIND_CONTROL_BUTTON_PAD,
        )
        .on_press(Message::Find(FindMessage::Close)),
        text(t!("ui_close").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
        tooltip::Position::Bottom,
    )
    .style(theme::tooltip)
    .padding(theme::TOOLTIP_PAD)
    .gap(theme::TOOLTIP_GAP);

    let controls = row![]
        .spacing(theme::FLOATING_PANEL_CONTROL_GAP)
        .align_y(iced::Alignment::Center)
        .push(text(count_label).size(theme::FIND_META_SIZE).style(theme::spine_text))
        .push(prev_btn)
        .push(next_btn)
        .push(close_btn);

    let placeholder = t!("find_placeholder").to_string();
    let query_input = text_input(placeholder.as_str(), state.ui().find_ui.query())
        .id(find_query_input_id())
        .on_input(|query| Message::Find(FindMessage::QueryChanged(query)))
        .on_submit(Message::Find(FindMessage::JumpSelected))
        .size(theme::FIND_QUERY_SIZE)
        .padding(theme::FIND_QUERY_PAD);

    let result_list: Element<'a, Message> = if state.ui().find_ui.query().trim().is_empty() {
        text(t!("find_hint_empty").to_string())
            .size(theme::FIND_RESULT_META_SIZE)
            .style(theme::spine_text)
            .into()
    } else if state.ui().find_ui.matches().is_empty() {
        text(t!("find_no_results").to_string())
            .size(theme::FIND_RESULT_META_SIZE)
            .style(theme::spine_text)
            .into()
    } else {
        let mut rows = column![].spacing(theme::PANEL_INNER_GAP);
        for (index, block_id) in state.ui().find_ui.matches().iter().enumerate() {
            let point = state.store.point(block_id).unwrap_or_default();
            let point_label = truncate_for_display(&point, theme::FIND_RESULT_POINT_TRUNCATE);
            let lineage = result_lineage(state, block_id);

            let mut row_content = column![].spacing(theme::FIND_RESULT_LINE_GAP).push(
                text(point_label).font(theme::LXGW_WENKAI).size(theme::FIND_RESULT_POINT_SIZE),
            );
            if !lineage.is_empty() {
                row_content = row_content.push(
                    text(lineage)
                        .font(theme::INTER)
                        .size(theme::FIND_RESULT_META_SIZE)
                        .style(theme::spine_text),
                );
            }

            rows = rows.push(SelectableRow::new(
                row_content,
                state.ui().find_ui.selected_index() == Some(index),
                Message::Find(FindMessage::JumpToIndex(index)),
            ));
        }

        scrollable(rows).height(Length::Fixed(theme::FIND_RESULT_LIST_HEIGHT)).into()
    };

    let viewport_width = state.ui().window_size.width;
    let viewport_height = state.ui().window_size.height;

    let content = column![]
        .spacing(theme::FLOATING_PANEL_SECTION_GAP)
        .push(PanelHeader::new(title, controls))
        .push(query_input)
        .push(result_list);

    floating_panel::wrap(content, viewport_width, viewport_height)
}

fn refresh_matches(state: &mut AppState) {
    let query = state.ui().find_ui.query().trim();
    let matches = if query.is_empty() { vec![] } else { state.store.find_block_point(query) };
    state.ui_mut().find_ui.replace_matches(matches);
}

/// Returns true when `next` looks like a leaked global command shortcut key.
///
/// Iced text inputs insert printable `text` even with command modifiers.
/// For global shortcuts (Cmd/Ctrl+F/G/Z), this can leak a single character
/// into the query before our global handler runs.
fn is_command_shortcut_query_leak(
    previous: &str, next: &str, modifiers: keyboard::Modifiers,
) -> bool {
    if !modifiers.command() {
        return false;
    }
    let Some(inserted_char) = single_inserted_char(previous, next) else {
        return false;
    };
    matches!(inserted_char.to_ascii_lowercase(), 'f' | 'g' | 'z')
}

/// Returns the inserted character if `next` is exactly `previous` plus one char.
fn single_inserted_char(previous: &str, next: &str) -> Option<char> {
    let previous_chars = previous.chars().collect::<Vec<_>>();
    let next_chars = next.chars().collect::<Vec<_>>();

    if next_chars.len() != previous_chars.len() + 1 {
        return None;
    }

    let mut previous_index = 0;
    let mut next_index = 0;
    let mut inserted_char = None;

    while previous_index < previous_chars.len() && next_index < next_chars.len() {
        if previous_chars[previous_index] == next_chars[next_index] {
            previous_index += 1;
            next_index += 1;
            continue;
        }

        if inserted_char.is_some() {
            return None;
        }

        inserted_char = Some(next_chars[next_index]);
        next_index += 1;
    }

    if inserted_char.is_none() {
        inserted_char = next_chars.last().copied();
    }

    inserted_char
}

fn jump_to_selected(state: &mut AppState) -> Task<Message> {
    if state.ui().document_mode != DocumentMode::Find {
        return Task::none();
    }

    let Some(target) = state.ui().find_ui.selected_block_id() else {
        return focus(find_query_input_id());
    };

    if state.store.node(&target).is_none() {
        tracing::error!(target = ?target, "cannot jump to missing find result");
        return focus(find_query_input_id());
    }

    state.navigation.reveal_parent_path(&state.store, &target);
    state.editor_buffers.ensure_block(&state.store, &target);
    state.set_focus(target);
    state.set_overflow_open(false);

    tracing::info!(target = ?target, "jumped to find result");
    Task::batch([focus(find_query_input_id()), super::scroll::scroll_block_into_view(target)])
}

fn result_lineage(state: &AppState, block_id: &BlockId) -> String {
    let mut points = state
        .store
        .lineage_points_for_id(block_id)
        .points()
        .map(|point| truncate_for_display(point, theme::FIND_RESULT_LINEAGE_TRUNCATE))
        .collect::<Vec<_>>();

    if !points.is_empty() {
        points.pop();
    }

    points.retain(|point| !point.is_empty());
    points.join(" > ")
}

fn find_query_input_id() -> Id {
    Id::new(FIND_QUERY_INPUT_ID)
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> (AppState, BlockId) {
        AppState::test_state()
    }

    fn flush_debounced_query(state: &mut AppState) {
        let revision = state.ui().find_ui.query_revision;
        let _ = AppState::update(state, Message::Find(FindMessage::DebounceElapsed(revision)));
    }

    #[test]
    fn toggle_opens_and_closes_overlay() {
        let (mut state, _) = test_state();
        assert!(state.ui().document_mode != DocumentMode::Find);

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Toggle));
        assert!(state.ui().document_mode == DocumentMode::Find);

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Toggle));
        assert!(state.ui().document_mode != DocumentMode::Find);
    }

    #[test]
    fn query_changed_refreshes_matches() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "root".to_string());
        let target =
            state.store.append_child(&root, "alpha beta".to_string()).expect("append child");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Open));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("beta".to_string())),
        );
        flush_debounced_query(&mut state);

        assert_eq!(state.ui().find_ui.matches(), &[target]);
        assert_eq!(state.ui().find_ui.selected_block_id(), Some(target));
    }

    #[test]
    fn query_changed_is_ignored_when_panel_closed() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "root".to_string());
        let _ = state.store.append_child(&root, "alpha beta".to_string()).expect("append child");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Open));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("beta".to_string())),
        );
        flush_debounced_query(&mut state);
        assert_eq!(state.ui().find_ui.query(), "beta");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Close));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("betaf".to_string())),
        );

        assert_eq!(state.ui().find_ui.query(), "beta");
    }

    #[test]
    fn query_changed_ignores_command_shortcut_leak() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "root".to_string());
        let _ = state.store.append_child(&root, "alpha beta".to_string()).expect("append child");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Open));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("beta".to_string())),
        );
        flush_debounced_query(&mut state);
        assert_eq!(state.ui().find_ui.query(), "beta");

        state.ui_mut().keyboard_modifiers = keyboard::Modifiers::COMMAND;
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("betaf".to_string())),
        );

        assert_eq!(state.ui().find_ui.query(), "beta");
    }

    #[test]
    fn stale_debounce_elapsed_is_ignored() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "root".to_string());
        let _ = state.store.append_child(&root, "alpha".to_string()).expect("append child");
        let target = state.store.append_child(&root, "alpine".to_string()).expect("append child");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Open));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("alp".to_string())),
        );
        let stale_revision = state.ui().find_ui.query_revision;

        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("alpi".to_string())),
        );

        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::DebounceElapsed(stale_revision)),
        );
        assert!(state.ui().find_ui.matches().is_empty());

        flush_debounced_query(&mut state);
        assert_eq!(state.ui().find_ui.matches(), &[target]);
        assert_eq!(state.ui().find_ui.selected_block_id(), Some(target));
    }

    #[test]
    fn jump_next_wraps_across_matches() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "root".to_string());
        let first = state.store.append_child(&root, "alpha".to_string()).expect("append child");
        let second = state.store.append_child(&root, "alpine".to_string()).expect("append child");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Open));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("alp".to_string())),
        );
        flush_debounced_query(&mut state);

        let _ = AppState::update(&mut state, Message::Find(FindMessage::JumpNext));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(second));

        let _ = AppState::update(&mut state, Message::Find(FindMessage::JumpNext));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(first));
    }

    #[test]
    fn jump_reveals_parent_navigation_path() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "root".to_string());
        let child = state.store.append_child(&root, "child".to_string()).expect("append child");
        let grand = state.store.append_child(&child, "grand".to_string()).expect("append child");
        let target =
            state.store.append_child(&grand, "target text".to_string()).expect("append child");

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Open));
        let _ = AppState::update(
            &mut state,
            Message::Find(FindMessage::QueryChanged("target text".to_string())),
        );
        flush_debounced_query(&mut state);
        let _ = AppState::update(&mut state, Message::Find(FindMessage::JumpSelected));

        assert_eq!(state.focus().map(|focus| focus.block_id), Some(target));
        let layers =
            state.navigation.layers().iter().map(|layer| layer.block_id).collect::<Vec<BlockId>>();
        assert_eq!(layers, vec![root, child, grand]);
    }

    #[test]
    fn escape_falls_back_to_friend_picker_cancel_when_closed() {
        let (mut state, _) = test_state();
        state.ui_mut().document_mode = DocumentMode::PickFriend;

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Escape));

        assert_eq!(state.ui().document_mode, DocumentMode::Normal);
    }

    #[test]
    fn escape_closes_focused_panel_when_no_other_action_is_triggered() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.store.set_block_panel_state(&root, Some(BlockPanelBarState::Instruction));

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Escape));

        assert_eq!(state.store.block_panel_state(&root).copied(), None);
    }

    #[test]
    fn escape_keeps_panel_open_when_friend_cancel_handles_it() {
        let (mut state, root) = test_state();
        state.set_focus(root);
        state.ui_mut().document_mode = DocumentMode::PickFriend;
        state.store.set_block_panel_state(&root, Some(BlockPanelBarState::References));

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Escape));

        assert_eq!(state.ui().document_mode, DocumentMode::Normal);
        assert_eq!(
            state.store.block_panel_state(&root).copied(),
            Some(BlockPanelBarState::References)
        );
    }
}
