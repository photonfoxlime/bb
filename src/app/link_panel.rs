//! Link-input panel for searching the filesystem and converting a block's
//! point into a [`PointLink`].
//!
//! Entered by typing `@` in an empty point editor. Shows a floating panel
//! with fuzzy filesystem search starting from `$HOME`. Selecting a candidate
//! converts the entire point to a link via [`PointLink::infer`].
//!
//! # User interaction flow
//!
//! 1. User focuses an empty block and types `@`.
//! 2. The `@` is detected in [`crate::app::edit`] (PointEdited handler),
//!    which clears the editor and emits [`LinkModeMessage::Enter`].
//! 3. The panel opens, showing `$HOME` entries. The search input is focused.
//! 4. Typing narrows the candidates via [`search_filesystem`].
//! 5. **Confirm** (Enter or click): the selected path replaces the block's
//!    point with a [`PointLink`]. The editor buffer is removed since link
//!    blocks render as chips, not text editors.
//! 6. **Cancel** (Escape): exits link mode with no changes.
//! 7. **Double-`@`**: typing `@` as the first character in the search input
//!    exits link mode and inserts a literal `@` into the block's point editor.
//!
//! # Design rationale
//!
//! - **Filesystem only**: URL link sources may be added later. The search
//!   currently enumerates local directories synchronously. This is acceptable
//!   for `$HOME`-level browsing but may need async I/O for deep trees.
//! - **Absolute paths**: selected paths are stored as absolute strings. This
//!   simplifies display and image loading but makes documents non-portable.
//!   May be revisited with relative-path support later.
//! - **No validation**: broken links are not detected or flagged.
//!   The chip always renders, even if the target does not exist.

use std::path::{Path, PathBuf};

use iced::widget::{
    Id, button, column, container, operation::focus, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::app::{AppState, DocumentMode, LinkModeMessage, Message};
use crate::store::PointLink;
use crate::theme;
use rust_i18n::t;

/// Widget ID for the link panel search input, used for auto-focus.
const LINK_QUERY_INPUT_ID: &str = "link-query-input";

/// Return the widget [`Id`] for the link panel search input.
fn link_query_input_id() -> Id {
    Id::new(LINK_QUERY_INPUT_ID)
}

/// Handle a [`LinkModeMessage`], mutating state and returning any follow-up
/// [`Task`].
pub fn handle(state: &mut AppState, message: LinkModeMessage) -> Task<Message> {
    match message {
        | LinkModeMessage::Enter(block_id) => {
            state.ui_mut().document_mode = DocumentMode::LinkInput;
            state.ui_mut().link_panel = crate::app::LinkPanelState {
                block_id: Some(block_id),
                query: String::new(),
                candidates: list_home_entries(),
                selected_index: 0,
            };
            // Auto-focus the search input.
            focus(link_query_input_id())
        }
        | LinkModeMessage::QueryChanged(query) => {
            // Double-@ escape: typing `@` as the first character in the link
            // panel query exits link mode and inserts a literal `@` into the
            // block's point editor.
            if query == "@" {
                if let Some(block_id) = state.ui().link_panel.block_id {
                    state.store.update_point(&block_id, "@".to_string());
                    state.editor_buffers.set_text(&block_id, "@");
                    state.persist_with_context("insert literal @");
                }
                exit_link_mode(state);
                return Task::none();
            }

            let candidates = search_filesystem(&query);
            state.ui_mut().link_panel.selected_index = 0;
            state.ui_mut().link_panel.candidates = candidates;
            state.ui_mut().link_panel.query = query;
            Task::none()
        }
        | LinkModeMessage::SelectPrevious => {
            let panel = &mut state.ui_mut().link_panel;
            if panel.selected_index > 0 {
                panel.selected_index -= 1;
            }
            Task::none()
        }
        | LinkModeMessage::SelectNext => {
            let panel = &mut state.ui_mut().link_panel;
            if !panel.candidates.is_empty() {
                panel.selected_index = (panel.selected_index + 1).min(panel.candidates.len() - 1);
            }
            Task::none()
        }
        | LinkModeMessage::Confirm => {
            let panel = &state.ui().link_panel;
            let selected = panel.candidates.get(panel.selected_index).cloned();

            if let (Some(block_id), Some(path)) = (panel.block_id, selected) {
                let href = path.to_string_lossy().to_string();
                let link = PointLink::infer(href);
                state.store.set_point_content(&block_id, crate::store::PointContent::Link(link));
                state.persist_with_context("convert to link");
                // Clear the editor buffer — link blocks render as chips, not
                // text editors, so the buffer would be stale.
                state.editor_buffers.remove_blocks(&[block_id]);
            }
            exit_link_mode(state);
            Task::none()
        }
        | LinkModeMessage::Cancel => {
            exit_link_mode(state);
            Task::none()
        }
    }
}

/// Reset link panel state and return to normal document mode.
fn exit_link_mode(state: &mut AppState) {
    state.ui_mut().document_mode = DocumentMode::Normal;
    state.ui_mut().link_panel = crate::app::LinkPanelState::default();
}

/// Render the link-input floating overlay. Returns an invisible spacer when
/// the mode is not [`DocumentMode::LinkInput`].
pub fn floating_overlay<'a>(state: &'a AppState) -> Element<'a, Message> {
    if !matches!(state.ui().document_mode, DocumentMode::LinkInput) {
        return container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let panel = &state.ui().link_panel;
    let viewport_width = state.ui().window_size.width;
    let viewport_height = state.ui().window_size.height;

    // --- Title row ---
    let title = text(t!("link_panel_title")).size(theme::FIND_QUERY_SIZE);
    let close_btn = button(text(t!("link_panel_close")).size(theme::FIND_RESULT_META_SIZE))
        .style(theme::action_button)
        .on_press(Message::LinkMode(LinkModeMessage::Cancel));
    let title_row =
        row![title, container(iced::widget::Space::new()).width(Length::Fill), close_btn]
            .align_y(Alignment::Center);

    // --- Search input ---
    let input = text_input(&t!("link_panel_placeholder"), &panel.query)
        .id(link_query_input_id())
        .on_input(|q| Message::LinkMode(LinkModeMessage::QueryChanged(q)))
        .on_submit(Message::LinkMode(LinkModeMessage::Confirm))
        .size(theme::FIND_QUERY_SIZE)
        .padding(theme::FIND_QUERY_PAD);

    // --- Candidate list ---
    let mut rows = column![].width(Length::Fill);

    for (i, path) in panel.candidates.iter().enumerate() {
        let display = abbreviate_path(path);
        let is_selected = i == panel.selected_index;

        let label = text(display).size(theme::FIND_RESULT_POINT_SIZE);
        let row_container = container(label)
            .width(Length::Fill)
            .padding(Padding::from([theme::FIND_RESULT_PAD_V, theme::FIND_RESULT_PAD_H]));
        let row_container = if is_selected {
            row_container.style(theme::friend_picker_hover)
        } else {
            row_container
        };

        rows = rows.push(
            button(row_container)
                .style(theme::action_button)
                .padding(Padding::ZERO)
                .width(Length::Fill)
                .on_press(Message::LinkMode(LinkModeMessage::Confirm)),
        );
    }

    let result_list = scrollable(rows).height(Length::Fixed(theme::LINK_PANEL_LIST_HEIGHT));

    // --- Hint ---
    let hint =
        text(t!("link_panel_hint")).size(theme::FIND_RESULT_META_SIZE).style(theme::spine_text);

    // --- Assemble panel ---
    let panel_content =
        column![title_row, input, result_list, hint].spacing(theme::PANEL_INNER_GAP);

    let panel_width = if viewport_width > 0.0 {
        (viewport_width - (theme::LINK_PANEL_MARGIN * 2.0)).min(theme::LINK_PANEL_MAX_WIDTH)
    } else {
        theme::LINK_PANEL_MAX_WIDTH
    };
    let panel_top = if viewport_height > 0.0 {
        (viewport_height * theme::LINK_PANEL_TOP_RATIO).max(theme::LINK_PANEL_MARGIN)
    } else {
        theme::LINK_PANEL_MARGIN
    };

    container(
        container(panel_content)
            .width(panel_width)
            .padding(Padding::new(theme::LINK_PANEL_CONTENT_PAD))
            .style(theme::draft_panel),
    )
    .padding(Padding::ZERO.top(panel_top))
    .align_x(Alignment::Center)
    .align_y(Alignment::Start)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// List entries in `$HOME` (non-recursive, sorted).
fn list_home_entries() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    read_dir_sorted(&home)
}

/// Resolve the user's home directory via the `directories` crate.
fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Search the filesystem based on the query string.
///
/// The search strategy has three tiers:
///
/// 1. **Empty query**: return all entries in `$HOME`.
/// 2. **Path prefix** (query ends with `/` or contains a valid parent dir):
///    list the matching directory and filter by the filename fragment.
///    For example, `/Users/foo/Do` shows entries in `/Users/foo/` whose
///    names contain `"do"` (case-insensitive).
/// 3. **Fallback**: fuzzy-filter `$HOME` entries by the query.
///
/// Note: all I/O is synchronous. This is acceptable for shallow directory
/// listings but would need async for deep recursive searches.
fn search_filesystem(query: &str) -> Vec<PathBuf> {
    if query.is_empty() {
        return list_home_entries();
    }

    let query_path = PathBuf::from(query);

    // If query ends with '/', treat it as a directory to list.
    if query.ends_with('/') || query.ends_with(std::path::MAIN_SEPARATOR) {
        if query_path.is_dir() {
            return read_dir_sorted(&query_path);
        }
    }

    // If the parent directory exists, list it and filter by the file stem.
    if let Some(parent) = query_path.parent() {
        if parent.is_dir() {
            let prefix = query_path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let entries = read_dir_sorted(parent);
            if prefix.is_empty() {
                return entries;
            }
            return entries
                .into_iter()
                .filter(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_lowercase().contains(&prefix))
                        .unwrap_or(false)
                })
                .collect();
        }
    }

    // Fallback: fuzzy-filter home entries.
    let lower_query = query.to_lowercase();
    list_home_entries()
        .into_iter()
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase().contains(&lower_query))
                .unwrap_or(false)
        })
        .collect()
}

/// Read a directory and return sorted entries, with directories listed first.
/// Hidden files (starting with `.`) are included but sorted after visible ones.
fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<PathBuf> = read_dir.filter_map(|e| e.ok().map(|e| e.path())).collect();

    entries.sort_by(|a, b| {
        let a_dir = a.is_dir();
        let b_dir = b.is_dir();
        // Directories first, then alphabetical.
        b_dir.cmp(&a_dir).then_with(|| a.file_name().cmp(&b.file_name()))
    });

    entries
}

/// Abbreviate a path for display, replacing `$HOME` with `~`.
fn abbreviate_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            let suffix = if path.is_dir() { "/" } else { "" };
            return format!("~/{}{}", relative.display(), suffix);
        }
    }
    let suffix = if path.is_dir() { "/" } else { "" };
    format!("{}{}", path.display(), suffix)
}
