//! Transient UI state types and document interaction modes.
//!
//! These types represent ephemeral interaction state that is not persisted
//! with the document. See [`TransientUiState`] for the main grouping.

use crate::store::BlockId;
use iced::keyboard;
use iced::widget::text_editor;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::find_panel::FindUiState;

/// Common selection behavior for search/browse panels that maintain a query and
/// a list of candidates with a highlighted entry.
///
/// Implementors must provide three primitive operations; `select_next`,
/// `select_previous`, and `select_index` are supplied as default methods.
pub trait SelectableList {
    /// Total number of items currently available.
    fn item_count(&self) -> usize;
    /// Index of the currently highlighted item, or `None` when the list is empty.
    fn selection(&self) -> Option<usize>;
    /// Set the highlighted index. Receives `None` when the list becomes empty.
    fn set_selection(&mut self, index: Option<usize>);

    /// Advance to the next item, wrapping from the last item back to the first.
    fn select_next(&mut self) {
        let count = self.item_count();
        if count == 0 {
            self.set_selection(None);
            return;
        }
        let next = match self.selection() {
            | Some(i) => (i + 1) % count,
            | None => 0,
        };
        self.set_selection(Some(next));
    }

    /// Advance to the previous item, wrapping from the first item back to the last.
    fn select_previous(&mut self) {
        let count = self.item_count();
        if count == 0 {
            self.set_selection(None);
            return;
        }
        let prev = match self.selection() {
            | Some(0) | None => count - 1,
            | Some(i) => i - 1,
        };
        self.set_selection(Some(prev));
    }

    /// Highlight a specific item by index, if it is in range.
    fn select_index(&mut self, index: usize) {
        if index < self.item_count() {
            self.set_selection(Some(index));
        }
    }
}

/// Document interaction mode: normal editing vs picking a friend block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentMode {
    /// Normal block editing mode.
    #[default]
    Normal,
    /// Find mode.
    Find,
    /// Picking a friend block to add to the focused block.
    PickFriend,
    /// Selecting one or more blocks for keyboard-driven batch actions.
    ///
    /// Current scope only supports backspace-triggered block deletion. The mode
    /// exists as a dedicated state so future multi-select interactions can be
    /// added without overloading `Normal` behavior.
    Multiselect,
    /// Link input mode: searching the filesystem for a path to link.
    ///
    /// Entered by directly typing `@` at the end of a point editor. The mode
    /// shows a floating panel with fuzzy filesystem search. Selecting a
    /// candidate converts the block's point to a
    /// [`PointLink`](crate::store::PointLink).
    LinkInput,
    /// Archive panel: browse and permanently delete archived block subtrees.
    Archive,
}

impl DocumentMode {
    /// Toggle between `target` mode and [`Normal`](Self::Normal).
    ///
    /// If the current mode matches `target`, switches to `Normal`;
    /// otherwise switches to `target`.
    pub fn toggle(&mut self, target: Self) {
        *self = if *self == target { Self::Normal } else { target };
    }
}

/// Messages for the link-input panel.
#[derive(Debug, Clone)]
pub enum LinkModeMessage {
    /// Enter link mode for the given block.
    Enter(BlockId),
    /// The user changed the search query.
    QueryChanged(String),
    /// The user selected the current candidate (confirm).
    Confirm,
    /// The user clicked a specific candidate in the list.
    ConfirmCandidate(usize),
    /// The user pressed Up arrow.
    SelectPrevious,
    /// The user pressed Down arrow.
    SelectNext,
    /// Cancel link mode without changes.
    Cancel,
}

/// Transient state for the link-input panel.
///
/// Tracks the search query, filesystem candidates, and which candidate
/// is currently highlighted. Reset on mode exit.
#[derive(Debug, Clone, Default)]
pub struct LinkPanelState {
    /// The block whose point is being replaced by a link.
    pub block_id: Option<BlockId>,
    /// Current search query (path fragment).
    pub query: String,
    /// Candidate filesystem paths matching the query.
    pub candidates: Vec<std::path::PathBuf>,
    /// Index of the currently highlighted candidate.
    pub selected: usize,
}

impl SelectableList for LinkPanelState {
    fn item_count(&self) -> usize {
        self.candidates.len()
    }

    fn selection(&self) -> Option<usize> {
        if self.candidates.is_empty() { None } else { Some(self.selected) }
    }

    fn set_selection(&mut self, index: Option<usize>) {
        self.selected = index.unwrap_or(0);
    }

    /// Saturate at the last item rather than wrapping (file-browser feel).
    fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = (self.selected + 1).min(self.candidates.len() - 1);
        }
    }

    /// Saturate at 0 rather than wrapping.
    fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
}

/// Stable identifier for one transient probe panel instance.
///
/// Each click on the `Probe` toolbar action allocates a fresh id so repeated
/// clicks can append multiple independent probe panels under the same block.
///
/// Note: this id is intentionally local to one runtime session. It is not
/// serialized and does not need cross-restart stability because probe panel
/// shells are transient UI objects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbePanelId(pub u64);

/// Transient lifecycle state for one inline probe panel instance.
///
/// Probe panels are intentionally transient UI objects rather than persisted
/// store data. This keeps repeated toolbar clicks cheap and allows multiple
/// panels to coexist for the same block without complicating on-disk draft
/// schemas.
///
/// The struct models only the shell lifecycle of the probe surface:
/// - `instruction` is the editor-phase buffer;
/// - `inquiry` marks the transition into result/progress phase;
/// - `response` accumulates streamed or completed output;
/// - `is_probing` marks exclusive ownership of the in-flight request state for
///   that panel.
///
/// Note: persisted `instruction_draft` and `probe_draft` records still exist in
/// the store for workflow interoperability and backward compatibility, but
/// those records are not the source of truth for whether a probe panel is open.
#[derive(Debug, Clone)]
pub struct ProbePanelState {
    /// Stable id used to route panel-local messages and streamed probe results.
    pub id: ProbePanelId,
    /// Editor buffer for the instruction input shown in the panel header/body.
    pub instruction: text_editor::Content,
    /// The submitted probe question, when this panel has entered result phase.
    pub inquiry: Option<String>,
    /// Incrementally built or completed probe response text.
    pub response: String,
    /// Whether this panel currently owns the in-flight probe request for its block.
    pub is_probing: bool,
}

impl ProbePanelState {
    /// Create a fresh editor-phase probe panel.
    pub fn new(id: ProbePanelId) -> Self {
        Self {
            id,
            instruction: text_editor::Content::new(),
            inquiry: None,
            response: String::new(),
            is_probing: false,
        }
    }

    /// Whether the panel has left editor phase and is showing probe progress or result.
    pub fn is_result_phase(&self) -> bool {
        self.is_probing || self.inquiry.is_some()
    }
}

/// Active inline perspective editor in the reference panel.
///
/// Only one reference perspective editor may be open at a time. The enum keeps
/// the target identity and current input buffer coupled so callers cannot
/// accidentally update the wrong row type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencePerspectiveEditState {
    /// Inline perspective editor for one friend relation.
    Friend {
        /// Block whose reference panel owns the relation.
        target: BlockId,
        /// Friend block being described.
        friend_id: BlockId,
        /// Current transient input buffer.
        input: String,
    },
    /// Inline perspective editor for one point link.
    Link {
        /// Block whose reference panel owns the link.
        target: BlockId,
        /// Link index inside the point content.
        link_index: usize,
        /// Current transient input buffer.
        input: String,
    },
}

impl ReferencePerspectiveEditState {
    /// Replace the transient input buffer while preserving the edited target.
    pub fn set_input(&mut self, input: String) {
        match self {
            | Self::Friend { input: current, .. } | Self::Link { input: current, .. } => {
                *current = input;
            }
        }
    }

    /// Return the current transient input buffer.
    pub fn input(&self) -> &str {
        match self {
            | Self::Friend { input, .. } | Self::Link { input, .. } => input,
        }
    }
}

/// Transient UI state owned by friend and link reference surfaces.
///
/// The References panel and the link-input panel are converging toward a shared
/// inline reference workflow, so their highlight/edit/search/preview state
/// lives in one subtree instead of being scattered across [`TransientUiState`].
///
/// Note: these fields must remain transient. They provide interaction affordances
/// only and have no durable document meaning.
#[derive(Debug, Clone, Default)]
pub struct ReferencePanelUiState {
    /// The friend block currently highlighted from the References panel.
    ///
    /// When `Some`, the corresponding block in the document tree is highlighted
    /// to help users identify the friend's location. The highlight is cleared
    /// when the selection is cleared or the references panel is closed.
    ///
    /// Note: this is driven by a click on the friend row, not true pointer
    /// hover. The state is named after the visible effect so call sites do not
    /// imply hover semantics that the current UI does not provide.
    ///
    /// # Visibility Constraint
    ///
    /// The highlight is only applied if the friend block is currently visible
    /// in the document tree (not collapsed and within the current navigation layer).
    /// If the friend is hidden, no visual feedback is shown to avoid confusing
    /// the user with a highlight that points to nothing visible.
    pub highlighted_friend_block: Option<BlockId>,
    /// The currently active inline perspective editor, if any.
    pub editing_perspective: Option<ReferencePerspectiveEditState>,
    /// State for the link-input panel (filesystem search).
    pub link_panel: LinkPanelState,
    /// Per-block expanded reference-link index (showing inline preview).
    ///
    /// Maps a block id to the index of its currently expanded link row.
    /// At most one link row per block can be expanded at a time.
    /// Transient: not persisted, reset on restart.
    pub expanded_links: BTreeMap<BlockId, usize>,
}

/// Which top-level screen is active.
///
/// The document view is the default; settings is reached via a gear icon button
/// and dismissed with a back arrow or Escape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// The main tree-structured document editor.
    #[default]
    Document,
    /// The settings configuration screen.
    Settings,
}

/// Current window dimensions for responsive layout.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowSize {
    pub width: f32,
    #[allow(dead_code)]
    pub height: f32,
}

/// UI focus state: keyboard focus, overflow menu state, and ancestor lineage.
#[derive(Clone, Debug)]
pub struct FocusState {
    /// The block that currently has keyboard focus.
    pub block_id: BlockId,
    /// Whether the overflow menu is open for this block.
    pub overflow_open: bool,
    /// IDs of all ancestor blocks of the focused block, for lineage highlighting.
    pub ancestor_ids: HashSet<BlockId>,
}

/// UI singleton state: transient interaction state not persisted with the document.
///
/// This struct groups ephemeral UI-only state such as focus, highlight feedback,
/// inline editor buffers, and temporary confirmation/overflow toggles.
/// It is intentionally excluded from undo snapshots and on-disk persistence.
///
/// Access pattern for app modules:
/// - read through [`AppState::ui`](super::AppState::ui)
/// - write through [`AppState::ui_mut`](super::AppState::ui_mut)
///
/// # Design Decisions
///
/// ## Why a Separate Struct?
///
/// - Keeps `AppState` organized by separating persistent state from transient UI feedback
/// - Avoids cluttering undo snapshots with non-semantic UI state
/// - Makes it clear which fields are not serialized or persisted
///
/// ## Why Not Persisted?
///
/// - Focus/hover/inline editor UI state has no durable document meaning
/// - Resetting on reload is acceptable and expected behavior
/// - Keeps serialization lean and focused on user data
#[derive(Debug, Clone, Default)]
pub struct TransientUiState {
    /// Transient find-overlay state (query, matches, and selection).
    pub find_ui: FindUiState,
    /// UI focus state: keyboard focus + overflow menu state.
    pub focus: Option<FocusState>,
    /// Current document interaction mode (normal, pick-friend, multiselect).
    pub document_mode: DocumentMode,
    /// Block ids currently selected in multiselect mode.
    ///
    /// This set is only interpreted while `document_mode == Multiselect`.
    /// Outside multiselect mode it is cleared eagerly.
    pub multiselect_selected_blocks: BTreeSet<BlockId>,
    /// Anchor for Shift+click range selection in multiselect mode.
    /// Set when entering multiselect or on the last block affected by a click.
    pub multiselect_anchor: Option<BlockId>,
    /// Which top-level screen is currently shown.
    pub active_view: ViewMode,
    /// Current window dimensions for responsive layout.
    pub window_size: WindowSize,
    /// Last observed keyboard modifier state from global events.
    ///
    /// This is used to filter command-shortcut key leaks (for example,
    /// suppressing `Cmd/Ctrl+F` text insertion into active editors/inputs).
    pub keyboard_modifiers: keyboard::Modifiers,
    /// Preferred char column for consecutive vertical cursor navigation.
    ///
    /// This is set when processing `ArrowUp`/`ArrowDown` editor motions and
    /// cleared on non-vertical editor actions or focus changes. Storing the
    /// value as a Unicode scalar (char) column keeps traversal stable across
    /// mixed UTF-8 lines and prevents large horizontal jumps after crossing a
    /// short line.
    ///
    /// Lifecycle:
    /// - seeded from current caret column on first vertical move in a chain,
    /// - reused while vertical motion continues across lines/blocks,
    /// - reset when edit flow switches to non-vertical motion or explicit
    ///   focus change.
    ///
    /// Note: this field tracks horizontal intent only. Final visual-row
    /// placement for wrapped lines is resolved at runtime by editor motions in
    /// `app::edit::set_cursor`.
    ///
    /// Note: this field is transient by design and must never be persisted.
    pub vertical_cursor_preferred_column: Option<usize>,
    /// Whether the keyboard-shortcuts help banner is visible.
    pub show_shortcut_help: bool,
    /// Whether the current theme is dark.
    ///
    /// Initialized from persisted app config when available; otherwise from
    /// system appearance. Runtime system theme-change events only apply while
    /// no persisted override exists.
    pub is_dark: bool,
    /// Mount block id waiting for inline-all confirmation.
    ///
    /// The first click on "Inline all" arms this confirmation state for one
    /// block. Any unrelated message clears it. A second click on the same block
    /// performs the inline operation.
    pub pending_inline_mount_confirmation: Option<BlockId>,
    /// Mount block id whose path-operations overflow menu is open.
    ///
    /// This drives the mount-header overflow UI (move/inline/inline-all).
    /// Only one mount overflow is open at a time.
    pub mount_action_overflow_block: Option<BlockId>,
    /// Context menu state: (block_id, position) when visible.
    pub context_menu: Option<(BlockId, iced::Point)>,
    /// Last known cursor position for context menu placement.
    pub cursor_position: Option<iced::Point>,
    /// Transient state for friend and link reference panels.
    pub reference_panel: ReferencePanelUiState,
    /// Transient probe panels keyed by owning block.
    ///
    /// This map is the source of truth for visible probe panels. A block may
    /// simultaneously have an open References toggle panel and one or more
    /// probe panels because the two panel families now live in different
    /// lifecycle systems.
    pub probe_panels: BTreeMap<BlockId, Vec<ProbePanelState>>,
    /// Monotonic id source for allocating new probe panels.
    pub next_probe_panel_id: u64,
}
