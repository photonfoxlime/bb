//! Application orchestration layer for the Iced UI.
//!
//! Domain semantics are documented next to the owning handlers and state types.

mod action_bar;
mod config;
mod diff;
mod document;
mod editor_buffers;
mod error;
mod friends_panel;
mod instruction_panel;
mod llm_requests;
mod settings;
mod reduce;
mod expand;
mod structure;
mod overlay;
mod mount_file;
mod error_banner;

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
    friends_panel::FriendPanelMessage,
    instruction_panel::InstructionPanelMessage,
    llm_requests::{LlmRequests, RequestSignature},
    mount_file::MountFileMessage,
    overlay::OverlayMessage,
    reduce::ReduceMessage,
    settings::{SettingsMessage, SettingsState},
    shortcut::ShortcutMessage,
    structure::StructureMessage,
    undo_redo::UndoRedoMessage,
};
use crate::{
    i18n, llm,
    store::{BlockId, BlockStore, PanelBarState, StoreLoadError},
    undo::UndoHistory,
};
use iced::{
    Element, Event, Subscription, Task, event, keyboard, system,
    widget::{self, text_editor},
};
use std::time::Duration;

pub use config::AppConfig;

/// Document interaction mode: normal editing vs picking a friend block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentMode {
    /// Normal block editing mode.
    #[default]
    Normal,
    /// Picking a friend block to add to the focused block.
    PickFriend,
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
/// - selectors (`focused_block_id`, `editing_block_id`) and
///   overlay flags: view/controller state only.
#[derive(Clone)]
pub struct AppState {
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
    /// Block currently holding open the overflow menu.
    overflow_open_for: Option<BlockId>,
    /// (target_block_id, friend_block_id) of friend perspective currently being edited inline.
    editing_friend_perspective: Option<(BlockId, BlockId)>,
    /// Current text input value when editing friend perspective.
    editing_friend_perspective_input: Option<String>,
    /// Block whose point editor currently has keyboard focus.
    ///
    /// Panel bar state is derived from this via [`BlockStore::panel_state`].
    focused_block_id: Option<BlockId>,
    /// Block currently coalescing point edits into a single undo entry.
    editing_block_id: Option<BlockId>,
    /// Current document interaction mode (normal vs picking a friend).
    document_mode: DocumentMode,
    /// Whether the current theme is dark. Detected from the system at startup
    /// and updated live via `iced::system::theme_changes()`.
    pub is_dark: bool,
    /// Which top-level screen is currently shown.
    pub active_view: ViewMode,
    /// Draft form state for the settings screen.
    pub settings: SettingsState,
    /// Persisted app preferences (e.g. optional locale). Loaded at startup from
    /// `<config_dir>/app.toml`; effective locale is derived via [`i18n::resolved_locale_from_config`].
    pub config: AppConfig,
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

        let is_dark = matches!(dark_light::detect(), Ok(dark_light::Mode::Dark));
        tracing::info!(is_dark, "detected system appearance");
        let config = crate::app::config::load();
        let settings = SettingsState::from_providers(&providers, &config);
        Self {
            store,
            undo_history: UndoHistory::with_capacity(UNDO_CAPACITY),
            providers,
            errors,
            llm_requests: LlmRequests::new(),
            editor_buffers,
            persistence_blocked,
            persistence_write_disabled: false,
            overflow_open_for: None,
            editing_friend_perspective: None,
            editing_friend_perspective_input: None,
            focused_block_id: None,
            editing_block_id: None,
            document_mode: DocumentMode::default(),

            is_dark,
            active_view: ViewMode::default(),
            settings,
            config,
        }
    }

    /// Persist app config to `<config_dir>/app.toml`. Call when config changes (e.g. locale from settings).
    pub fn save_app_config(&self) -> Result<(), crate::app::config::SaveError> {
        crate::app::config::save(&self.config)
    }

    /// Effective UI locale for this session (config → env → default, normalized).
    pub fn effective_locale(&self) -> String {
        i18n::resolved_locale_from_config(&self.config)
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

    fn record_error(&mut self, error: AppError) {
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

    /// Resolve shortcut target priority: focused editor, then first root.
    fn current_block_for_shortcuts(&self) -> Option<BlockId> {
        self.focused_block_id.or_else(|| self.store.roots().first().copied())
    }

    /// Snapshot the current store into undo history before a mutation.
    fn snapshot_for_undo(&mut self) {
        self.undo_history.push(UndoSnapshot { store: self.store.clone() });
        self.editing_block_id = None;
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
        self.llm_requests.clear();
        self.focused_block_id = None;
        self.editing_block_id = None;

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
    Overlay(OverlayMessage),
    MountFile(MountFileMessage),
    FriendPanel(FriendPanelMessage),
    InstructionPanel(BlockId, InstructionPanelMessage),
    Settings(SettingsMessage),
}

impl AppState {
    /// Process one message and return a follow-up task (if any).
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // When the settings view is active, Escape (arriving as CancelFriendPicker
        // from the global event handler) should close settings instead.
        if self.active_view == ViewMode::Settings {
            if matches!(&message, Message::FriendPanel(FriendPanelMessage::CancelFriendPicker)) {
                return settings::handle(self, SettingsMessage::Close);
            }
        }

        // When editing friend perspective, Escape (arriving as CancelEditingFriendPerspective)
        // should just clear the editing state (handled in friends panel handler).

        match message {
            | Message::UndoRedo(message) => undo_redo::handle(self, message),
            | Message::Shortcut(message) => shortcut::handle(self, message),
            | Message::Error(message) => error::handle(self, message),
            | Message::Edit(EditMessage::PointEdited { block_id, action }) => {
                edit::handle_point_edited(self, block_id, action)
            }
            | Message::Reduce(message) => reduce::handle(self, message),
            | Message::Expand(message) => expand::handle(self, message),
            | Message::Overlay(message) => overlay::handle(self, message),
            | Message::FriendPanel(message) => friends_panel::handle(self, message),
            | Message::Structure(message) => structure::handle(self, message),
            | Message::MountFile(message) => mount_file::handle(self, message),
            | Message::InstructionPanel(target, message) => {
                instruction_panel::handle(self, target, message)
            }
            | Message::Settings(message) => settings::handle(self, message),
        }
    }
}

impl AppState {
    pub fn view(&self) -> Element<'_, Message> {
        i18n::set_app_locale(&self.effective_locale());
        match self.active_view {
            | ViewMode::Document => document::DocumentView::new(self).view(),
            | ViewMode::Settings => settings::view(self),
        }
    }
}

impl AppState {
    /// Global event subscription: keyboard shortcuts, mouse clicks, escape,
    /// and system theme changes.
    pub fn subscription(_state: &AppState) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(|event, status, _window| {
                if status == event::Status::Captured {
                    return None;
                }

                match event {
                    | Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) => {
                        // Cancel friend perspective editing - uses state internally
                        Some(Message::FriendPanel(
                            FriendPanelMessage::CancelEditingFriendPerspective,
                        ))
                    }
                    | Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                        if modifiers.command() {
                            match &key {
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
                        action_bar::shortcut_to_action(key, modifiers)
                            .map(ShortcutMessage::Trigger)
                            .map(Message::Shortcut)
                    }
                    | _ => None,
                }
            }),
            system::theme_changes()
                .map(|mode| Message::MountFile(MountFileMessage::SystemThemeChanged(mode))),
        ])
    }
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

/// Snapshot of undoable application state.
///
/// Contains only the store. Editor buffers are
/// rebuilt from the store on restore since `text_editor::Content` is
/// not cheaply cloneable with full cursor state.
#[derive(Clone)]
struct UndoSnapshot {
    store: BlockStore,
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
                let current = UndoSnapshot { store: state.store.clone() };
                if let Some(previous) = state.undo_history.undo(current) {
                    tracing::info!("undo applied");
                    state.restore_snapshot(previous);
                }
                Task::none()
            }
            | UndoRedoMessage::Redo => {
                let current = UndoSnapshot { store: state.store.clone() };
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
        PointEdited { block_id: BlockId, action: text_editor::Action },
    }

    /// Direction tag for vertical cursor movement edge-detection.
    ///
    /// Used to defer block traversal until *after* the editor processes
    /// the motion, so wrapped (visual) lines are handled correctly.
    enum VerticalDir {
        Up,
        Down,
    }

    pub fn handle_point_edited(
        state: &mut AppState, block_id: BlockId, action: text_editor::Action,
    ) -> Task<Message> {
        // Don't change focus in PickFriend mode
        if state.document_mode == DocumentMode::PickFriend {
            return Task::none();
        }
        state.focused_block_id = Some(block_id);
        if state.editing_block_id.as_ref() != Some(&block_id) {
            state.snapshot_for_undo();
            state.editing_block_id = Some(block_id);
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
            if state.document_mode == DocumentMode::Normal {
                state.focused_block_id = Some(target_id);
                let wid_clone = wid.clone();
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
}

mod shortcut {
    use super::*;

    /// Messages for keyboard shortcut dispatch.
    #[derive(Debug, Clone)]
    pub enum ShortcutMessage {
        Trigger(ActionId),
        ForBlock { block_id: BlockId, action_id: ActionId },
    }

    pub fn handle(state: &mut AppState, message: ShortcutMessage) -> Task<Message> {
        match message {
            | ShortcutMessage::Trigger(action_id) => {
                let Some(block_id) = state.current_block_for_shortcuts() else {
                    return Task::none();
                };
                run_shortcut_for_block(state, block_id, action_id)
            }
            | ShortcutMessage::ForBlock { block_id, action_id } => {
                // Don't change focus in PickFriend mode
                if state.document_mode != DocumentMode::PickFriend {
                    state.focused_block_id = Some(block_id);
                }
                run_shortcut_for_block(state, block_id, action_id)
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
        let state = Self {
            editor_buffers: EditorBuffers::from_store(&store),
            store,
            undo_history: UndoHistory::with_capacity(64),
            settings: SettingsState::from_providers(&providers, &config),
            providers,
            errors: vec![],
            llm_requests: LlmRequests::new(),
            overflow_open_for: None,
            editing_friend_perspective: None,
            editing_friend_perspective_input: None,
            focused_block_id: None,
            editing_block_id: None,
            document_mode: DocumentMode::default(),
            persistence_blocked: false,
            persistence_write_disabled: true,
            is_dark: false,
            active_view: ViewMode::default(),
            config,
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
}
