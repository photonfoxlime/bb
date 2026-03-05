//! Application orchestration layer for the Iced UI.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Domain semantics are documented next to the owning handlers and state types.
//!
//! # Screen and workflow inventory
//!
//! The iced runtime is organized around two top-level screens:
//!
//! - `Document` renders the tree editor, action bar, breadcrumbs, mount
//!   controls, friend/instruction panels, and floating find/link/help overlays.
//! - `Settings` renders provider management, per-task LLM settings, locale and
//!   appearance preferences, and resolved data/config paths.
//!
//! Core document workflows supported by this layer:
//!
//! - direct block editing with undo/redo and structural shortcuts,
//! - LLM-assisted amplify, distill, atomize, and probe requests,
//! - friend-block context curation and instruction drafting,
//! - mount expansion/collapse/save/load/move/inline for external subtree files,
//! - drill-down navigation, multiselect deletion, point-to-link conversion, and
//!   global phrase-aware find.
//!
//! # Architecture summary
//!
//! This module is the Elm boundary for the GUI:
//!
//! - [`AppState`] owns durable document/config state plus transient session UI.
//! - [`Message`] is the single top-level event enum routed into focused handler
//!   modules such as `edit`, `patch`, `settings`, `find_panel`, and
//!   `mount_file`.
//! - [`DocumentView`](document::DocumentView) is a pure renderer over borrowed
//!   state, while `update` centralizes mutation and `subscription` wires global
//!   keyboard/window/theme events.
//!
//! Persistence is intentionally split. [`BlockStore`] holds user-authored data
//! and persisted per-block UI hints, while [`TransientUiState`] carries
//! disposable interaction state such as focus, overlays, multiselect, and link
//! panel search results. Undo snapshots include navigation but rebuild editor
//! buffers on restore so the app preserves semantic state without serializing
//! widget internals.

// Global
mod config;
mod shortcut;
mod error;
// Main Views
mod context_menu;
mod document;
mod document_toolbar;
mod document_top_right;
mod mount_indicator;
mod point_editor;
mod settings;
mod find_panel;
mod link_panel;
mod error_banner;
// Block Editor
mod edit;
mod editor_buffers;
pub(crate) mod diff;
mod overlay;
// Panel Views
mod archive_panel;
mod friends_panel;
mod instruction_panel;
// Actions and LLM Requests
mod action_bar;
mod llm_requests;
mod patch;
// Structural Operations
mod structure;
mod multiselect;
mod navigation;
mod mount_file;
mod patch_panel;
mod point_text_editor;
mod shortcut_help_banner;
mod state;
mod undo_redo;

use self::{
    action_bar::{
        ActionAvailability, ActionId, RowContext, ViewportBucket, action_to_message_by_id,
        build_action_bar_vm, project_for_viewport,
    },
    archive_panel::ArchivePanelMessage,
    edit::EditMessage,
    editor_buffers::EditorBuffers,
    error::{AppError, ErrorMessage, UiError},
    error_banner::ErrorBanner,
    find_panel::FindMessage,
    friends_panel::FriendPanelMessage,
    instruction_panel::InstructionPanelMessage,
    llm_requests::{LlmRequests, RequestSignature},
    mount_file::MountFileMessage,
    navigation::{NavigationMessage, NavigationStack},
    overlay::OverlayMessage,
    patch::PatchMessage,
    settings::{SettingsMessage, SettingsState},
    shortcut::ShortcutMessage,
    state::{
        DocumentMode, FocusState, LinkModeMessage, LinkPanelState, TransientUiState, ViewMode,
        WindowSize,
    },
    structure::StructureMessage,
    undo_redo::{UndoRedoMessage, UndoSnapshot},
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
use std::collections::BTreeSet;
use std::time::Duration;

pub use config::AppConfig;

/// Context menu actions for text editors.
#[derive(Debug, Clone)]
pub enum ContextMenuMessage {
    /// Show context menu at the given position for the specified block.
    Show { block_id: BlockId, position: iced::Point },
    /// Hide the context menu.
    Hide,
    /// Execute a context menu action.
    Action(ContextMenuAction),
}

/// Actions available in the text editor context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// Convert a text point to a link (href = current text, kind inferred).
    ConvertToLink,
    /// Convert a link point back to plain text (display text becomes content).
    ConvertToText,
}

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

/// Elm-architecture messages driving all state transitions.
#[derive(Debug, Clone)]
pub enum Message {
    UndoRedo(UndoRedoMessage),
    Edit(EditMessage),
    Shortcut(ShortcutMessage),
    Error(ErrorMessage),
    Patch(PatchMessage),
    Structure(StructureMessage),
    Find(FindMessage),
    Archive(ArchivePanelMessage),
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
    ContextMenu(ContextMenuMessage),
    LinkMode(LinkModeMessage),
    /// Toggle inline preview for a link chip (expand / collapse).
    LinkChipToggle(BlockId),
    /// Block clicked in multiselect mode. Modifiers at click time drive behavior.
    MultiselectBlockClicked(BlockId),
    /// Plain Backspace in multiselect mode: delete selection. Handled globally
    /// because blocks render as plain text (no editor to receive the key).
    MultiselectBackspace,
    CursorPosition(iced::Point),
    EscapePressed,
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
            | Message::Patch(message) => patch::handle(self, message),
            | Message::Find(message) => find_panel::handle(self, message),
            | Message::Archive(message) => archive_panel::handle(self, message),
            | Message::Overlay(message) => overlay::handle(self, message),
            | Message::FriendPanel(message) => friends_panel::handle(self, message),
            | Message::Structure(message) => structure::handle(self, message),
            | Message::MountFile(message) => mount_file::handle(self, message),
            | Message::InstructionPanel(target, message) => {
                instruction_panel::handle(self, target, message)
            }
            | Message::Settings(message) => settings::handle(self, message),
            | Message::ContextMenu(message) => context_menu::handle(self, message),
            | Message::LinkMode(message) => link_panel::handle(self, message),
            | Message::LinkChipToggle(block_id) => {
                if !self.ui_mut().expanded_links.remove(&block_id) {
                    self.ui_mut().expanded_links.insert(block_id);
                }
                Task::none()
            }
            | Message::EscapePressed => {
                // Highest priority: close context menu if open
                if self.ui().context_menu.is_some() {
                    self.ui_mut().context_menu = None;
                    Task::none()
                } else if matches!(self.ui().document_mode, DocumentMode::LinkInput) {
                    link_panel::handle(self, LinkModeMessage::Cancel)
                } else if matches!(self.ui().document_mode, DocumentMode::Archive) {
                    archive_panel::handle(self, ArchivePanelMessage::Close)
                } else {
                    find_panel::handle(self, FindMessage::Escape)
                }
            }
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

                match mode {
                    | DocumentMode::Multiselect => {
                        let selected = self
                            .focus()
                            .map(|focus| focus.block_id)
                            .filter(|block_id| self.store.node(block_id).is_some());

                        self.ui_mut().document_mode = DocumentMode::Multiselect;
                        self.ui_mut().multiselect_selected_blocks.clear();
                        self.ui_mut().multiselect_anchor = None;
                        if let Some(block_id) = selected {
                            self.ui_mut().multiselect_selected_blocks.insert(block_id);
                            self.ui_mut().multiselect_anchor = Some(block_id);
                        }
                    }
                    | _ => {
                        self.ui_mut().document_mode = mode;
                        self.ui_mut().multiselect_selected_blocks.clear();
                        self.ui_mut().multiselect_anchor = None;
                    }
                }
                Task::none()
            }
            | Message::MultiselectBlockClicked(block_id) => {
                if self.ui().document_mode != DocumentMode::Multiselect {
                    return Task::none();
                }
                if self.store.node(&block_id).is_none() {
                    return Task::none();
                }
                multiselect::handle_block_clicked(self, block_id);
                Task::none()
            }
            | Message::MultiselectBackspace => {
                if self.ui().document_mode != DocumentMode::Multiselect {
                    return Task::none();
                }
                edit::handle_multiselect_backspace(self)
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
            | Message::CursorPosition(position) => {
                self.ui_mut().cursor_position = Some(position);
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
    /// system theme changes, window resize events, and cursor tracking.
    pub fn subscription(_state: &AppState) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(|event, status, _window| match event {
                | Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorPosition(position))
                }
                | Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                    Some(Message::KeyboardModifiersChanged(modifiers))
                }
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::EscapePressed),
                // Arrow keys for link panel candidate navigation.
                // Emitted unconditionally; `update()` ignores them when not
                // in `DocumentMode::LinkInput`.
                // Note: Exclude command/control/alt so movement shortcuts
                // (Ctrl+arrows on macOS, Alt+arrows elsewhere) are not consumed.
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::ArrowUp),
                    modifiers,
                    ..
                }) if status != event::Status::Captured
                    && !modifiers.command()
                    && !modifiers.control()
                    && !modifiers.alt() =>
                {
                    Some(Message::LinkMode(LinkModeMessage::SelectPrevious))
                }
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::ArrowDown),
                    modifiers,
                    ..
                }) if status != event::Status::Captured
                    && !modifiers.command()
                    && !modifiers.control()
                    && !modifiers.alt() =>
                {
                    Some(Message::LinkMode(LinkModeMessage::SelectNext))
                }
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Backspace),
                    modifiers,
                    ..
                }) if status != event::Status::Captured
                    && !modifiers.shift()
                    && !modifiers.alt()
                    && !modifiers.command()
                    && !modifiers.control() =>
                {
                    // Emit unconditionally; update() no-ops when not in Multiselect.
                    // Needed because multiselect blocks render as plain text (no editor).
                    Some(Message::MultiselectBackspace)
                }
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

                    let action_shortcut = action_bar::shortcut_to_action(key, modifiers)
                        .filter(|action_id| AppState::allow_global_action_shortcut(*action_id));

                    if status == event::Status::Captured {
                        // Text editors can capture command+punctuation while still emitting
                        // insert actions. Keep expand/reduce available through the global
                        // subscription path so `Cmd/Ctrl+.` and `Cmd/Ctrl+,` remain reliable.
                        return action_shortcut
                            .filter(|action_id| {
                                matches!(action_id, ActionId::Amplify | ActionId::Distill)
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

    /// Whether a shortcut should be handled by the global key subscription.
    ///
    /// Design decision: Enter-based structural shortcuts (`Cmd/Ctrl+Enter` and
    /// `Cmd/Ctrl+Shift+Enter`) are handled in editor key binding so they are
    /// dispatched exactly once with the focused block id from that editor.
    /// The global subscription intentionally ignores these actions to avoid
    /// duplicate block creation from overlapping global/editor key paths.
    fn allow_global_action_shortcut(action_id: ActionId) -> bool {
        !matches!(action_id, ActionId::AddChild | ActionId::AddSibling)
    }
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
        let config = crate::app::config::load();
        let mut errors = vec![];
        // Validate each task's configured provider at startup.
        for task_cfg in [
            &config.tasks.amplify,
            &config.tasks.distill,
            &config.tasks.atomize,
            &config.tasks.probe,
        ] {
            if let Err(err) = providers.resolve(&task_cfg.provider, &task_cfg.model) {
                errors.push(AppError::Configuration(UiError::from_message(err)));
            }
        }
        let (store, persistence_blocked, persistence_errors) =
            Self::startup_store_from_load_result(BlockStore::load());
        errors.extend(persistence_errors);
        let editor_buffers = EditorBuffers::from_store(&store);
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

    fn llm_config_for_distill(&mut self, block_id: BlockId) -> Option<llm::LlmConfig> {
        let task = &self.config.tasks.distill;
        match self.providers.resolve(&task.provider, &task.model) {
            | Ok(config) => Some(config),
            | Err(err) => {
                let ui_err = UiError::from_message(err);
                self.record_error(AppError::Configuration(ui_err.clone()));
                self.llm_requests.set_distill_error(block_id, ui_err);
                None
            }
        }
    }

    fn llm_config_for_atomize(&mut self, block_id: BlockId) -> Option<llm::LlmConfig> {
        let task = &self.config.tasks.atomize;
        match self.providers.resolve(&task.provider, &task.model) {
            | Ok(config) => Some(config),
            | Err(err) => {
                let ui_err = UiError::from_message(err);
                self.record_error(AppError::Configuration(ui_err.clone()));
                self.llm_requests.set_atomize_error(block_id, ui_err);
                None
            }
        }
    }

    fn llm_config_for_amplify(&mut self, block_id: BlockId) -> Option<llm::LlmConfig> {
        let task = &self.config.tasks.amplify;
        match self.providers.resolve(&task.provider, &task.model) {
            | Ok(config) => Some(config),
            | Err(err) => {
                let ui_err = UiError::from_message(err);
                self.record_error(AppError::Configuration(ui_err.clone()));
                self.llm_requests.set_amplify_error(block_id, ui_err);
                None
            }
        }
    }

    fn llm_config_for_probe(&mut self) -> Option<llm::LlmConfig> {
        let task = &self.config.tasks.probe;
        match self.providers.resolve(&task.provider, &task.model) {
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

        if self.ui().document_mode == DocumentMode::Multiselect {
            self.ui_mut().multiselect_selected_blocks.clear();
            self.ui_mut().multiselect_selected_blocks.insert(block_id);
        }
    }

    /// Clear the focus.
    fn clear_focus(&mut self) {
        self.ui_mut().focus = None;
        if self.ui().document_mode == DocumentMode::Multiselect {
            self.ui_mut().multiselect_selected_blocks.clear();
        }
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
    fn global_shortcut_filter_ignores_enter_structural_actions() {
        assert!(!AppState::allow_global_action_shortcut(ActionId::AddChild));
        assert!(!AppState::allow_global_action_shortcut(ActionId::AddSibling));
        assert!(AppState::allow_global_action_shortcut(ActionId::Amplify));
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
