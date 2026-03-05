//! Transient UI state types and document interaction modes.
//!
//! These types represent ephemeral interaction state that is not persisted
//! with the document. See [`TransientUiState`] for the main grouping.

use crate::store::BlockId;
use iced::keyboard;
use std::collections::BTreeSet;

use super::find_panel::FindUiState;

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
    /// Entered by typing `@` in an empty point editor. The mode shows a
    /// floating panel with fuzzy filesystem search. Selecting a candidate
    /// converts the block's point to a [`PointLink`](crate::store::PointLink).
    LinkInput,
    /// Archive panel: browse and permanently delete archived block subtrees.
    Archive,
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
    pub selected_index: usize,
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

/// UI focus state: keyboard focus + overflow menu state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusState {
    /// The block that currently has keyboard focus.
    pub block_id: BlockId,
    /// Whether the overflow menu is open for this block.
    pub overflow_open: bool,
}

/// UI singleton state: transient interaction state not persisted with the document.
///
/// This struct groups ephemeral UI-only state such as focus, hover feedback,
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
    /// Whether the keyboard-shortcuts help banner is visible.
    pub show_shortcut_help: bool,
    /// Whether the current theme is dark.
    ///
    /// Initialized from persisted app config when available; otherwise from
    /// system appearance. Runtime system theme-change events only apply while
    /// no persisted override exists.
    pub is_dark: bool,
    /// The friend block currently being hovered in the Friends Panel.
    ///
    /// When `Some`, the corresponding block in the document tree is highlighted
    /// to help users identify the friend's location. The highlight is cleared
    /// when hover exits or the friend panel is closed.
    ///
    /// # Visibility Constraint
    ///
    /// The highlight is only applied if the friend block is currently visible
    /// in the document tree (not collapsed and within the current navigation layer).
    /// If the friend is hidden, no visual feedback is shown to avoid confusing
    /// the user with a highlight that points to nothing visible.
    pub hovered_friend_block: Option<BlockId>,
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
    /// (target_block_id, friend_block_id) currently being edited inline.
    pub editing_friend_perspective: Option<(BlockId, BlockId)>,
    /// Current text input value for friend perspective inline editing.
    pub editing_friend_perspective_input: Option<String>,
    /// Context menu state: (block_id, position) when visible.
    pub context_menu: Option<(BlockId, iced::Point)>,
    /// Last known cursor position for context menu placement.
    pub cursor_position: Option<iced::Point>,
    /// State for the link-input panel (filesystem search).
    pub link_panel: LinkPanelState,
    /// Set of blocks whose link chips are expanded (showing inline preview).
    ///
    /// Transient: not persisted, reset on restart.
    pub expanded_links: BTreeSet<BlockId>,
}
