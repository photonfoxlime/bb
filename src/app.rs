//! Application orchestration layer for the Iced UI.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Domain semantics are documented next to the owning handlers and state types.

mod action_bar;
mod config;
mod diff;
mod document;
mod editor_buffers;
mod error;
mod friends_panel;
mod find_panel;
mod instruction_panel;
mod llm_requests;
mod settings;
mod reduce;
mod expand;
mod structure;
mod overlay;
mod mount_file;
mod error_banner;
mod navigation;

use self::{
    action_bar::{
        ActionAvailability, ActionId, RowContext, ViewportBucket, action_to_message_by_id,
        build_action_bar_vm, project_for_viewport,
    },
    edit::EditMessage,
    editor_buffers::EditorBuffers,
    error::{AppError, ErrorMessage, UiError},
    error_banner::ErrorBanner,
    expand::ExpandMessage,
    find_panel::{FindMessage, FindUiState},
    friends_panel::FriendPanelMessage,
    instruction_panel::InstructionPanelMessage,
    llm_requests::{LlmRequests, RequestSignature},
    mount_file::MountFileMessage,
    navigation::{NavigationMessage, NavigationStack},
    overlay::OverlayMessage,
    reduce::ReduceMessage,
    settings::{SettingsMessage, SettingsState},
    shortcut::ShortcutMessage,
    structure::StructureMessage,
    undo_redo::UndoRedoMessage,
};
use crate::{
    i18n, llm,
    store::{BlockId, BlockPanelBarState, BlockStore, StoreLoadError},
    undo::UndoHistory,
};
use iced::{
    Element, Event, Subscription, Task, event, keyboard, system,
    widget::{self, text_editor},
    window,
};
use std::time::Duration;

pub use config::AppConfig;

/// Default capacity: 64 undo steps.
const UNDO_CAPACITY: usize = 64;
const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// All mutable application state for the iced Elm architecture.
///
/// Named LLM provider configurations with active provider selection.
/// async operation states, and transient UI state (overflow, focused/editing block ids).
///
/// Ownership split:
/// - `store`: authoritative graph, persisted drafts, mount runtime metadata.
/// - `editor_buffers`: widget-local text buffers + focus ids.
/// - persistence flags: recovery guard for unsafe-on-disk state plus a runtime
///   write gate for side-effect-free runs.
/// - `transient_ui`: non-persisted interaction state (focus, mode/view, hover, inline editors, popovers, find).
/// - `edit_session`: undo coalescing session tracker.
#[derive(Clone)]
pub struct AppState {
    /// Draft form state for the settings screen.
    pub settings: SettingsState,
    /// Persisted app preferences (e.g. optional locale). Loaded at startup from
    /// `<config_dir>/app.toml`; effective locale is derived via [`i18n::resolved_locale_from_config`].
    pub config: AppConfig,
    store: BlockStore,
    undo_history: UndoHistory<UndoSnapshot>,
    providers: llm::LlmProviders,
    errors: Vec<AppError>,
    llm_requests: LlmRequests,
    editor_buffers: EditorBuffers,
    /// Hard guard set when startup cannot trust persisted `blocks.json`.
    ///
    /// IMPORTANT: this protects potentially recoverable user data.
    /// When true, save-through is blocked so the app does not overwrite
    /// potentially recoverable user data with recovery-session edits.
    persistence_blocked: bool,
    /// Explicit write kill-switch for side-effect-free execution contexts.
    ///
    /// IMPORTANT: this is a test-only runtime flag so tests can opt in
    /// per `AppState` instance while keeping the normal persistence path
    /// compiled and typechecked in test builds.
    ///
    /// `AppState::load()` initializes this to `false`; tests explicitly set it
    /// to `true` in `test_state` to avoid touching on-disk `blocks.json`.
    persistence_write_disabled: bool,
    /// Transient UI singleton state (focus, mode/view, overlays, viewport, theme, find).
    ///
    /// Access this field via [`Self::ui`] and [`Self::ui_mut`] from app submodules.
    /// This keeps call sites consistent and centralizes expectations for
    /// non-persisted UI state usage.
    transient_ui: TransientUiState,
    /// Edit session: block currently coalescing point edits into a single undo entry.
    edit_session: Option<BlockId>,
    /// Navigation stack: tracks drill-down path through block subtrees.
    ///
    /// Enables "drilling down" into a block's children, showing only that
    /// subtree in the main view. The stack tracks the path from root to
    /// current view, with breadcrumbs rendered for quick navigation.
    ///
    /// Mount path hints are carried on navigation layers for breadcrumb
    /// context. The stack is part of the undo snapshot for consistency.
    navigation: NavigationStack,
}

impl AppState {
    /// Load startup state.
    ///
    /// Persistence safety policy:
    /// - missing `blocks.json` is treated as first-run default state,
    /// - load path/read/parse failures enter guarded recovery mode
    ///   (`persistence_blocked`),
    /// - recovery mode uses a blank one-root workspace and blocks save-through
    ///   to avoid overwriting unknown/corrupt on-disk state.
    pub fn load() -> Self {
        let providers = match llm::LlmProviders::load() {
            | Ok(p) => p,
            | Err(err) => {
                tracing::error!(%err, "failed to load LLM providers; using defaults");
                llm::LlmProviders::default()
            }
        };
        let mut errors = vec![];
        if let Err(err) = providers.resolve_active() {
            errors.push(AppError::Configuration(UiError::from_message(err)));
        }
        let (store, persistence_blocked, persistence_errors) =
            Self::startup_store_from_load_result(BlockStore::load());
        errors.extend(persistence_errors);
        let editor_buffers = EditorBuffers::from_store(&store);

        let config = crate::app::config::load();
        let system_is_dark = matches!(dark_light::detect(), Ok(dark_light::Mode::Dark));
        let is_dark = config.resolved_dark_mode(system_is_dark);
        tracing::info!(
            system_is_dark,
            config_dark_mode = ?config.dark_mode,
            is_dark,
            "resolved startup appearance"
        );
        let settings = SettingsState::from_providers(&providers, &config);
        let transient_ui = TransientUiState {
            is_dark,
            window_size: WindowSize::default(),
            ..TransientUiState::default()
        };
        Self {
            settings,
            config,
            store,
            undo_history: UndoHistory::with_capacity(UNDO_CAPACITY),
            providers,
            errors,
            llm_requests: LlmRequests::new(),
            editor_buffers,
            persistence_blocked,
            persistence_write_disabled: false,
            transient_ui,
            edit_session: None,
            navigation: NavigationStack::default(),
        }
    }

    /// Persist app config to `<config_dir>/app.toml`. Call when config changes (e.g. locale from settings).
    #[allow(dead_code)]
    pub fn save_app_config(&self) -> Result<(), crate::app::config::SaveError> {
        crate::app::config::save(&self.config)
    }

    /// Effective UI locale for this session (config → env → default, normalized).
    pub fn effective_locale(&self) -> String {
        i18n::resolved_locale_from_config(&self.config)
    }

    /// Borrow transient UI state.
    ///
    /// This is the canonical read API for ephemeral interaction state.
    /// Reads through this accessor are always side-effect free and never
    /// mutate persisted document state or undo history.
    pub(crate) fn ui(&self) -> &TransientUiState {
        &self.transient_ui
    }

    /// Mutably borrow transient UI state.
    ///
    /// This is the canonical write API for ephemeral interaction state.
    /// Callers should only mutate UI/session fields (focus, overlays, find,
    /// viewport/theme hints) and must not encode durable document semantics
    /// through this channel.
    pub(crate) fn ui_mut(&mut self) -> &mut TransientUiState {
        &mut self.transient_ui
    }

    /// Whether the current UI appearance mode is dark.
    pub fn is_dark_mode(&self) -> bool {
        self.ui().is_dark
    }

    /// Whether undo has at least one available snapshot.
    pub(crate) fn can_undo(&self) -> bool {
        self.undo_history.can_undo()
    }

    /// Whether redo has at least one available snapshot.
    pub(crate) fn can_redo(&self) -> bool {
        self.undo_history.can_redo()
    }

    fn startup_store_from_load_result(
        load_result: Result<BlockStore, StoreLoadError>,
    ) -> (BlockStore, bool, Vec<AppError>) {
        match load_result {
            | Ok(store) => (store, false, vec![]),
            | Err(err) => {
                tracing::error!(%err, "failed to load block store; entering recovery mode");
                let error = AppError::Persistence(UiError::from_message(format!(
                    "failed to load blocks.json: {err}; opened a temporary recovery workspace and disabled autosave for this session"
                )));
                (BlockStore::recovery_store(), true, vec![error])
            }
        }
    }

    /// Persist all graph state.
    ///
    /// Write order is main-file first, then mounted files (`save` then
    /// `save_mounts`). This prioritizes keeping the main graph shape current,
    /// while accepting temporary cross-file skew if a later mount write fails.
    fn save_tree(&mut self) -> std::io::Result<()> {
        if self.persistence_blocked {
            let err = std::io::Error::other("persistence disabled after initial load failure");
            self.record_error(AppError::Persistence(UiError::from_message(err.to_string())));
            return Err(err);
        }

        match self.store.save().and_then(|_| self.store.save_mounts()) {
            | Ok(()) => {
                self.errors.retain(|err| !matches!(err, AppError::Persistence(_)));
                Ok(())
            }
            | Err(err) => {
                self.record_error(AppError::Persistence(UiError::from_message(format!(
                    "failed to persist data: {err}"
                ))));
                Err(err)
            }
        }
    }

    fn persist_with_context(&mut self, context: &'static str) {
        if self.persistence_write_disabled {
            return;
        }
        if let Err(err) = self.save_tree() {
            tracing::error!(%err, context, "failed to save tree");
        }
    }

    fn llm_config_for_reduce(&mut self, block_id: BlockId) -> Option<llm::LlmConfig> {
        match self.providers.resolve_active() {
            | Ok(config) => Some(config),
            | Err(err) => {
                let ui_err = UiError::from_message(err);
                self.record_error(AppError::Configuration(ui_err.clone()));
                self.llm_requests.set_reduce_error(block_id, ui_err);
                None
            }
        }
    }

    fn llm_config_for_expand(&mut self, block_id: BlockId) -> Option<llm::LlmConfig> {
        match self.providers.resolve_active() {
            | Ok(config) => Some(config),
            | Err(err) => {
                let ui_err = UiError::from_message(err);
                self.record_error(AppError::Configuration(ui_err.clone()));
                self.llm_requests.set_expand_error(block_id, ui_err);
                None
            }
        }
    }

    fn llm_config_for_inquire(&mut self) -> Option<llm::LlmConfig> {
        match self.providers.resolve_active() {
            | Ok(config) => Some(config),
            | Err(err) => {
                self.record_error(AppError::Configuration(UiError::from_message(err)));
                None
            }
        }
    }

    fn record_error(&mut self, error: AppError) {
        tracing::error!(%error, "recording error");
        if self.errors.last() == Some(&error) {
            return;
        }
        self.errors.push(error);
    }

    fn resolve_llm_request<T, E>(
        result: Result<Result<T, E>, tokio::time::error::Elapsed>,
        timeout_message: impl Into<String>,
    ) -> Result<T, UiError>
    where
        E: ToString,
    {
        match result {
            | Ok(inner) => inner.map_err(UiError::from_message),
            | Err(_) => Err(UiError::from_message(timeout_message.into())),
        }
    }

    /// Get the current UI focus state.
    fn focus(&self) -> Option<FocusState> {
        self.ui().focus
    }

    /// Set the focused block.
    fn set_focus(&mut self, block_id: BlockId) {
        if let Some(state) = &mut self.ui_mut().focus {
            state.block_id = block_id;
        } else {
            self.ui_mut().focus = Some(FocusState { block_id, overflow_open: false });
        }
    }

    /// Clear the focus.
    fn clear_focus(&mut self) {
        self.ui_mut().focus = None;
    }

    /// Set the overflow menu open/closed for the focused block.
    fn set_overflow_open(&mut self, open: bool) {
        if let Some(state) = &mut self.ui_mut().focus {
            state.overflow_open = open;
        }
    }

    /// Close the focused block panel, if one is currently open.
    ///
    /// Returns `true` when Escape consumed this fallback by closing a panel.
    /// This is intentionally side-effecting because panel-open state is
    /// persisted per block.
    fn close_focused_block_panel(&mut self) -> bool {
        let Some(block_id) = self.focus().map(|state| state.block_id) else {
            return false;
        };
        let Some(panel_state) = self.store.block_panel_state(&block_id).copied() else {
            return false;
        };

        self.store.set_block_panel_state(&block_id, None);
        if panel_state == BlockPanelBarState::Friends {
            self.ui_mut().hovered_friend_block = None;
        }
        self.persist_with_context("after closing focused block panel");
        tracing::info!(block_id = ?block_id, panel = ?panel_state, "closed focused panel");
        true
    }

    /// Snapshot the current store into undo history before a mutation.
    fn snapshot_for_undo(&mut self) {
        self.undo_history
            .push(UndoSnapshot { store: self.store.clone(), navigation: self.navigation.clone() });
        self.edit_session = None;
    }

    fn mutate_with_undo_and_persist<F>(&mut self, context: &'static str, mutate: F)
    where
        F: FnOnce(&mut Self) -> bool,
    {
        self.snapshot_for_undo();
        if mutate(self) {
            self.persist_with_context(context);
        }
    }

    fn restore_snapshot(&mut self, snapshot: UndoSnapshot) {
        self.editor_buffers = EditorBuffers::from_store(&snapshot.store);
        self.store = snapshot.store;
        self.navigation = snapshot.navigation;
        self.llm_requests.clear();
        self.clear_focus();
        self.edit_session = None;

        self.persist_with_context("after undo/redo");
    }

    fn block_context_signature(&self, block_id: &BlockId) -> Option<RequestSignature> {
        let context = self.store.block_context_for_id(block_id);
        RequestSignature::from_block_context(&context)
    }

    fn is_stale_response(&self, block_id: &BlockId, request_signature: RequestSignature) -> bool {
        self.block_context_signature(block_id)
            .is_none_or(|current_signature| current_signature != request_signature)
    }
}

/// Elm-architecture messages driving all state transitions.
#[derive(Debug, Clone)]
pub enum Message {
    UndoRedo(UndoRedoMessage),
    Edit(EditMessage),
    Shortcut(ShortcutMessage),
    Error(ErrorMessage),
    Reduce(ReduceMessage),
    Expand(ExpandMessage),
    Structure(StructureMessage),
    Find(FindMessage),
    Overlay(OverlayMessage),
    MountFile(MountFileMessage),
    FriendPanel(FriendPanelMessage),
    InstructionPanel(BlockId, InstructionPanelMessage),
    Settings(SettingsMessage),
    WindowResized(WindowSize),
    KeyboardModifiersChanged(keyboard::Modifiers),
    DocumentMode(DocumentMode),
    SystemThemeChanged(iced::theme::Mode),
    Navigation(NavigationMessage),
}

impl AppState {
    /// Process one message and return a follow-up task (if any).
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let keep_inline_confirmation =
            matches!(&message, Message::MountFile(MountFileMessage::InlineMountAll(_)));
        if !keep_inline_confirmation {
            self.ui_mut().pending_inline_mount_confirmation = None;
        }

        let keep_mount_action_overflow =
            matches!(&message, Message::Overlay(OverlayMessage::ToggleMountActionsOverflow(_)));
        if !keep_mount_action_overflow {
            self.ui_mut().mount_action_overflow_block = None;
        }

        match message {
            | Message::UndoRedo(message) => undo_redo::handle(self, message),
            | Message::Shortcut(message) => shortcut::handle(self, message),
            | Message::Error(message) => error::handle(self, message),
            | Message::Edit(message) => edit::handle(self, message),
            | Message::Reduce(message) => reduce::handle(self, message),
            | Message::Expand(message) => expand::handle(self, message),
            | Message::Find(message) => find_panel::handle(self, message),
            | Message::Overlay(message) => overlay::handle(self, message),
            | Message::FriendPanel(message) => friends_panel::handle(self, message),
            | Message::Structure(message) => structure::handle(self, message),
            | Message::MountFile(message) => mount_file::handle(self, message),
            | Message::InstructionPanel(target, message) => {
                instruction_panel::handle(self, target, message)
            }
            | Message::Settings(message) => settings::handle(self, message),
            | Message::WindowResized(size) => {
                self.ui_mut().window_size = size;
                Task::none()
            }
            | Message::KeyboardModifiersChanged(modifiers) => {
                self.ui_mut().keyboard_modifiers = modifiers;
                Task::none()
            }
            | Message::DocumentMode(mode) => {
                // Clear friend hover state when changing document modes
                self.ui_mut().hovered_friend_block = None;
                self.ui_mut().document_mode = mode;
                Task::none()
            }
            | Message::SystemThemeChanged(mode) => {
                if self.config.dark_mode.is_some() {
                    tracing::debug!(
                        config_dark_mode = ?self.config.dark_mode,
                        "ignored system theme change due to persisted override"
                    );
                    return Task::none();
                }
                let dark = matches!(mode, iced::theme::Mode::Dark);
                if self.ui().is_dark != dark {
                    tracing::info!(is_dark = dark, "system theme changed");
                    self.ui_mut().is_dark = dark;
                }
                Task::none()
            }
            | Message::Navigation(message) => navigation::handle(self, message),
        }
    }
}

impl AppState {
    pub fn view(&self) -> Element<'_, Message> {
        i18n::set_app_locale(&self.effective_locale());
        match self.ui().active_view {
            | ViewMode::Document => document::DocumentView::new(self).view(),
            | ViewMode::Settings => settings::view(self),
        }
    }
}

impl AppState {
    /// Global event subscription: keyboard shortcuts, mouse clicks, escape,
    /// system theme changes, and window resize events.
    pub fn subscription(_state: &AppState) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(|event, status, _window| match event {
                | Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                    Some(Message::KeyboardModifiersChanged(modifiers))
                }
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::Find(FindMessage::Escape)),
                | Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    if let Some(shortcut) = shortcut::movement_shortcut_from_key(&key, modifiers) {
                        return Some(Message::Shortcut(shortcut));
                    }

                    if modifiers.command() {
                        match &key {
                            | keyboard::Key::Character(c) if c.eq_ignore_ascii_case("f") => {
                                return Some(Message::Find(FindMessage::Toggle));
                            }
                            | keyboard::Key::Character(c) if c.eq_ignore_ascii_case("g") => {
                                return if modifiers.shift() {
                                    Some(Message::Find(FindMessage::JumpPrevious))
                                } else {
                                    Some(Message::Find(FindMessage::JumpNext))
                                };
                            }
                            | keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") => {
                                return if modifiers.shift() {
                                    Some(Message::UndoRedo(UndoRedoMessage::Redo))
                                } else {
                                    Some(Message::UndoRedo(UndoRedoMessage::Undo))
                                };
                            }
                            | _ => {}
                        }
                    }

                    let action_shortcut = action_bar::shortcut_to_action(key, modifiers);

                    if status == event::Status::Captured {
                        // Text editors can capture command+punctuation while still emitting
                        // insert actions. Keep expand/reduce available through the global
                        // subscription path so `Cmd/Ctrl+.` and `Cmd/Ctrl+,` remain reliable.
                        return action_shortcut
                            .filter(|action_id| {
                                matches!(action_id, ActionId::Expand | ActionId::Reduce)
                            })
                            .map(ShortcutMessage::Trigger)
                            .map(Message::Shortcut);
                    }

                    action_shortcut.map(ShortcutMessage::Trigger).map(Message::Shortcut)
                }
                | Event::Window(window::Event::Resized(size)) => {
                    Some(Message::WindowResized(WindowSize {
                        width: size.width as f32,
                        height: size.height as f32,
                    }))
                }
                | _ => None,
            }),
            system::theme_changes().map(Message::SystemThemeChanged),
        ])
    }
}

/// Document interaction mode: normal editing vs picking a friend block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentMode {
    /// Normal block editing mode.
    #[default]
    Normal,
    /// Picking a friend block to add to the focused block.
    PickFriend,
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
/// - read through [`AppState::ui`]
/// - write through [`AppState::ui_mut`]
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
    /// Current document interaction mode (normal vs picking a friend).
    pub document_mode: DocumentMode,
    /// Which top-level screen is currently shown.
    pub active_view: ViewMode,
    /// Current window dimensions for responsive layout.
    pub window_size: WindowSize,
    /// Last observed keyboard modifier state from global events.
    ///
    /// This is used to filter command-shortcut key leaks (for example,
    /// suppressing `Cmd/Ctrl+F` text insertion into active editors/inputs).
    pub keyboard_modifiers: keyboard::Modifiers,
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
}

/// Snapshot of undoable application state.
///
/// Contains the store and navigation stack. Editor buffers are
/// rebuilt from the store on restore since `text_editor::Content` is
/// not cheaply cloneable with full cursor state.
///
/// # Design Decisions
///
/// ## Navigation Stack Inclusion
///
/// The navigation stack is part of the undo snapshot to maintain consistency
/// between document structure and view state. Without this, undoing a structural
/// change (e.g., deleting a block) could leave the user viewing a non-existent
/// block or an outdated view.
///
/// ## Editor Buffers Exclusion
///
/// Editor buffers (text editor content with cursor state) are intentionally
/// excluded from the snapshot. They are rebuilt from the store on restore
/// because:
/// - Full cursor state is expensive to clone
/// - Text content is derived from `BlockStore::points`
/// - Cursor position reset is acceptable UX for undo operations
#[derive(Clone)]
struct UndoSnapshot {
    store: BlockStore,
    navigation: NavigationStack,
}

mod undo_redo {
    use super::*;

    /// Messages for global undo/redo operations.
    #[derive(Debug, Clone)]
    pub enum UndoRedoMessage {
        Undo,
        Redo,
    }

    pub fn handle(state: &mut AppState, message: UndoRedoMessage) -> Task<Message> {
        match message {
            | UndoRedoMessage::Undo => {
                let current = UndoSnapshot {
                    store: state.store.clone(),
                    navigation: state.navigation.clone(),
                };
                if let Some(previous) = state.undo_history.undo(current) {
                    tracing::info!("undo applied");
                    state.restore_snapshot(previous);
                }
                Task::none()
            }
            | UndoRedoMessage::Redo => {
                let current = UndoSnapshot {
                    store: state.store.clone(),
                    navigation: state.navigation.clone(),
                };
                if let Some(next) = state.undo_history.redo(current) {
                    tracing::info!("redo applied");
                    state.restore_snapshot(next);
                }
                Task::none()
            }
        }
    }
}

mod edit {
    use super::*;

    /// Messages for point text editing.
    #[derive(Debug, Clone)]
    pub enum EditMessage {
        PointEdited {
            block_id: BlockId,
            action: text_editor::Action,
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
        fn enter_at_end_of_only_line_inserts_empty_first_child_when_enabled() {
            let (mut state, root) = AppState::test_state();
            state.store.update_point(&root, "hello".to_string());
            let existing = state
                .store
                .append_child(&root, "existing".to_string())
                .expect("append child succeeds");
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
            let existing = state
                .store
                .append_child(&root, "existing".to_string())
                .expect("append child succeeds");
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
    }
}

mod shortcut {
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
        let reduction_draft = state.store.reduction_draft(&block_id);
        let row_context = RowContext {
            block_id,
            point_text,
            has_draft: expansion_draft.is_some() || reduction_draft.is_some(),
            draft_suggestion_count: expansion_draft.map(|d| d.children.len()).unwrap_or(0)
                + reduction_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
            has_expand_error: state.llm_requests.has_expand_error(block_id),
            has_reduce_error: state.llm_requests.has_reduce_error(block_id),
            is_expanding: state.llm_requests.is_expanding(block_id),
            is_reducing: state.llm_requests.is_reducing(block_id),
            is_mounted: state.store.mount_table().entry(block_id).is_some(),
            has_children: !state.store.children(&block_id).is_empty(),
            is_unexpanded_mount: state
                .store
                .node(&block_id)
                .is_some_and(|n| n.mount_path().is_some()),
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
            assert!(matches!(
                down,
                Some(ShortcutMessage::Movement(MovementShortcut::MoveSiblingNext))
            ));
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

            let _ = handle(
                &mut state,
                ShortcutMessage::Movement(MovementShortcut::FocusSiblingPrevious),
            );

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

            let _ = handle(
                &mut state,
                ShortcutMessage::Movement(MovementShortcut::MoveSiblingPrevious),
            );

            assert_eq!(state.store.roots(), &[sibling, root]);
            assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
        }

        #[test]
        fn move_after_parent_outdents_block() {
            let (mut state, root) = AppState::test_state();
            let child = state
                .store
                .append_child(&root, "child".to_string())
                .expect("append child succeeds");
            state.set_focus(child);

            let _ =
                handle(&mut state, ShortcutMessage::Movement(MovementShortcut::MoveAfterParent));

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
            let child = state
                .store
                .append_child(&root, "child".to_string())
                .expect("append child succeeds");
            state.store.toggle_collapsed(&root);
            state.set_focus(root);

            let _ =
                handle(&mut state, ShortcutMessage::Movement(MovementShortcut::FocusFirstChild));

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
}

#[cfg(test)]
impl AppState {
    pub fn test_state() -> (Self, crate::store::BlockId) {
        use crate::llm;
        use crate::store::BlockStore;
        use crate::undo::UndoHistory;

        let store = BlockStore::default();
        let root = *store.roots().first().expect("default store has a root");
        let providers = llm::LlmProviders::test_valid();
        let config = AppConfig::default();
        let settings = SettingsState::from_providers(&providers, &config);
        let editor_buffers = EditorBuffers::from_store(&store);
        let state = Self {
            settings,
            config,
            store,
            undo_history: UndoHistory::with_capacity(64),
            providers,
            errors: vec![],
            llm_requests: LlmRequests::new(),
            editor_buffers,
            persistence_blocked: false,
            persistence_write_disabled: true,
            transient_ui: TransientUiState::default(),
            edit_session: None,
            navigation: NavigationStack::default(),
        };
        (state, root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreLoadError;

    fn test_state() -> (AppState, crate::store::BlockId) {
        AppState::test_state()
    }

    #[test]
    fn response_is_stale_after_point_change() {
        let (mut state, root) = test_state();
        let request_signature = state.block_context_signature(&root).expect("root has lineage");
        state.store.update_point(&root, "changed".to_string());
        assert!(state.is_stale_response(&root, request_signature));
    }

    #[test]
    fn response_is_not_stale_without_point_change() {
        let (state, root) = test_state();
        let request_signature = state.block_context_signature(&root).expect("root has lineage");
        assert!(!state.is_stale_response(&root, request_signature));
    }

    #[test]
    fn request_signature_changes_when_lineage_changes() {
        let (mut state, root) = test_state();
        let child =
            state.store.append_child(&root, "child".to_string()).expect("append child succeeds");
        let before = state.block_context_signature(&child).expect("child has lineage");
        state.store.update_point(&root, "root changed".to_string());
        let after = state.block_context_signature(&child).expect("child has lineage");
        assert_ne!(before, after);
    }

    #[test]
    fn load_failure_enters_recovery_mode_with_blank_workspace() {
        let (store, persistence_blocked, errors) =
            AppState::startup_store_from_load_result(Err(StoreLoadError::PathUnavailable));

        assert!(persistence_blocked);
        assert!(errors.iter().any(|err| matches!(err, AppError::Persistence(_))));
        let root = *store.roots().first().expect("recovery store has one root");
        assert_eq!(store.point(&root).as_deref(), Some(""));
        assert_ne!(store.point(&root).as_deref(), Some("Notes on liberating productivity"));
    }

    #[test]
    fn test_build_persistence_is_side_effect_free() {
        let (mut state, root) = test_state();
        state.store.update_point(&root, "edited".to_string());

        state.persist_with_context("test-only persistence noop");

        assert!(state.errors.is_empty());
    }

    #[test]
    fn system_theme_changes_apply_without_persisted_override() {
        let (mut state, _root) = test_state();
        state.config.dark_mode = None;
        state.ui_mut().is_dark = false;

        let _ = state.update(Message::SystemThemeChanged(iced::theme::Mode::Dark));

        assert!(state.ui().is_dark);
    }

    #[test]
    fn system_theme_changes_are_ignored_with_persisted_override() {
        let (mut state, _root) = test_state();
        state.config.dark_mode = Some(true);
        state.ui_mut().is_dark = true;

        let _ = state.update(Message::SystemThemeChanged(iced::theme::Mode::Light));

        assert!(state.ui().is_dark);
    }
}
