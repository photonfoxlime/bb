//! Global find overlay for searching and jumping to block points.
//!
//! This panel is transient UI state (not persisted). It provides phrase-aware
//! search via [`crate::store::BlockStore::find_block_point`] and fast keyboard
//! navigation (`Cmd/Ctrl+F`, `Cmd/Ctrl+G`, `Esc`). Query updates are debounced
//! to avoid running expensive searches while users are still typing.

use crate::app::{AppState, Message, friends_panel::FriendPanelMessage};
use crate::store::BlockId;
use crate::text::truncate_for_display;
use crate::theme;
use iced::widget::{
    Id, button, column, container, operation::focus, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};
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
    open: bool,
    query: String,
    matches: Vec<BlockId>,
    selected: Option<usize>,
    query_revision: u64,
}

impl FindUiState {
    /// Whether the find overlay is currently visible.
    pub fn is_open(&self) -> bool {
        self.open
    }

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

    /// Open the find overlay.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Close the find overlay.
    pub fn close(&mut self) {
        self.open = false;
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
    /// Escape key behavior: close find if open, otherwise fall back.
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
            if state.ui().find_ui.is_open() {
                state.ui_mut().find_ui.close();
                Task::none()
            } else {
                state.ui_mut().find_ui.open();
                refresh_matches(state);
                focus(find_query_input_id())
            }
        }
        | FindMessage::Open => {
            state.ui_mut().find_ui.open();
            refresh_matches(state);
            focus(find_query_input_id())
        }
        | FindMessage::Close => {
            state.ui_mut().find_ui.close();
            Task::none()
        }
        | FindMessage::Escape => {
            if state.ui().find_ui.is_open() {
                state.ui_mut().find_ui.close();
                return Task::none();
            }
            AppState::update(
                state,
                Message::FriendPanel(FriendPanelMessage::CancelEditingFriendPerspective),
            )
        }
        | FindMessage::QueryChanged(query) => {
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
            if !state.ui().find_ui.is_open() || !state.ui().find_ui.is_current_revision(revision) {
                return Task::none();
            }
            refresh_matches(state);
            Task::none()
        }
        | FindMessage::JumpSelected => jump_to_selected(state),
        | FindMessage::JumpNext => {
            if !state.ui().find_ui.is_open() {
                return Task::none();
            }
            state.ui_mut().find_ui.select_next();
            jump_to_selected(state)
        }
        | FindMessage::JumpPrevious => {
            if !state.ui().find_ui.is_open() {
                return Task::none();
            }
            state.ui_mut().find_ui.select_previous();
            jump_to_selected(state)
        }
        | FindMessage::JumpToIndex(index) => {
            if !state.ui().find_ui.is_open() {
                return Task::none();
            }
            state.ui_mut().find_ui.select_index(index);
            jump_to_selected(state)
        }
    }
}

/// Render the floating find overlay.
pub fn floating_overlay<'a>(state: &'a AppState) -> Element<'a, Message> {
    if !state.ui().find_ui.is_open() {
        return container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let title = text(t!("ui_find").to_string()).font(theme::INTER).size(theme::FIND_TITLE_SIZE);
    let count_label = if state.ui().find_ui.query().trim().is_empty() {
        t!("find_hint_type").to_string()
    } else {
        t!("find_results_count", count = state.ui().find_ui.matches().len()).to_string()
    };

    let controls = row![]
        .spacing(theme::PANEL_BUTTON_GAP)
        .align_y(Alignment::Center)
        .push(text(count_label).size(theme::FIND_META_SIZE).style(theme::spine_text))
        .push(
            button(
                text(t!("find_prev").to_string()).font(theme::INTER).size(theme::FIND_META_SIZE),
            )
            .style(theme::action_button)
            .on_press(Message::Find(FindMessage::JumpPrevious)),
        )
        .push(
            button(
                text(t!("find_next").to_string()).font(theme::INTER).size(theme::FIND_META_SIZE),
            )
            .style(theme::action_button)
            .on_press(Message::Find(FindMessage::JumpNext)),
        )
        .push(
            button(text(t!("ui_close").to_string()).font(theme::INTER).size(theme::FIND_META_SIZE))
                .style(theme::action_button)
                .on_press(Message::Find(FindMessage::Close)),
        );

    let placeholder = t!("find_placeholder").to_string();
    let query_input = text_input(placeholder.as_str(), state.ui().find_ui.query())
        .id(find_query_input_id())
        .on_input(|query| Message::Find(FindMessage::QueryChanged(query)))
        .on_submit(Message::Find(FindMessage::JumpSelected))
        .size(theme::FIND_QUERY_SIZE)
        .padding(theme::FIND_QUERY_PAD);

    let result_list: Element<'a, Message> = if state.ui().find_ui.query().trim().is_empty() {
        container(text(t!("find_hint_empty").to_string()).style(theme::spine_text))
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .width(Length::Fill)
            .into()
    } else if state.ui().find_ui.matches().is_empty() {
        container(text(t!("find_no_results").to_string()).style(theme::spine_text))
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .width(Length::Fill)
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

            let row_container = container(row_content)
                .width(Length::Fill)
                .padding(Padding::from([theme::FIND_RESULT_PAD_V, theme::FIND_RESULT_PAD_H]));
            let row_container = if state.ui().find_ui.selected_index() == Some(index) {
                row_container.style(theme::friend_picker_hover)
            } else {
                row_container
            };

            rows = rows.push(
                button(row_container)
                    .style(theme::action_button)
                    .padding(0)
                    .width(Length::Fill)
                    .on_press(Message::Find(FindMessage::JumpToIndex(index))),
            );
        }

        scrollable(rows).height(Length::Fixed(theme::FIND_RESULT_LIST_HEIGHT)).into()
    };

    let viewport_width = state.ui().window_size.width;
    let viewport_height = state.ui().window_size.height;
    let panel_width = if viewport_width > 0.0 {
        (viewport_width - (theme::FIND_PANEL_MARGIN * 2.0)).min(theme::FIND_PANEL_MAX_WIDTH)
    } else {
        theme::FIND_PANEL_MAX_WIDTH
    };
    let panel_top_offset = if viewport_height > 0.0 {
        (viewport_height * theme::FIND_PANEL_TOP_RATIO).max(theme::FIND_PANEL_MARGIN)
    } else {
        theme::FIND_PANEL_MARGIN
    };

    let panel = container(
        column![]
            .spacing(theme::PANEL_INNER_GAP)
            .push(
                row![
                    title,
                    container(controls)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Right)
                ]
                .align_y(Alignment::Center),
            )
            .push(query_input)
            .push(result_list),
    )
    .style(theme::draft_panel)
    .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
    .width(Length::Fixed(panel_width));

    container(
        container(panel).padding(Padding::new(theme::FIND_PANEL_MARGIN).top(panel_top_offset)),
    )
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Top)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn refresh_matches(state: &mut AppState) {
    let query = state.ui().find_ui.query().trim();
    let matches = if query.is_empty() { vec![] } else { state.store.find_block_point(query) };
    state.ui_mut().find_ui.replace_matches(matches);
}

fn jump_to_selected(state: &mut AppState) -> Task<Message> {
    if !state.ui().find_ui.is_open() {
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
    focus(find_query_input_id())
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
        assert!(!state.ui().find_ui.is_open());

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Toggle));
        assert!(state.ui().find_ui.is_open());

        let _ = AppState::update(&mut state, Message::Find(FindMessage::Toggle));
        assert!(!state.ui().find_ui.is_open());
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
}
