//! Link-input panel for searching the filesystem and appending a [`PointLink`]
//! to a block's point.
//!
//! Entered by directly typing `@` at the end of a point editor, or by pressing
//! the "Add Link" button in the action bar / context menu. Shows a floating
//! panel with fuzzy filesystem search starting from `$HOME`.
//!
//! Confirming a directory drills into that directory (file-explorer style).
//! Confirming a file appends a new reference link via [`PointLink::infer`]; the
//! block's text is unchanged.
//!
//! # User interaction flow
//!
//! 1. User focuses a block and types `@` at the end of a point editor, or
//!    presses the "Add Link" action.
//! 2. The `@` is detected in [`crate::app::edit`] (PointEdited handler),
//!    which removes the trigger character from the editor and emits
//!    [`LinkModeMessage::Enter`].
//! 3. The panel opens, showing `$HOME` entries. The search input is focused.
//! 4. Typing narrows the candidates via filesystem-backed path completion.
//! 5. **Confirm** (Enter or click):
//!    - directories open in-place and refresh candidates,
//!    - files are appended to the block's `links` vec as a new reference link.
//!    The text editor is unaffected.
//! 6. **Cancel** (Escape): exits link mode with no changes.
//! 7. **Double-`@`**: typing `@` as the first character in the search input
//!    exits link mode and appends a literal `@` to the block's point editor.
//!    Because link entry is keyed off the original insert action, this buffer
//!    synchronization does not immediately re-open the panel.
//!
//! # Design rationale
//!
//! - **Filesystem only**: URL link sources may be added later. The panel
//!   currently enumerates local directories synchronously. This is acceptable
//!   for interactive directory browsing but may need async I/O for deep trees.
//! - **Absolute paths**: selected paths are stored as absolute strings. This
//!   simplifies display and image loading but makes documents non-portable.
//!   May be revisited with relative-path support later.
//! - **No validation**: broken links are not detected or flagged.
//!   The reference row always renders, even if the target does not exist.

use std::path::{Path, PathBuf};

use iced::widget::{
    Id, column,
    operation::{focus, move_cursor_to_end},
    scrollable, text, text_input, tooltip,
};
use iced::{Element, Length, Task};

use crate::app::{AppState, DocumentMode, EditMessage, LinkModeMessage, Message};
use crate::component::floating_panel::{self, PanelHeader, SelectableRow};
use crate::component::icon_button::IconButton;
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
            let filesystem = LinkFilesystem::new();
            state.ui_mut().document_mode = DocumentMode::LinkInput;
            state.ui_mut().reference_panel.link_panel = crate::app::LinkPanelState {
                block_id: Some(block_id),
                query: String::new(),
                candidates: filesystem.list_home_entries(),
                selected_index: 0,
            };
            // Auto-focus the search input.
            focus(link_query_input_id())
        }
        | LinkModeMessage::QueryChanged(query) => {
            // Double-@ escape: typing `@` as the first character in the link
            // panel query exits link mode and appends a literal `@` to the
            // block's point editor without re-triggering link mode.
            if query == "@" {
                if let Some(block_id) = state.ui().reference_panel.link_panel.block_id {
                    let mut point_text = state.store.point(&block_id).unwrap_or_default();
                    point_text.push('@');
                    state.store.update_point(&block_id, point_text.clone());
                    state.editor_buffers.set_text(&block_id, &point_text);
                    state.persist_with_context("append literal @");
                    exit_link_mode(state);
                    return refocus_point_editor_at_end(state, block_id);
                }
                exit_link_mode(state);
                return Task::none();
            }

            let filesystem = LinkFilesystem::new();
            let candidates = filesystem.search(&query);
            state.ui_mut().reference_panel.link_panel.selected_index = 0;
            state.ui_mut().reference_panel.link_panel.candidates = candidates;
            state.ui_mut().reference_panel.link_panel.query = query;
            Task::none()
        }
        | LinkModeMessage::SelectPrevious => {
            let panel = &mut state.ui_mut().reference_panel.link_panel;
            if panel.selected_index > 0 {
                panel.selected_index -= 1;
            }
            Task::none()
        }
        | LinkModeMessage::SelectNext => {
            let panel = &mut state.ui_mut().reference_panel.link_panel;
            if !panel.candidates.is_empty() {
                panel.selected_index = (panel.selected_index + 1).min(panel.candidates.len() - 1);
            }
            Task::none()
        }
        | LinkModeMessage::Confirm => confirm_selection(state, None),
        | LinkModeMessage::ConfirmCandidate(index) => confirm_selection(state, Some(index)),
        | LinkModeMessage::Cancel => {
            exit_link_mode(state);
            Task::none()
        }
    }
}

/// Reset link panel state and return to normal document mode.
fn exit_link_mode(state: &mut AppState) {
    state.ui_mut().document_mode = DocumentMode::Normal;
    state.ui_mut().reference_panel.link_panel = crate::app::LinkPanelState::default();
}

/// Restore focus to a point editor and place the caret at the end.
///
/// The cursor is moved immediately in editor state and again via
/// [`EditMessage::SetCursor`] after focus transfer so runtime focus changes do
/// not overwrite the final caret position.
fn refocus_point_editor_at_end(
    state: &mut AppState, block_id: crate::store::BlockId,
) -> Task<Message> {
    state.set_focus(block_id);
    state.editor_buffers.ensure_block(&state.store, &block_id);

    let (line, column_byte) = if let Some(content) = state.editor_buffers.get_mut(&block_id) {
        let mut line = content.line_count().saturating_sub(1);
        while content.line(line).is_none() && line > 0 {
            line = line.saturating_sub(1);
        }
        let column_byte = content.line(line).map(|line| line.text.len()).unwrap_or(0);
        content.move_to(iced::widget::text_editor::Cursor {
            position: iced::widget::text_editor::Position { line, column: column_byte },
            selection: None,
        });
        (line, column_byte)
    } else {
        tracing::warn!(
            block_id = ?block_id,
            "skipped immediate point-editor refocus because editor buffer is missing"
        );
        (0, 0)
    };

    let set_cursor = Task::done(Message::Edit(EditMessage::SetCursor {
        block_id,
        line,
        column_byte,
        seek_visual_end: false,
    }));

    if let Some(widget_id) = state.editor_buffers.widget_id(&block_id).cloned() {
        return Task::batch([iced::widget::operation::focus(widget_id), set_cursor]);
    }

    set_cursor
}

/// Confirm the current selection. Directories are opened in-place, files are
/// added as links.
fn confirm_selection(state: &mut AppState, requested_index: Option<usize>) -> Task<Message> {
    let (block_id, query, candidates, selected_index) = {
        let panel = &state.ui().reference_panel.link_panel;
        (panel.block_id, panel.query.clone(), panel.candidates.clone(), panel.selected_index)
    };

    let Some(block_id) = block_id else {
        exit_link_mode(state);
        return Task::none();
    };

    let filesystem = LinkFilesystem::new();
    let Some(path) = resolve_confirmation_target(
        &query,
        &candidates,
        selected_index,
        requested_index,
        &filesystem,
    ) else {
        exit_link_mode(state);
        return Task::none();
    };

    if path.is_dir() {
        open_directory(state, &filesystem, &path)
    } else {
        append_file_link(state, block_id, &path);
        Task::none()
    }
}

/// Pick the filesystem path that should be confirmed.
///
/// Priority:
/// 1. Explicit clicked candidate (`requested_index`),
/// 2. keyboard-selected candidate (`selected_index`),
/// 3. exact typed path, if it exists.
///
/// Note: selecting candidates before exact query path keeps Enter behavior
/// explorer-like when the current query already points to an open directory.
fn resolve_confirmation_target(
    query: &str, candidates: &[PathBuf], selected_index: usize, requested_index: Option<usize>,
    filesystem: &LinkFilesystem,
) -> Option<PathBuf> {
    if let Some(index) = requested_index {
        return candidates.get(index).cloned().or_else(|| filesystem.resolve_existing_path(query));
    }

    candidates.get(selected_index).cloned().or_else(|| filesystem.resolve_existing_path(query))
}

/// Open a directory in the panel and refresh candidates without leaving link mode.
fn open_directory(
    state: &mut AppState, filesystem: &LinkFilesystem, directory: &Path,
) -> Task<Message> {
    let query = filesystem.directory_query(directory);
    let candidates = filesystem.search(&query);
    let panel = &mut state.ui_mut().reference_panel.link_panel;
    panel.query = query;
    panel.candidates = candidates;
    panel.selected_index = 0;
    tracing::info!(path = %directory.display(), "link panel opened directory");
    Task::batch([focus(link_query_input_id()), move_cursor_to_end(link_query_input_id())])
}

/// Append a file link to the selected block and close link mode.
fn append_file_link(state: &mut AppState, block_id: crate::store::BlockId, path: &Path) {
    let href = path.to_string_lossy().to_string();
    let link = PointLink::infer(href);
    state.store.add_link_to_point(&block_id, link);
    state.persist_with_context("add link");
    tracing::info!(block_id = ?block_id, path = %path.display(), "link panel added file link");
    // Editor buffer is kept: the text editor is always present alongside reference links.
    exit_link_mode(state);
}

/// Render the link-input floating overlay. Returns an invisible spacer when
/// the mode is not [`DocumentMode::LinkInput`].
pub fn floating_overlay<'a>(state: &'a AppState) -> Element<'a, Message> {
    if !matches!(state.ui().document_mode, DocumentMode::LinkInput) {
        return floating_panel::invisible_spacer();
    }

    let panel = &state.ui().reference_panel.link_panel;
    let filesystem = LinkFilesystem::new();
    let viewport_width = state.ui().window_size.width;
    let viewport_height = state.ui().window_size.height;

    // --- Title row ---
    let title = text(t!("link_panel_title")).size(theme::FIND_QUERY_SIZE);
    let close_btn = tooltip(
        IconButton::panel_close().on_press(Message::LinkMode(LinkModeMessage::Cancel)),
        text(t!("ui_close").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
        tooltip::Position::Bottom,
    )
    .style(theme::tooltip)
    .padding(theme::TOOLTIP_PAD)
    .gap(theme::TOOLTIP_GAP);
    let title_row = PanelHeader::new(title, close_btn);

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
        let display = filesystem.abbreviate_path(path);
        let label = text(display).size(theme::FIND_RESULT_POINT_SIZE);
        rows = rows.push(SelectableRow::new(
            label,
            i == panel.selected_index,
            Message::LinkMode(LinkModeMessage::ConfirmCandidate(i)),
        ));
    }

    let result_list = scrollable(rows).height(Length::Fixed(theme::LINK_PANEL_LIST_HEIGHT));

    // --- Hint ---
    let hint =
        text(t!("link_panel_hint")).size(theme::FIND_RESULT_META_SIZE).style(theme::spine_text);

    let panel_content =
        column![title_row, input, result_list, hint].spacing(theme::FLOATING_PANEL_SECTION_GAP);

    floating_panel::wrap(panel_content, viewport_width, viewport_height)
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Filesystem access helper for link-panel path browsing and completion.
#[derive(Debug, Clone)]
struct LinkFilesystem {
    home_dir: Option<PathBuf>,
    working_dir: PathBuf,
}

impl LinkFilesystem {
    /// Construct from process environment (`$HOME` and current working directory).
    fn new() -> Self {
        let home_dir = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
        let working_dir = std::env::current_dir()
            .ok()
            .or_else(|| home_dir.clone())
            .unwrap_or_else(|| PathBuf::from(std::path::MAIN_SEPARATOR.to_string()));
        Self { home_dir, working_dir }
    }

    /// Build deterministic roots for tests.
    #[cfg(test)]
    fn with_roots(home_dir: Option<PathBuf>, working_dir: PathBuf) -> Self {
        Self { home_dir, working_dir }
    }

    /// List entries in `$HOME` (non-recursive, sorted).
    fn list_home_entries(&self) -> Vec<PathBuf> {
        let Some(home) = self.home_dir.as_ref() else {
            return Vec::new();
        };
        read_dir_sorted(home)
    }

    /// Search candidate entries for the query.
    ///
    /// Query behavior:
    /// - empty query lists `$HOME`,
    /// - `~` and `~/...` resolve against `$HOME`,
    /// - relative paths resolve against current working directory,
    /// - plain words (no separators) filter `$HOME` entries.
    ///
    /// Matching ranks `starts_with` first, then `contains`, then subsequence.
    ///
    /// Note: synchronous listing keeps the implementation simple and
    /// deterministic inside iced update handlers.
    fn search(&self, query: &str) -> Vec<PathBuf> {
        let normalized_query = query.trim();
        if normalized_query.is_empty() {
            return self.list_home_entries();
        }

        let Some(query_path) = self.query_to_path(normalized_query) else {
            return Vec::new();
        };
        let Some(request) = self.resolve_listing_request(normalized_query, &query_path) else {
            return rank_entries(self.list_home_entries(), normalized_query);
        };

        let entries = read_dir_sorted(&request.directory);
        rank_entries(entries, &request.fragment)
    }

    /// Resolve the query to an existing filesystem path (if any).
    fn resolve_existing_path(&self, query: &str) -> Option<PathBuf> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        let path = self.query_to_path(query)?;
        path.exists().then_some(path)
    }

    /// Format a directory path as a query string with trailing separator.
    fn directory_query(&self, directory: &Path) -> String {
        let mut query = self.path_to_query(directory);
        if !query_ends_with_separator(&query) {
            query.push(std::path::MAIN_SEPARATOR);
        }
        query
    }

    /// Abbreviate a path for display, replacing `$HOME` with `~`.
    fn abbreviate_path(&self, path: &Path) -> String {
        let mut display = self.path_to_query(path);
        if path.is_dir() && !query_ends_with_separator(&display) {
            display.push(std::path::MAIN_SEPARATOR);
        }
        display
    }

    /// Expand a typed query into an absolute path candidate.
    fn query_to_path(&self, query: &str) -> Option<PathBuf> {
        if query == "~" {
            return self.home_dir.clone();
        }

        if let Some(suffix) = query.strip_prefix("~/").or_else(|| query.strip_prefix("~\\")) {
            return self.home_dir.as_ref().map(|home| home.join(suffix));
        }

        if query.starts_with('~') {
            // Unsupported shell forms such as `~other`.
            return None;
        }

        let path = PathBuf::from(query);
        if path.is_absolute() {
            return Some(path);
        }

        let has_separator = query.contains('/') || query.contains('\\');
        if has_separator || query.starts_with('.') {
            return Some(self.working_dir.join(path));
        }

        Some(self.home_dir.clone().unwrap_or_else(|| self.working_dir.clone()).join(path))
    }

    /// Convert an absolute path into display/query form, abbreviating `$HOME`.
    fn path_to_query(&self, path: &Path) -> String {
        if let Some(home) = self.home_dir.as_ref() {
            if let Ok(relative) = path.strip_prefix(home) {
                if relative.as_os_str().is_empty() {
                    return "~".to_string();
                }
                return format!("~{}{}", std::path::MAIN_SEPARATOR, relative.display());
            }
        }
        path.display().to_string()
    }

    /// Resolve which directory should be listed and which fragment should be filtered.
    fn resolve_listing_request(&self, query: &str, query_path: &Path) -> Option<ListingRequest> {
        if query_ends_with_separator(query) || query_path.is_dir() {
            return query_path.is_dir().then(|| ListingRequest {
                directory: query_path.to_path_buf(),
                fragment: String::new(),
            });
        }

        let parent = query_path.parent()?;
        if !parent.is_dir() {
            return None;
        }
        let fragment = query_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(ListingRequest { directory: parent.to_path_buf(), fragment })
    }
}

/// Directory listing context: where to list and what fragment to match.
#[derive(Debug)]
struct ListingRequest {
    directory: PathBuf,
    fragment: String,
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

/// Rank directory entries by match quality against a user-typed fragment.
fn rank_entries(entries: Vec<PathBuf>, fragment: &str) -> Vec<PathBuf> {
    let needle = fragment.to_lowercase();
    if needle.is_empty() {
        return entries;
    }

    let mut scored: Vec<(u8, bool, String, PathBuf)> = entries
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().to_lowercase();
            let rank = if name.starts_with(&needle) {
                0
            } else if name.contains(&needle) {
                1
            } else if is_subsequence(&needle, &name) {
                2
            } else {
                return None;
            };
            Some((rank, !path.is_dir(), name, path))
        })
        .collect();

    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
    scored.into_iter().map(|(_, _, _, path)| path).collect()
}

/// Return true if all chars in `needle` appear in `haystack` in order.
fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut needle_chars = needle.chars();
    let mut next = needle_chars.next();
    for ch in haystack.chars() {
        if Some(ch) == next {
            next = needle_chars.next();
            if next.is_none() {
                return true;
            }
        }
    }
    next.is_none()
}

/// Cross-platform "ends with path separator" check for typed queries.
fn query_ends_with_separator(query: &str) -> bool {
    query.ends_with('/') || query.ends_with('\\')
}

#[cfg(test)]
mod tests {
    use super::{LinkFilesystem, resolve_confirmation_target};
    use crate::app::{AppState, DocumentMode, LinkModeMessage};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn double_at_escape_refocuses_point_editor_at_end() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, String::new());
        state.editor_buffers.set_text(&root, "");

        let _ = super::handle(&mut state, LinkModeMessage::Enter(root));
        let _ = super::handle(&mut state, LinkModeMessage::QueryChanged("@".to_string()));

        assert_eq!(state.ui().document_mode, DocumentMode::Normal);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
        assert_eq!(state.store.point(&root).as_deref(), Some("@"));

        let cursor =
            state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 1);
    }

    #[test]
    fn double_at_escape_appends_literal_at_to_existing_point() {
        let (mut state, root) = AppState::test_state();
        state.store.update_point(&root, "existing".to_string());
        state.editor_buffers.set_text(&root, "existing");

        let _ = super::handle(&mut state, LinkModeMessage::Enter(root));
        let _ = super::handle(&mut state, LinkModeMessage::QueryChanged("@".to_string()));

        assert_eq!(state.ui().document_mode, DocumentMode::Normal);
        assert_eq!(state.store.point(&root).as_deref(), Some("existing@"));

        let cursor =
            state.editor_buffers.get(&root).expect("editor content exists").cursor().position;
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 9);
    }

    #[test]
    fn plain_query_filters_home_entries_with_prefix_first() {
        let home = tempdir().expect("create home dir");
        fs::create_dir(home.path().join("Documents")).expect("create Documents");
        fs::create_dir(home.path().join("Downloads")).expect("create Downloads");
        fs::write(home.path().join("todo.txt"), b"todo").expect("create todo file");
        fs::write(home.path().join("notes.txt"), b"notes").expect("create notes file");

        let filesystem =
            LinkFilesystem::with_roots(Some(home.path().to_path_buf()), home.path().to_path_buf());
        let results = filesystem.search("do");
        let names: Vec<String> = results
            .into_iter()
            .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
            .collect();

        assert_eq!(names, vec!["Documents", "Downloads", "todo.txt"]);
    }

    #[test]
    fn resolve_existing_path_expands_tilde() {
        let home = tempdir().expect("create home dir");
        let file = home.path().join("design.md");
        fs::write(&file, b"content").expect("create design file");

        let filesystem =
            LinkFilesystem::with_roots(Some(home.path().to_path_buf()), home.path().to_path_buf());
        let resolved = filesystem.resolve_existing_path("~/design.md");

        assert_eq!(resolved, Some(file));
    }

    #[test]
    fn relative_query_uses_working_directory() {
        let home = tempdir().expect("create home dir");
        let work = tempdir().expect("create work dir");
        fs::create_dir(work.path().join("assets")).expect("create assets dir");
        fs::write(work.path().join("atlas.md"), b"atlas").expect("create atlas file");

        let filesystem =
            LinkFilesystem::with_roots(Some(home.path().to_path_buf()), work.path().to_path_buf());
        let results = filesystem.search("./a");
        let names: Vec<String> = results
            .into_iter()
            .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
            .collect();

        assert_eq!(names, vec!["assets", "atlas.md"]);
    }

    #[test]
    fn directory_query_abbreviates_home_and_appends_separator() {
        let home = tempdir().expect("create home dir");
        let docs = home.path().join("docs");
        fs::create_dir(&docs).expect("create docs dir");

        let filesystem =
            LinkFilesystem::with_roots(Some(home.path().to_path_buf()), PathBuf::from("/tmp"));
        let query = filesystem.directory_query(&docs);

        assert_eq!(query, "~/docs/");
    }

    #[test]
    fn confirmation_prefers_selected_candidate_over_exact_directory_query() {
        let home = tempdir().expect("create home dir");
        let root = home.path().join("root");
        let child = root.join("child");
        fs::create_dir(&root).expect("create root dir");
        fs::create_dir(&child).expect("create child dir");

        let filesystem =
            LinkFilesystem::with_roots(Some(home.path().to_path_buf()), home.path().to_path_buf());
        let query = filesystem.directory_query(&root);
        let candidates = vec![child.clone()];

        let target = resolve_confirmation_target(&query, &candidates, 0, None, &filesystem);
        assert_eq!(target, Some(child));
    }
}
