//! Application orchestration layer for the Iced UI.
//!
//! Top-level routing is `update -> AppState::dispatch_message`. Domain semantics
//! are documented next to the owning handlers and state types.

mod action_bar;
mod diff;
mod error;
mod editor_buffers;
mod llm_requests;
mod view;

use crate::llm;
use crate::paths::AppPaths;
use crate::store::{
    BlockId, BlockStore, ExpansionDraftRecord, FriendBlock, MountFormat, ReductionDraftRecord,
    StoreLoadError,
};
use crate::theme;
use crate::undo::UndoHistory;
use action_bar::{
    ActionAvailability, ActionId, RowContext, ViewportBucket, action_to_message_by_id,
    build_action_bar_vm, project_for_viewport, shortcut_to_action,
};
use editor_buffers::EditorBuffers;
use error::{AppError, UiError};
use iced::theme::Mode;
use iced::widget::{button, column, container, row, scrollable, text, text_editor};
use iced::{Element, Event, Fill, Subscription, Task, event, keyboard, mouse, system, widget};
use llm_requests::{LlmRequests, RequestSignature};

use std::time::Duration;

/// Snapshot of undoable application state.
///
/// Contains only the store. Editor buffers are
/// rebuilt from the store on restore since `text_editor::Content` is
/// not cheaply cloneable with full cursor state.
#[derive(Clone)]
struct UndoSnapshot {
    store: BlockStore,
}

/// Default capacity: 64 undo steps.
const UNDO_CAPACITY: usize = 64;
const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const ERROR_STACK_PREVIEW_LIMIT: usize = 2;

/// All mutable application state for the iced Elm architecture.
///
/// Owns the document store, editor buffers, undo history, LLM config,
/// async operation states, and transient UI state (overflow, active/focused/editing block ids).
///
/// Ownership split:
/// - `store`: authoritative graph, persisted drafts, mount runtime metadata.
/// - `editor_buffers`: widget-local text buffers + focus ids.
/// - persistence flags: recovery guard for unsafe-on-disk state plus a runtime
///   write gate for side-effect-free runs.
/// - selectors (`active_block_id`, `focused_block_id`, `editing_block_id`) and
///   overlay flags: view/controller state only.
#[derive(Clone)]
pub struct AppState {
    store: BlockStore,
    undo_history: UndoHistory<UndoSnapshot>,
    llm_config: Result<llm::LlmConfig, llm::LlmConfigError>,
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
    overflow_open_for: Option<BlockId>,
    /// Last block interacted with by actions or edits.
    active_block_id: Option<BlockId>,
    /// Block whose point editor currently has keyboard focus.
    focused_block_id: Option<BlockId>,
    /// Block currently coalescing point edits into a single undo entry.
    editing_block_id: Option<BlockId>,
    /// Whether the current theme is dark. Detected from the system at startup
    /// and updated live via `iced::system::theme_changes()`.
    pub is_dark: bool,
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
        let llm_config = llm::LlmConfig::load();
        let mut errors = vec![];
        if let Some(err) = llm_config.as_ref().err() {
            errors.push(AppError::Configuration(UiError::from_message(err)));
        }
        let (store, persistence_blocked, persistence_errors) =
            Self::startup_store_from_load_result(BlockStore::load());
        errors.extend(persistence_errors);
        let editor_buffers = EditorBuffers::from_store(&store);

        let is_dark = matches!(dark_light::detect(), Ok(dark_light::Mode::Dark));
        tracing::info!(is_dark, "detected system appearance");
        Self {
            store,
            undo_history: UndoHistory::with_capacity(UNDO_CAPACITY),
            llm_config,
            errors,
            llm_requests: LlmRequests::new(),
            editor_buffers,
            persistence_blocked,
            persistence_write_disabled: false,
            overflow_open_for: None,
            active_block_id: None,
            focused_block_id: None,
            editing_block_id: None,

            is_dark,
        }
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
        match &self.llm_config {
            | Ok(config) => Some(config.clone()),
            | Err(err) => {
                let ui_err = UiError::from_message(err);
                self.record_error(AppError::Configuration(ui_err.clone()));
                self.llm_requests.set_reduce_error(block_id, ui_err);
                None
            }
        }
    }

    fn llm_config_for_expand(&mut self, block_id: BlockId) -> Option<llm::LlmConfig> {
        match &self.llm_config {
            | Ok(config) => Some(config.clone()),
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
        result: Result<Result<T, E>, tokio::time::error::Elapsed>, timeout_message: &'static str,
    ) -> Result<T, UiError>
    where
        E: ToString,
    {
        match result {
            | Ok(inner) => inner.map_err(UiError::from_message),
            | Err(_) => Err(UiError::from_message(timeout_message)),
        }
    }

    /// Resolve shortcut target priority: focused editor, then active block, then first root.
    fn current_block_for_shortcuts(&self) -> Option<BlockId> {
        self.focused_block_id
            .or(self.active_block_id)
            .or_else(|| self.store.roots().first().copied())
    }

    fn set_active_block(&mut self, block_id: &BlockId) {
        self.active_block_id = Some(*block_id);
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
        self.active_block_id = self.store.roots().first().copied();

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

    fn dispatch_message(&mut self, message: Message) -> Task<Message> {
        match message {
            | Message::UndoRedo(message) => self.handle_undo_redo(message),
            | Message::Shortcut(message) => self.handle_shortcut_message(message),
            | Message::Error(message) => self.handle_error_message(message),
            | Message::Edit(EditMessage::PointEdited { block_id, action }) => {
                self.handle_point_edited(block_id, action)
            }
            | Message::Reduce(message) => self.handle_reduce_message(message),
            | Message::Expand(message) => self.handle_expand_message(message),
            | Message::Overlay(message) => self.handle_overlay_message(message),
            | Message::Structure(message) => self.handle_structure_message(message),
            | Message::MountFile(message) => self.handle_mount_and_file_message(message),
        }
    }

    fn handle_undo_redo(&mut self, message: UndoRedoMessage) -> Task<Message> {
        handle_undo_redo(self, message)
    }

    fn handle_shortcut_message(&mut self, message: ShortcutMessage) -> Task<Message> {
        handle_shortcut_message(self, message)
    }

    fn handle_error_message(&mut self, message: ErrorMessage) -> Task<Message> {
        handle_error_message(self, message)
    }

    fn handle_point_edited(
        &mut self, block_id: BlockId, action: text_editor::Action,
    ) -> Task<Message> {
        handle_point_edited(self, block_id, action)
    }

    fn handle_reduce_message(&mut self, message: ReduceMessage) -> Task<Message> {
        handle_reduce_message(self, message)
    }

    fn handle_expand_message(&mut self, message: ExpandMessage) -> Task<Message> {
        handle_expand_message(self, message)
    }

    fn handle_overlay_message(&mut self, message: OverlayMessage) -> Task<Message> {
        handle_overlay_message(self, message)
    }

    fn handle_structure_message(&mut self, message: StructureMessage) -> Task<Message> {
        handle_structure_message(self, message)
    }

    fn handle_mount_and_file_message(&mut self, message: MountFileMessage) -> Task<Message> {
        handle_mount_and_file_message(self, message)
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
}

#[derive(Debug, Clone)]
pub enum UndoRedoMessage {
    Undo,
    Redo,
}

#[derive(Debug, Clone)]
pub enum EditMessage {
    PointEdited { block_id: BlockId, action: text_editor::Action },
}

#[derive(Debug, Clone)]
pub enum ShortcutMessage {
    Trigger(ActionId),
    ForBlock { block_id: BlockId, action_id: ActionId },
}

#[derive(Debug, Clone)]
pub enum ErrorMessage {
    DismissAt(usize),
}

#[derive(Debug, Clone)]
pub enum ReduceMessage {
    Start(BlockId),
    Cancel(BlockId),
    Done {
        block_id: BlockId,
        request_signature: RequestSignature,
        result: Result<llm::ReduceResult, UiError>,
        children_snapshot: Vec<BlockId>,
    },
    Apply(BlockId),
    Reject(BlockId),
    AcceptChildDeletion {
        block_id: BlockId,
        child_index: usize,
    },
    RejectChildDeletion {
        block_id: BlockId,
        child_index: usize,
    },
    AcceptAllDeletions(BlockId),
    RejectAllDeletions(BlockId),
}

#[derive(Debug, Clone)]
pub enum ExpandMessage {
    Start(BlockId),
    Cancel(BlockId),
    Done {
        block_id: BlockId,
        request_signature: RequestSignature,
        result: Result<llm::ExpandResult, UiError>,
    },
    ApplyRewrite(BlockId),
    RejectRewrite(BlockId),
    AcceptChild {
        block_id: BlockId,
        child_index: usize,
    },
    RejectChild {
        block_id: BlockId,
        child_index: usize,
    },
    AcceptAllChildren(BlockId),
    DiscardAllChildren(BlockId),
}

#[derive(Debug, Clone)]
pub enum StructureMessage {
    AddChild(BlockId),
    AddSibling(BlockId),
    DuplicateBlock(BlockId),
    ArchiveBlock(BlockId),
    ToggleFold(BlockId),
    /// Add the given block as a friend of the target (for LLM context).
    AddFriendBlock { target: BlockId, friend_id: BlockId },
    /// Remove a friend from the target's friend list.
    RemoveFriendBlock { target: BlockId, friend_id: BlockId },
}

#[derive(Debug, Clone)]
pub enum OverlayMessage {
    ToggleOverflow(BlockId),
    CloseOverflow,
}

#[derive(Debug, Clone)]
pub enum MountFileMessage {
    ExpandMount(BlockId),
    CollapseMount(BlockId),
    SaveToFile(BlockId),
    SaveToFilePicked { block_id: BlockId, path: Option<std::path::PathBuf> },
    LoadFromFile(BlockId),
    LoadFromFilePicked { block_id: BlockId, path: Option<std::path::PathBuf> },
    SystemThemeChanged(Mode),
}

/// Process one message and return a follow-up task (if any).
pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    state.dispatch_message(message)
}

/// Global event subscription: keyboard shortcuts, mouse clicks, escape,
/// and system theme changes.
pub fn subscription(_state: &AppState) -> Subscription<Message> {
    Subscription::batch([
        event::listen_with(handle_event),
        system::theme_changes()
            .map(|mode| Message::MountFile(MountFileMessage::SystemThemeChanged(mode))),
    ])
}

fn handle_event(event: Event, status: event::Status, _window: iced::window::Id) -> Option<Message> {
    if status == event::Status::Captured {
        return None;
    }

    match event {
        | Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Overlay(OverlayMessage::CloseOverflow)),
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
            shortcut_to_action(key, modifiers).map(ShortcutMessage::Trigger).map(Message::Shortcut)
        }
        | Event::Mouse(mouse::Event::ButtonPressed(_)) => {
            Some(Message::Overlay(OverlayMessage::CloseOverflow))
        }
        | _ => None,
    }
}

fn run_shortcut_for_block(
    state: &mut AppState, block_id: BlockId, action_id: ActionId,
) -> Task<Message> {
    state.set_active_block(&block_id);

    let point_text =
        state.editor_buffers.get(&block_id).map(text_editor::Content::text).unwrap_or_default();
    let expansion_draft = state.store.expansion_draft(&block_id);
    let reduction_draft = state.store.reduction_draft(&block_id);
    let friend_block_ids = state
        .store
        .friend_blocks_for(&block_id)
        .iter()
        .map(|f| f.block_id)
        .collect();
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
        is_unexpanded_mount: state.store.node(&block_id).is_some_and(|n| n.mount_path().is_some()),
        active_block_id: state.active_block_id,
        friend_block_ids,
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
        return update(state, next);
    }

    Task::none()
}

fn handle_undo_redo(state: &mut AppState, message: UndoRedoMessage) -> Task<Message> {
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

fn handle_shortcut_message(state: &mut AppState, message: ShortcutMessage) -> Task<Message> {
    match message {
        | ShortcutMessage::Trigger(action_id) => {
            let Some(block_id) = state.current_block_for_shortcuts() else {
                return Task::none();
            };
            run_shortcut_for_block(state, block_id, action_id)
        }
        | ShortcutMessage::ForBlock { block_id, action_id } => {
            state.focused_block_id = Some(block_id);
            run_shortcut_for_block(state, block_id, action_id)
        }
    }
}

fn handle_error_message(state: &mut AppState, message: ErrorMessage) -> Task<Message> {
    match message {
        | ErrorMessage::DismissAt(index) => {
            if index < state.errors.len() {
                state.errors.remove(index);
                tracing::info!(
                    dismissed_index = index,
                    remaining = state.errors.len(),
                    "dismissed app error"
                );
            }
            Task::none()
        }
    }
}

fn handle_reduce_message(state: &mut AppState, message: ReduceMessage) -> Task<Message> {
    match message {
        | ReduceMessage::Start(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if state.llm_requests.is_reducing(block_id) {
                return Task::none();
            }
            let context = state.store.block_context_for_id(&block_id);
            let Some(config) = state.llm_config_for_reduce(block_id) else {
                return Task::none();
            };
            tracing::info!(block_id = ?block_id, "reduce request started");
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            let children_snapshot: Vec<BlockId> = state.store.children(&block_id).to_vec();
            state.llm_requests.mark_reduce_loading(block_id, request_signature);
            let request_task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(LLM_REQUEST_TIMEOUT, client.reduce_block(&context))
                            .await,
                        "reduce request timed out after 30 seconds",
                    )
                },
                move |result| {
                    Message::Reduce(ReduceMessage::Done {
                        block_id,
                        request_signature,
                        result,
                        children_snapshot,
                    })
                },
            );
            let (request_task, handle) = Task::abortable(request_task);
            state.llm_requests.replace_reduce_handle(block_id, handle);
            request_task
        }
        | ReduceMessage::Cancel(block_id) => {
            state.set_active_block(&block_id);
            if state.llm_requests.cancel_reduce(block_id) {
                tracing::info!(block_id = ?block_id, "reduce request cancelled");
            }
            Task::none()
        }
        | ReduceMessage::Done { block_id, request_signature, result, children_snapshot } => {
            let pending_signature = state.llm_requests.finish_reduce_request(block_id);
            if state.store.node(&block_id).is_none() {
                return Task::none();
            }
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale reduce response after context changed"
                );
                return Task::none();
            }
            match result {
                | Ok(reduce_result) => {
                    let (reduction, redundant_indices) = reduce_result.into_parts();
                    let redundant_children: Vec<BlockId> = redundant_indices
                        .iter()
                        .filter_map(|&idx| children_snapshot.get(idx).copied())
                        .collect();
                    tracing::info!(
                        block_id = ?block_id,
                        chars = reduction.len(),
                        redundant_children = redundant_children.len(),
                        "reduce request succeeded"
                    );
                    state.mutate_with_undo_and_persist("after creating reduction draft", |state| {
                        state.store.insert_reduction_draft(
                            block_id,
                            ReductionDraftRecord { reduction, redundant_children },
                        );
                        state.errors.retain(|err| !matches!(err, AppError::Reduce(_)));
                        true
                    });
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "reduce request failed");
                    state.llm_requests.set_reduce_error(block_id, reason.clone());
                    state.record_error(AppError::Reduce(reason));
                }
            }
            Task::none()
        }
        | ReduceMessage::Apply(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after applying reduction", |state| {
                if let Some(draft) = state.store.remove_reduction_draft(&block_id) {
                    tracing::info!(
                        block_id = ?block_id,
                        chars = draft.reduction.len(),
                        deletions = draft.redundant_children.len(),
                        "applied reduction with child deletions"
                    );
                    state.store.update_point(&block_id, draft.reduction.clone());
                    state.editor_buffers.set_text(&block_id, &draft.reduction);
                    for child_id in &draft.redundant_children {
                        if state.store.node(child_id).is_some()
                            && let Some(removed_ids) = state.store.remove_block_subtree(child_id)
                        {
                            state.editor_buffers.remove_blocks(&removed_ids);
                            for id in &removed_ids {
                                state.llm_requests.remove_block(*id);
                            }
                        }
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | ReduceMessage::Reject(block_id) => {
            state.set_active_block(&block_id);
            tracing::info!(block_id = ?block_id, "rejected reduction");
            state.store.remove_reduction_draft(&block_id);
            state.persist_with_context("after rejecting reduction");
            Task::none()
        }
        | ReduceMessage::AcceptChildDeletion { block_id, child_index } => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after accepting child deletion", |state| {
                let child_id = state
                    .store
                    .reduction_draft(&block_id)
                    .and_then(|d| d.redundant_children.get(child_index).copied())
                    .filter(|id| state.store.node(id).is_some());
                if let Some(child_id) = child_id
                    && let Some(removed_ids) = state.store.remove_block_subtree(&child_id)
                {
                    state.editor_buffers.remove_blocks(&removed_ids);
                    for id in &removed_ids {
                        state.llm_requests.remove_block(*id);
                    }
                }
                if let Some(draft) = state.store.reduction_draft(&block_id) {
                    let mut updated = draft.clone();
                    if child_index < updated.redundant_children.len() {
                        updated.redundant_children.remove(child_index);
                        state.store.insert_reduction_draft(block_id, updated);
                    }
                }
                true
            });
            Task::none()
        }
        | ReduceMessage::RejectChildDeletion { block_id, child_index } => {
            state.set_active_block(&block_id);
            if let Some(draft) = state.store.reduction_draft(&block_id) {
                let mut updated = draft.clone();
                if child_index < updated.redundant_children.len() {
                    updated.redundant_children.remove(child_index);
                    state.store.insert_reduction_draft(block_id, updated);
                }
            }
            state.persist_with_context("after rejecting child deletion");
            Task::none()
        }
        | ReduceMessage::AcceptAllDeletions(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after accepting all child deletions", |state| {
                let draft = state.store.reduction_draft(&block_id).cloned();
                if let Some(draft) = draft {
                    for child_id in &draft.redundant_children {
                        if state.store.node(child_id).is_some()
                            && let Some(removed_ids) = state.store.remove_block_subtree(child_id)
                        {
                            state.editor_buffers.remove_blocks(&removed_ids);
                            for id in &removed_ids {
                                state.llm_requests.remove_block(*id);
                            }
                        }
                    }
                    state.store.insert_reduction_draft(
                        block_id,
                        ReductionDraftRecord {
                            reduction: draft.reduction,
                            redundant_children: vec![],
                        },
                    );
                    return true;
                }
                false
            });
            Task::none()
        }
        | ReduceMessage::RejectAllDeletions(block_id) => {
            state.set_active_block(&block_id);
            if let Some(draft) = state.store.reduction_draft(&block_id) {
                state.store.insert_reduction_draft(
                    block_id,
                    ReductionDraftRecord {
                        reduction: draft.reduction.clone(),
                        redundant_children: vec![],
                    },
                );
            }
            state.persist_with_context("after rejecting all child deletions");
            Task::none()
        }
    }
}

fn handle_expand_message(state: &mut AppState, message: ExpandMessage) -> Task<Message> {
    match message {
        | ExpandMessage::Start(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if state.llm_requests.is_expanding(block_id) {
                return Task::none();
            }
            let context = state.store.block_context_for_id(&block_id);
            let Some(config) = state.llm_config_for_expand(block_id) else {
                return Task::none();
            };

            tracing::info!(block_id = ?block_id, "expand request started");
            let Some(request_signature) = RequestSignature::from_block_context(&context) else {
                return Task::none();
            };
            state.llm_requests.mark_expand_loading(block_id, request_signature);
            let request_task = Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    AppState::resolve_llm_request(
                        tokio::time::timeout(LLM_REQUEST_TIMEOUT, client.expand_block(&context))
                            .await,
                        "expand request timed out after 30 seconds",
                    )
                },
                move |result| {
                    Message::Expand(ExpandMessage::Done { block_id, request_signature, result })
                },
            );
            let (request_task, handle) = Task::abortable(request_task);
            state.llm_requests.replace_expand_handle(block_id, handle);
            request_task
        }
        | ExpandMessage::Cancel(block_id) => {
            state.set_active_block(&block_id);
            if state.llm_requests.cancel_expand(block_id) {
                tracing::info!(block_id = ?block_id, "expand request cancelled");
            }
            Task::none()
        }
        | ExpandMessage::Done { block_id, request_signature, result } => {
            let pending_signature = state.llm_requests.finish_expand_request(block_id);
            if state.store.node(&block_id).is_none() {
                return Task::none();
            }
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale expand response after point changed"
                );
                return Task::none();
            }
            match result {
                | Ok(raw_result) => {
                    let (rewrite, children) = raw_result.into_parts();
                    let rewrite =
                        rewrite.map(|value| value.trim().to_string()).filter(|v| !v.is_empty());
                    let children = children
                        .into_iter()
                        .map(llm::ExpandSuggestion::into_point)
                        .map(|value| value.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .collect::<Vec<_>>();
                    tracing::info!(
                        block_id = ?block_id,
                        has_rewrite = rewrite.is_some(),
                        child_count = children.len(),
                        "expand request succeeded"
                    );
                    if rewrite.is_none() && children.is_empty() {
                        let reason = UiError::from_message("expand returned no usable suggestions");
                        state.llm_requests.set_expand_error(block_id, reason.clone());
                        state.record_error(AppError::Expand(reason));
                        return Task::none();
                    }
                    state.mutate_with_undo_and_persist("after creating expansion draft", |state| {
                        state.store.insert_expansion_draft(
                            block_id,
                            ExpansionDraftRecord { rewrite, children },
                        );
                        state.errors.retain(|err| !matches!(err, AppError::Expand(_)));
                        true
                    });
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "expand request failed");
                    state.llm_requests.set_expand_error(block_id, reason.clone());
                    state.record_error(AppError::Expand(reason));
                }
            }
            Task::none()
        }
        | ExpandMessage::ApplyRewrite(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after applying rewrite", |state| {
                let mut should_save = false;
                let mut should_remove_draft = false;
                let mut applied_rewrite: Option<String> = None;
                if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                    applied_rewrite = draft.rewrite.take();
                    should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
                }
                if let Some(rewrite) = applied_rewrite {
                    tracing::info!(block_id = ?block_id, chars = rewrite.len(), "applied expanded rewrite");
                    state.store.update_point(&block_id, rewrite.clone());
                    state.editor_buffers.set_text(&block_id, &rewrite);
                    should_save = true;
                }
                if should_remove_draft {
                    state.store.remove_expansion_draft(&block_id);
                }
                should_save
            });
            Task::none()
        }
        | ExpandMessage::RejectRewrite(block_id) => {
            state.set_active_block(&block_id);
            let mut changed = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                draft.rewrite = None;
                tracing::info!(block_id = ?block_id, "rejected expanded rewrite");
                should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
                changed = true;
            }
            if should_remove_draft {
                state.store.remove_expansion_draft(&block_id);
            }
            if changed {
                state.persist_with_context("after rejecting rewrite");
            }
            Task::none()
        }
        | ExpandMessage::AcceptChild { block_id, child_index } => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after accepting expanded child", |state| {
                let mut should_save = false;
                let mut should_remove_draft = false;
                let mut accepted_child_point: Option<String> = None;
                if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                    if child_index < draft.children.len() {
                        accepted_child_point = Some(draft.children.remove(child_index));
                    }
                    if draft.rewrite.is_none() && draft.children.is_empty() {
                        should_remove_draft = true;
                    }
                }
                if let Some(point) = accepted_child_point
                    && let Some(child_id) = state.store.append_child(&block_id, point.clone())
                {
                    tracing::info!(
                        parent_block_id = ?block_id,
                        child_block_id = ?child_id,
                        chars = point.len(),
                        "accepted expanded child"
                    );
                    state.editor_buffers.set_text(&child_id, &point);
                    should_save = true;
                }
                if should_remove_draft {
                    state.store.remove_expansion_draft(&block_id);
                }
                should_save
            });
            Task::none()
        }
        | ExpandMessage::RejectChild { block_id, child_index } => {
            state.set_active_block(&block_id);
            let mut changed = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                if child_index < draft.children.len() {
                    draft.children.remove(child_index);
                    tracing::info!(block_id = ?block_id, child_index, "rejected expanded child");
                    changed = true;
                }
                should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
            }
            if should_remove_draft {
                state.store.remove_expansion_draft(&block_id);
            }
            if changed {
                state.persist_with_context("after rejecting expanded child");
            }
            Task::none()
        }
        | ExpandMessage::AcceptAllChildren(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after accepting expanded children", |state| {
                if let Some(mut draft) = state.store.remove_expansion_draft(&block_id) {
                    for point in draft.children.drain(..) {
                        if let Some(child_id) = state.store.append_child(&block_id, point.clone()) {
                            tracing::info!(
                                parent_block_id = ?block_id,
                                child_block_id = ?child_id,
                                chars = point.len(),
                                "accepted expanded child (bulk)"
                            );
                            state.editor_buffers.set_text(&child_id, &point);
                        }
                    }
                    if draft.rewrite.is_some() {
                        state.store.insert_expansion_draft(block_id, draft);
                    }
                    return true;
                }
                false
            });
            Task::none()
        }
        | ExpandMessage::DiscardAllChildren(block_id) => {
            state.set_active_block(&block_id);
            let mut changed = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.store.expansion_draft_mut(&block_id) {
                if !draft.children.is_empty() {
                    draft.children.clear();
                    tracing::info!(block_id = ?block_id, "discarded all expanded children");
                    changed = true;
                }
                should_remove_draft = draft.rewrite.is_none() && draft.children.is_empty();
            }
            if should_remove_draft {
                state.store.remove_expansion_draft(&block_id);
            }
            if changed {
                state.persist_with_context("after discarding expanded children");
            }
            Task::none()
        }
    }
}

fn handle_structure_message(state: &mut AppState, message: StructureMessage) -> Task<Message> {
    match message {
        | StructureMessage::AddChild(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            state.mutate_with_undo_and_persist("after adding child", |state| {
                if let Some(child_id) = state.store.append_child(&block_id, String::new()) {
                    tracing::info!(parent_block_id = ?block_id, child_block_id = ?child_id, "added child block");
                    state.editor_buffers.set_text(&child_id, "");
                    return true;
                }
                false
            });
            Task::none()
        }
        | StructureMessage::AddSibling(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after adding sibling", |state| {
                if let Some(sibling_id) = state.store.append_sibling(&block_id, String::new()) {
                    tracing::info!(block_id = ?block_id, sibling_block_id = ?sibling_id, "added sibling block");
                    state.editor_buffers.set_text(&sibling_id, "");
                    state.overflow_open_for = None;
                    return true;
                }
                false
            });
            Task::none()
        }
        | StructureMessage::DuplicateBlock(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after duplicating subtree", |state| {
                if let Some(duplicate_id) = state.store.duplicate_subtree_after(&block_id) {
                    tracing::info!(block_id = ?block_id, duplicate_block_id = ?duplicate_id, "duplicated block subtree");
                    state.editor_buffers.ensure_subtree(&state.store, &duplicate_id);
                    state.overflow_open_for = None;
                    return true;
                }
                false
            });
            Task::none()
        }
        | StructureMessage::ArchiveBlock(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            if let Some(removed_ids) = state.store.remove_block_subtree(&block_id) {
                tracing::info!(block_id = ?block_id, removed = removed_ids.len(), "archived block subtree");
                state.editor_buffers.remove_blocks(&removed_ids);
                for id in &removed_ids {
                    state.llm_requests.remove_block(*id);
                }
                if removed_ids.iter().any(|id| Some(*id) == state.focused_block_id) {
                    state.focused_block_id = None;
                }
                for root_id in state.store.roots() {
                    state.editor_buffers.ensure_block(&state.store, root_id);
                }
                state.overflow_open_for = None;
                if state.active_block_id == Some(block_id) {
                    state.active_block_id = state.store.roots().first().copied();
                }
                state.persist_with_context("after archiving subtree");
            }
            Task::none()
        }
        | StructureMessage::ToggleFold(block_id) => {
            state.store.toggle_collapsed(&block_id);
            state.persist_with_context("after toggling fold");
            Task::none()
        }
        | StructureMessage::AddFriendBlock { target, friend_id } => {
            state.set_active_block(&target);
            state.overflow_open_for = None;
            state.mutate_with_undo_and_persist("after adding friend block", |state| {
                if friend_id == target {
                    return false;
                }
                if state.store.node(&target).is_none() || state.store.node(&friend_id).is_none() {
                    return false;
                }
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                if friends.iter().any(|f| f.block_id == friend_id) {
                    return false;
                }
                friends.push(FriendBlock { block_id: friend_id, perspective: None });
                state.store.set_friend_blocks_for(&target, friends);
                tracing::info!(target = ?target, friend_id = ?friend_id, "added friend block");
                true
            });
            Task::none()
        }
        | StructureMessage::RemoveFriendBlock { target, friend_id } => {
            state.mutate_with_undo_and_persist("after removing friend block", |state| {
                let mut friends = state.store.friend_blocks_for(&target).to_vec();
                let prev = friends.len();
                friends.retain(|f| f.block_id != friend_id);
                if friends.len() == prev {
                    return false;
                }
                if friends.is_empty() {
                    state.store.set_friend_blocks_for(&target, vec![]);
                } else {
                    state.store.set_friend_blocks_for(&target, friends);
                }
                tracing::info!(target = ?target, friend_id = ?friend_id, "removed friend block");
                true
            });
            Task::none()
        }
    }
}

fn handle_overlay_message(state: &mut AppState, message: OverlayMessage) -> Task<Message> {
    match message {
        | OverlayMessage::ToggleOverflow(block_id) => {
            state.set_active_block(&block_id);
            if state.overflow_open_for == Some(block_id) {
                state.overflow_open_for = None;
            } else {
                state.overflow_open_for = Some(block_id);
            }
            Task::none()
        }
        | OverlayMessage::CloseOverflow => {
            state.overflow_open_for = None;
            state.focused_block_id = None;
            Task::none()
        }
    }
}

fn handle_mount_and_file_message(state: &mut AppState, message: MountFileMessage) -> Task<Message> {
    match message {
        | MountFileMessage::ExpandMount(block_id) => {
            state.set_active_block(&block_id);
            let base_dir = AppPaths::data_dir().unwrap_or_default();
            state.mutate_with_undo_and_persist("after expanding mount", |state| {
                match state.store.expand_mount(&block_id, &base_dir) {
                    | Ok(new_roots) => {
                        tracing::info!(block_id = ?block_id, children = new_roots.len(), "expanded mount");
                        for &id in &new_roots {
                            state.editor_buffers.ensure_subtree(&state.store, &id);
                        }
                        true
                    }
                    | Err(err) => {
                        tracing::error!(block_id = ?block_id, %err, "failed to expand mount");
            state.record_error(AppError::Mount(UiError::from_message(&err)));
                        false
                    }
                }
            });
            Task::none()
        }
        | MountFileMessage::CollapseMount(block_id) => {
            state.set_active_block(&block_id);
            state.mutate_with_undo_and_persist("after collapsing mount", |state| {
                if let Some(()) = state.store.collapse_mount(&block_id) {
                    tracing::info!(block_id = ?block_id, "collapsed mount");
                    state.editor_buffers = EditorBuffers::from_store(&state.store);
                    return true;
                }
                false
            });
            Task::none()
        }
        | MountFileMessage::SaveToFile(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title("Save block to file")
                        .add_filter("JSON", &["json"])
                        .add_filter("Markdown", &["md", "markdown"])
                        .save_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| {
                    Message::MountFile(MountFileMessage::SaveToFilePicked { block_id, path })
                },
            )
        }
        | MountFileMessage::SaveToFilePicked { block_id, path } => {
            if let Some(path) = path {
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                state.mutate_with_undo_and_persist("after save-to-file", |state| {
                    match state.store.save_subtree_to_file(&block_id, &path, &base_dir) {
                        | Ok(()) => {
                            let mount_format = state
                                .store
                                .node(&block_id)
                                .and_then(|node| node.mount_format())
                                .unwrap_or(MountFormat::Json);
                            tracing::info!(block_id = ?block_id, path = %path.display(), ?mount_format, "saved subtree to file");
                            match state.store.expand_mount(&block_id, &base_dir) {
                                | Ok(new_roots) => {
                                    for &id in &new_roots {
                                        state.editor_buffers.ensure_subtree(&state.store, &id);
                                    }
                                }
                                | Err(err) => {
                                    tracing::error!(block_id = ?block_id, %err, "failed to re-expand after save-to-file");
                                    state.record_error(AppError::Mount(UiError::from_message(&err)));
                                }
                            }
                            true
                        }
                        | Err(err) => {
                            tracing::error!(block_id = ?block_id, %err, "failed to save subtree to file");
                    state.record_error(AppError::Mount(UiError::from_message(&err)));
                            false
                        }
                    }
                });
            }
            Task::none()
        }
        | MountFileMessage::LoadFromFile(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title("Load block from file")
                        .add_filter("JSON", &["json"])
                        .add_filter("Markdown", &["md", "markdown"])
                        .pick_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| {
                    Message::MountFile(MountFileMessage::LoadFromFilePicked { block_id, path })
                },
            )
        }
        | MountFileMessage::LoadFromFilePicked { block_id, path } => {
            if let Some(path) = path {
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                state.mutate_with_undo_and_persist("after load-from-file", |state| {
                    let rel_path = path
                        .strip_prefix(&base_dir)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| path.clone());
                    let mount_format = match path
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .map(str::to_ascii_lowercase)
                        .as_deref()
                    {
                        | Some("md") | Some("markdown") => MountFormat::Markdown,
                        | _ => MountFormat::Json,
                    };
                    let mounted = match mount_format {
                        | MountFormat::Json => state.store.set_mount_path(&block_id, rel_path),
                        | MountFormat::Markdown => {
                            state.store.set_mount_path_with_format(&block_id, rel_path, mount_format)
                        }
                    };
                    if mounted.is_none() {
                        tracing::error!(block_id = ?block_id, "block has children or does not exist; cannot load");
                        return false;
                    }
                    match state.store.expand_mount(&block_id, &base_dir) {
                        | Ok(new_roots) => {
                            tracing::info!(block_id = ?block_id, path = %path.display(), children = new_roots.len(), "loaded file into block");
                            for &id in &new_roots {
                                state.editor_buffers.ensure_subtree(&state.store, &id);
                            }
                        }
                        | Err(err) => {
                            tracing::error!(block_id = ?block_id, %err, "failed to expand after load-from-file");
            state.record_error(AppError::Mount(UiError::from_message(&err)));
                        }
                    }
                    true
                });
            }
            Task::none()
        }
        | MountFileMessage::SystemThemeChanged(mode) => {
            let dark = matches!(mode, Mode::Dark);
            if state.is_dark != dark {
                tracing::info!(is_dark = dark, "system theme changed");
                state.is_dark = dark;
            }
            Task::none()
        }
    }
}

fn handle_point_edited(
    state: &mut AppState, block_id: BlockId, action: text_editor::Action,
) -> Task<Message> {
    state.set_active_block(&block_id);
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
        state.focused_block_id = Some(target_id);
        tracing::debug!(
            from = ?block_id,
            to = ?target_id,
            "keyboard traversal"
        );
        return widget::operation::focus(wid.clone());
    }
    Task::none()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ErrorBannerEntry {
    index: usize,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ErrorBanner {
    prefix: &'static str,
    latest: ErrorBannerEntry,
    previous_entries: Vec<ErrorBannerEntry>,
    hidden_previous_count: usize,
    total_count: usize,
}

impl ErrorBanner {
    fn from_state(state: &AppState) -> Option<Self> {
        let (latest_index, latest) = state.errors.iter().enumerate().last()?;
        let prefix = if state.persistence_blocked && matches!(latest, AppError::Persistence(_)) {
            "Recovery mode"
        } else {
            "Error"
        };
        let previous_entries = state
            .errors
            .iter()
            .enumerate()
            .rev()
            .skip(1)
            .take(ERROR_STACK_PREVIEW_LIMIT)
            .map(|(index, error)| ErrorBannerEntry { index, message: error.message().to_string() })
            .collect::<Vec<_>>();
        let hidden_previous_count = state.errors.len().saturating_sub(1 + previous_entries.len());
        Some(Self {
            prefix,
            latest: ErrorBannerEntry { index: latest_index, message: latest.message().to_string() },
            previous_entries,
            hidden_previous_count,
            total_count: state.errors.len(),
        })
    }

    fn title(&self) -> String {
        if self.total_count == 1 {
            format!("{}: {}", self.prefix, self.latest.message)
        } else {
            format!("{} ({} total): {}", self.prefix, self.total_count, self.latest.message)
        }
    }
}

pub fn view(state: &AppState) -> Element<'_, Message> {
    let mut layout = column![].spacing(theme::LAYOUT_GAP);
    if let Some(error_banner) = ErrorBanner::from_state(state) {
        let mut banner_content = column![
            row![
                text(error_banner.title()),
                button("Dismiss")
                    .on_press(Message::Error(ErrorMessage::DismissAt(error_banner.latest.index))),
            ]
            .spacing(8)
        ]
        .spacing(4);
        for entry in &error_banner.previous_entries {
            banner_content = banner_content.push(
                row![
                    text(format!("Earlier: {}", entry.message)),
                    button("Dismiss")
                        .on_press(Message::Error(ErrorMessage::DismissAt(entry.index))),
                ]
                .spacing(8),
            );
        }
        if error_banner.hidden_previous_count > 0 {
            banner_content = banner_content.push(text(format!(
                "...and {} older error(s)",
                error_banner.hidden_previous_count
            )));
        }
        layout = layout
            .push(container(banner_content).style(theme::error_banner).padding(theme::BANNER_PAD));
    }

    let tree = view::TreeView::new(state).render_roots();
    let content = container(tree).padding(theme::CANVAS_PAD).max_width(theme::CANVAS_MAX_WIDTH);
    layout = layout.push(
        scrollable(
            container(content)
                .width(Fill)
                .center_x(Fill)
                .padding(iced::Padding::ZERO.top(theme::CANVAS_TOP)),
        )
        .height(Fill),
    );

    container(layout).style(theme::canvas).width(Fill).height(Fill).into()
}

#[cfg(test)]
mod tests {
    use super::{AppError, AppState, Message, UiError, update};
    use super::{ErrorMessage, ExpandMessage, ReduceMessage};
    use crate::llm;
    use crate::store::{BlockStore, ExpansionDraftRecord, ReductionDraftRecord, StoreLoadError};
    use crate::undo::UndoHistory;
    fn test_state() -> (AppState, crate::store::BlockId) {
        let store = BlockStore::default();
        let root = *store.roots().first().expect("default store has a root");
        let state = AppState {
            editor_buffers: super::EditorBuffers::from_store(&store),
            store,
            undo_history: UndoHistory::with_capacity(64),
            llm_config: Ok(llm::LlmConfig::default()),
            errors: vec![],
            llm_requests: super::LlmRequests::new(),
            overflow_open_for: None,
            active_block_id: None,
            focused_block_id: None,
            editing_block_id: None,
            persistence_blocked: false,
            persistence_write_disabled: true,

            is_dark: false,
        };
        (state, root)
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
    fn error_banner_is_none_when_there_are_no_errors() {
        let (state, _) = test_state();
        assert!(super::ErrorBanner::from_state(&state).is_none());
    }

    #[test]
    fn error_banner_uses_latest_error_and_total_count() {
        let (mut state, _) = test_state();
        state.errors.push(AppError::Reduce(UiError::from_message("reduce failed")));
        state.errors.push(AppError::Expand(UiError::from_message("expand failed")));

        let banner = super::ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Error (2 total): expand failed");
        assert_eq!(
            banner.previous_entries,
            vec![super::ErrorBannerEntry { index: 0, message: "reduce failed".to_string() }]
        );
        assert_eq!(banner.hidden_previous_count, 0);
    }

    #[test]
    fn error_banner_uses_recovery_prefix_for_latest_persistence_error() {
        let (mut state, _) = test_state();
        state.persistence_blocked = true;
        state.errors.push(AppError::Persistence(UiError::from_message("persistence disabled")));

        let banner = super::ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Recovery mode: persistence disabled");
    }

    #[test]
    fn error_banner_limits_previous_preview_and_reports_hidden_count() {
        let (mut state, _) = test_state();
        state.errors.push(AppError::Mount(UiError::from_message("m1")));
        state.errors.push(AppError::Mount(UiError::from_message("m2")));
        state.errors.push(AppError::Mount(UiError::from_message("m3")));
        state.errors.push(AppError::Mount(UiError::from_message("m4")));
        state.errors.push(AppError::Mount(UiError::from_message("m5")));

        let banner = super::ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Error (5 total): m5");
        assert_eq!(
            banner.previous_entries,
            vec![
                super::ErrorBannerEntry { index: 3, message: "m4".to_string() },
                super::ErrorBannerEntry { index: 2, message: "m3".to_string() },
            ]
        );
        assert_eq!(banner.hidden_previous_count, 2);
    }

    #[test]
    fn dismiss_error_message_removes_selected_entry() {
        let (mut state, _) = test_state();
        state.errors.push(AppError::Mount(UiError::from_message("m1")));
        state.errors.push(AppError::Expand(UiError::from_message("e2")));
        state.errors.push(AppError::Reduce(UiError::from_message("r3")));

        let _ = update(&mut state, Message::Error(ErrorMessage::DismissAt(1)));

        assert_eq!(state.errors.len(), 2);
        assert_eq!(state.errors[0].message(), "m1");
        assert_eq!(state.errors[1].message(), "r3");
    }

    #[test]
    fn dismiss_error_message_out_of_bounds_is_noop() {
        let (mut state, _) = test_state();
        state.errors.push(AppError::Mount(UiError::from_message("m1")));

        let _ = update(&mut state, Message::Error(ErrorMessage::DismissAt(99)));

        assert_eq!(state.errors.len(), 1);
        assert_eq!(state.errors[0].message(), "m1");
    }

    #[test]
    fn expand_done_success_persists_draft_in_store() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ExpandResult::new(
                    Some("rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("child".to_string())],
                )),
            }),
        );
        let draft = state.store.expansion_draft(&root).expect("draft is created");
        assert_eq!(draft.rewrite.as_deref(), Some("rewrite"));
        assert_eq!(draft.children, vec!["child".to_string()]);
    }

    #[test]
    fn expand_done_stale_response_is_ignored() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        state.store.update_point(&root, "edited while pending".to_string());
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ExpandResult::new(
                    Some("stale rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("stale child".to_string())],
                )),
            }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn cancel_expand_clears_loading_state_and_pending_signature() {
        let (mut state, root) = test_state();
        let _ = update(&mut state, Message::Expand(ExpandMessage::Start(root)));
        assert!(state.llm_requests.is_expanding(root));
        assert!(state.llm_requests.has_pending_expand_signature(root));
        let _ = update(&mut state, Message::Expand(ExpandMessage::Cancel(root)));
        assert!(!state.llm_requests.is_expanding(root));
        assert!(!state.llm_requests.has_pending_expand_signature(root));
    }

    #[test]
    fn apply_expanded_rewrite_updates_point_and_clears_empty_draft() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: Some("rewritten point".to_string()), children: vec![] },
        );
        let _ = update(&mut state, Message::Expand(ExpandMessage::ApplyRewrite(root)));
        assert_eq!(state.store.point(&root).as_deref(), Some("rewritten point"));
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn reject_expanded_rewrite_keeps_child_suggestions() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: Some("rewrite".to_string()),
                children: vec!["child a".to_string()],
            },
        );
        let _ = update(&mut state, Message::Expand(ExpandMessage::RejectRewrite(root)));
        let draft = state.store.expansion_draft(&root).expect("draft remains with children");
        assert!(draft.rewrite.is_none());
        assert_eq!(draft.children, vec!["child a".to_string()]);
    }

    #[test]
    fn accept_expanded_child_appends_child_and_updates_draft() {
        let (mut state, root) = test_state();
        let before_children_len = state.store.children(&root).len();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: None,
                children: vec!["child a".to_string(), "child b".to_string()],
            },
        );
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::AcceptChild { block_id: root, child_index: 0 }),
        );
        let children = state.store.children(&root);
        assert_eq!(children.len(), before_children_len + 1);
        let child_id = *children.last().expect("new child is appended");
        assert_eq!(state.store.point(&child_id).as_deref(), Some("child a"));
        let draft = state.store.expansion_draft(&root).expect("draft remains with one child");
        assert_eq!(draft.children, vec!["child b".to_string()]);
    }

    #[test]
    fn accept_all_expanded_children_keeps_rewrite_and_clears_children() {
        let (mut state, root) = test_state();
        let before_children_len = state.store.children(&root).len();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord {
                rewrite: Some("rewrite".to_string()),
                children: vec!["child a".to_string(), "child b".to_string()],
            },
        );
        let _ = update(&mut state, Message::Expand(ExpandMessage::AcceptAllChildren(root)));
        let children = state.store.children(&root);
        assert_eq!(children.len(), before_children_len + 2);
        let first = children[before_children_len];
        let second = children[before_children_len + 1];
        assert_eq!(state.store.point(&first).as_deref(), Some("child a"));
        assert_eq!(state.store.point(&second).as_deref(), Some("child b"));
        let draft = state.store.expansion_draft(&root).expect("rewrite-only draft remains");
        assert_eq!(draft.rewrite.as_deref(), Some("rewrite"));
        assert!(draft.children.is_empty());
    }

    #[test]
    fn discard_all_expanded_children_removes_empty_draft() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: None, children: vec!["child a".to_string()] },
        );
        let _ = update(&mut state, Message::Expand(ExpandMessage::DiscardAllChildren(root)));
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn discard_all_expanded_children_after_reexpand_preserves_rewrite() {
        let (mut state, root) = test_state();

        let first_signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, first_signature);
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: first_signature,
                result: Ok(llm::ExpandResult::new(
                    Some("first rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("first child".to_string())],
                )),
            }),
        );
        let _ = update(&mut state, Message::Expand(ExpandMessage::AcceptAllChildren(root)));

        let second_signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, second_signature);
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: second_signature,
                result: Ok(llm::ExpandResult::new(
                    Some("second rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("second child".to_string())],
                )),
            }),
        );

        let _ = update(&mut state, Message::Expand(ExpandMessage::DiscardAllChildren(root)));

        let draft = state.store.expansion_draft(&root).expect("rewrite draft remains");
        assert_eq!(draft.rewrite.as_deref(), Some("second rewrite"));
        assert!(draft.children.is_empty());
    }

    #[test]
    fn reduce_done_success_persists_draft_in_store() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_reduce_loading(root, signature);
        let _ = update(
            &mut state,
            Message::Reduce(ReduceMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ReduceResult::new("reduced".to_string(), vec![])),
                children_snapshot: vec![],
            }),
        );
        let draft = state.store.reduction_draft(&root).expect("reduction draft is created");
        assert_eq!(draft.reduction, "reduced".to_string());
    }

    #[test]
    fn reduce_done_stale_response_is_ignored() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_reduce_loading(root, signature);
        state.store.update_point(&root, "edited while pending".to_string());
        let _ = update(
            &mut state,
            Message::Reduce(ReduceMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ReduceResult::new("stale reduction".to_string(), vec![])),
                children_snapshot: vec![],
            }),
        );
        assert!(state.store.reduction_draft(&root).is_none());
    }

    #[test]
    fn cancel_reduce_clears_loading_state_and_pending_signature() {
        let (mut state, root) = test_state();
        let _ = update(&mut state, Message::Reduce(ReduceMessage::Start(root)));
        assert!(state.llm_requests.is_reducing(root));
        assert!(state.llm_requests.has_pending_reduce_signature(root));
        let _ = update(&mut state, Message::Reduce(ReduceMessage::Cancel(root)));
        assert!(!state.llm_requests.is_reducing(root));
        assert!(!state.llm_requests.has_pending_reduce_signature(root));
    }

    #[test]
    fn apply_reduction_updates_point_and_clears_draft() {
        let (mut state, root) = test_state();
        state.store.insert_reduction_draft(
            root,
            ReductionDraftRecord {
                reduction: "reduced point".to_string(),
                redundant_children: vec![],
            },
        );
        let _ = update(&mut state, Message::Reduce(ReduceMessage::Apply(root)));
        assert_eq!(state.store.point(&root).as_deref(), Some("reduced point"));
        assert!(state.store.reduction_draft(&root).is_none());
    }

    #[test]
    fn reject_reduction_clears_draft() {
        let (mut state, root) = test_state();
        state.store.insert_reduction_draft(
            root,
            ReductionDraftRecord {
                reduction: "reduced point".to_string(),
                redundant_children: vec![],
            },
        );
        let _ = update(&mut state, Message::Reduce(ReduceMessage::Reject(root)));
        assert!(state.store.reduction_draft(&root).is_none());
    }

    #[test]
    fn reject_expanded_child_removes_draft_when_last_child() {
        let (mut state, root) = test_state();
        state.store.insert_expansion_draft(
            root,
            ExpansionDraftRecord { rewrite: None, children: vec!["only child".to_string()] },
        );
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::RejectChild { block_id: root, child_index: 0 }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
    }

    #[test]
    fn expand_done_error_sets_expand_error_state() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Err(super::UiError::from_message("failed")),
            }),
        );
        assert!(state.llm_requests.has_expand_error(root));
    }

    #[test]
    fn reduce_done_error_sets_reduce_error_state() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_reduce_loading(root, signature);
        let _ = update(
            &mut state,
            Message::Reduce(ReduceMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Err(super::UiError::from_message("failed")),
                children_snapshot: vec![],
            }),
        );
        assert!(state.llm_requests.has_reduce_error(root));
    }

    #[test]
    fn cancel_expand_then_late_response_is_ignored() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_expand_loading(root, signature);
        let _ = update(&mut state, Message::Expand(ExpandMessage::Cancel(root)));
        let _ = update(
            &mut state,
            Message::Expand(ExpandMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ExpandResult::new(
                    Some("late rewrite".to_string()),
                    vec![llm::ExpandSuggestion::new("late child".to_string())],
                )),
            }),
        );
        assert!(state.store.expansion_draft(&root).is_none());
        assert!(!state.llm_requests.is_expanding(root));
        assert!(!state.llm_requests.has_expand_error(root));
    }

    #[test]
    fn cancel_reduce_then_late_response_is_ignored() {
        let (mut state, root) = test_state();
        let signature = state.block_context_signature(&root).expect("root has lineage");
        state.llm_requests.mark_reduce_loading(root, signature);
        let _ = update(&mut state, Message::Reduce(ReduceMessage::Cancel(root)));
        let _ = update(
            &mut state,
            Message::Reduce(ReduceMessage::Done {
                block_id: root,
                request_signature: signature,
                result: Ok(llm::ReduceResult::new("late reduction".to_string(), vec![])),
                children_snapshot: vec![],
            }),
        );
        assert!(state.store.reduction_draft(&root).is_none());
        assert!(!state.llm_requests.is_reducing(root));
        assert!(!state.llm_requests.has_reduce_error(root));
    }

    #[test]
    fn reduce_handles_are_isolated_per_block_on_cancel() {
        let (mut state, root) = test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");

        let _ = update(&mut state, Message::Reduce(ReduceMessage::Start(root)));
        let _ = update(&mut state, Message::Reduce(ReduceMessage::Start(sibling)));

        assert!(state.llm_requests.has_reduce_handle(root));
        assert!(state.llm_requests.has_reduce_handle(sibling));

        let _ = update(&mut state, Message::Reduce(ReduceMessage::Cancel(root)));

        assert!(!state.llm_requests.has_reduce_handle(root));
        assert!(state.llm_requests.has_reduce_handle(sibling));
    }

    #[test]
    fn expand_handles_are_isolated_per_block_on_cancel() {
        let (mut state, root) = test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");

        let _ = update(&mut state, Message::Expand(ExpandMessage::Start(root)));
        let _ = update(&mut state, Message::Expand(ExpandMessage::Start(sibling)));

        assert!(state.llm_requests.has_expand_handle(root));
        assert!(state.llm_requests.has_expand_handle(sibling));

        let _ = update(&mut state, Message::Expand(ExpandMessage::Cancel(root)));

        assert!(!state.llm_requests.has_expand_handle(root));
        assert!(state.llm_requests.has_expand_handle(sibling));
    }
}
