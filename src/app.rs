//! Application state, messages, update, and view for the iced UI.
//!
//! The underlying document is a block store (each block with a slotmap id); the UI presents
//! the same content as a tree (roots and ordered children per node).

use crate::llm;
use crate::paths::AppPaths;
use crate::store::{BlockId, BlockStore};
use crate::theme;
use crate::undo::UndoHistory;
use std::collections::HashSet;
mod action_bar;
mod diff;
mod draft;
mod editor_store;
mod state;
mod view;

use draft::{ExpansionDraft, ReductionDraft};
use editor_store::EditorStore;
use state::{AppError, ExpandState, ReduceState, RequestSignature, UiError};

use action_bar::{
    ActionAvailability, ActionId, RowContext, ViewportBucket, action_to_message_by_id,
    build_action_bar_vm, project_for_viewport, shortcut_to_action,
};
use iced::theme::Mode;
use iced::widget::{column, container, scrollable, text, text_editor};
use iced::{Element, Event, Fill, Subscription, Task, event, keyboard, mouse, system, widget};
use slotmap::SecondaryMap;

/// Snapshot of undoable application state.
///
/// Contains only the store and expansion drafts. Editor buffers are
/// rebuilt from the store on restore since `text_editor::Content` is
/// not cheaply cloneable with full cursor state.
#[derive(Clone)]
struct UndoSnapshot {
    store: BlockStore,
    expansion_drafts: SecondaryMap<BlockId, ExpansionDraft>,
    reduction_drafts: SecondaryMap<BlockId, ReductionDraft>,
}

/// Default capacity: 64 undo steps.
const UNDO_CAPACITY: usize = 64;

/// All mutable application state for the iced Elm architecture.
///
/// Owns the document store, editor buffers, undo history, LLM config,
/// async operation states, and transient UI state (overflow, active/focused/editing block ids).
#[derive(Clone)]
pub struct AppState {
    store: BlockStore,
    undo_history: UndoHistory<UndoSnapshot>,
    llm_config: Result<llm::LlmConfig, llm::LlmConfigError>,
    error: Option<AppError>,
    reduce_states: SecondaryMap<BlockId, ReduceState>,
    expand_states: SecondaryMap<BlockId, ExpandState>,
    pending_reduce_signatures: SecondaryMap<BlockId, RequestSignature>,
    pending_expand_signatures: SecondaryMap<BlockId, RequestSignature>,
    expansion_drafts: SecondaryMap<BlockId, ExpansionDraft>,
    reduction_drafts: SecondaryMap<BlockId, ReductionDraft>,
    editors: EditorStore,
    overflow_open_for: Option<BlockId>,
    /// Last block interacted with by actions or edits.
    active_block_id: Option<BlockId>,
    /// Block whose point editor currently has keyboard focus.
    focused_block_id: Option<BlockId>,
    /// Block currently coalescing point edits into a single undo entry.
    editing_block_id: Option<BlockId>,
    /// Blocks whose children are folded (hidden) in the UI.
    /// View-only state: not persisted, not part of undo.
    collapsed: HashSet<BlockId>,
    /// Whether the current theme is dark. Detected from the system at startup
    /// and updated live via `iced::system::theme_changes()`.
    pub is_dark: bool,
}

impl AppState {
    pub fn load() -> Self {
        let llm_config = llm::LlmConfig::load();
        let error = llm_config
            .as_ref()
            .err()
            .map(|err| AppError::Configuration(UiError::from_message(err)));
        let store = BlockStore::load();
        let mut expansion_drafts = SecondaryMap::new();
        for (id, draft) in store.expansion_drafts().iter() {
            expansion_drafts.insert(id, ExpansionDraft::from_record(draft.clone()));
        }
        let mut reduction_drafts = SecondaryMap::new();
        for (id, draft) in store.reduction_drafts().iter() {
            reduction_drafts.insert(id, ReductionDraft::from_record(draft.clone()));
        }
        let editors = EditorStore::from_store(&store);
        let is_dark = matches!(dark_light::detect(), Ok(dark_light::Mode::Dark));
        tracing::info!(is_dark, "detected system appearance");
        Self {
            store,
            undo_history: UndoHistory::with_capacity(UNDO_CAPACITY),
            llm_config,
            error,
            reduce_states: SecondaryMap::new(),
            expand_states: SecondaryMap::new(),
            pending_reduce_signatures: SecondaryMap::new(),
            pending_expand_signatures: SecondaryMap::new(),
            expansion_drafts,
            reduction_drafts,
            editors,
            overflow_open_for: None,
            active_block_id: None,
            focused_block_id: None,
            editing_block_id: None,
            collapsed: HashSet::new(),
            is_dark,
        }
    }

    fn sync_drafts_to_store(&mut self) {
        let mut expansion = SecondaryMap::new();
        for (id, draft) in self.expansion_drafts.iter() {
            expansion.insert(id, draft.to_record());
        }
        let mut reduction = SecondaryMap::new();
        for (id, draft) in self.reduction_drafts.iter() {
            reduction.insert(id, draft.to_record());
        }
        self.store.replace_drafts(expansion, reduction);
    }

    fn save_tree(&mut self) -> std::io::Result<()> {
        self.sync_drafts_to_store();
        self.store.save()?;
        self.store.save_mounts()
    }

    fn is_reducing(&self, block_id: &BlockId) -> bool {
        self.reduce_states.get(*block_id).is_some_and(|s| matches!(s, ReduceState::Loading))
    }

    fn is_expanding(&self, block_id: &BlockId) -> bool {
        self.expand_states.get(*block_id).is_some_and(|s| matches!(s, ExpandState::Loading))
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
        self.sync_drafts_to_store();
        self.undo_history.push(UndoSnapshot {
            store: self.store.clone(),
            expansion_drafts: self.expansion_drafts.clone(),
            reduction_drafts: self.reduction_drafts.clone(),
        });
        self.editing_block_id = None;
    }

    fn restore_snapshot(&mut self, snapshot: UndoSnapshot) {
        self.editors = EditorStore::from_store(&snapshot.store);
        self.store = snapshot.store;
        self.expansion_drafts = snapshot.expansion_drafts;
        self.reduction_drafts = snapshot.reduction_drafts;
        self.reduce_states.clear();
        self.expand_states.clear();
        self.pending_reduce_signatures.clear();
        self.pending_expand_signatures.clear();
        self.focused_block_id = None;
        self.editing_block_id = None;
        self.active_block_id = self.store.roots().first().copied();
        if let Err(err) = self.save_tree() {
            tracing::error!(%err, "failed to save tree after undo/redo");
        }
    }

    fn lineage_signature(&self, block_id: &BlockId) -> Option<RequestSignature> {
        let lineage = self.store.lineage_points_for_id(block_id);
        RequestSignature::from_lineage(&lineage)
    }

    fn is_stale_response(&self, block_id: &BlockId, request_signature: RequestSignature) -> bool {
        self.lineage_signature(block_id)
            .is_none_or(|current_signature| current_signature != request_signature)
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
    Undo,
    Redo,
    PointEdited(BlockId, text_editor::Action),
    Shortcut(ActionId),
    ShortcutFor(BlockId, ActionId),
    Reduce(BlockId),
    ReduceDone(BlockId, RequestSignature, Result<String, UiError>),
    ApplyReduction(BlockId),
    RejectReduction(BlockId),
    Expand(BlockId),
    ExpandDone(BlockId, RequestSignature, Result<llm::ExpandResult, UiError>),
    ApplyExpandedRewrite(BlockId),
    RejectExpandedRewrite(BlockId),
    AcceptExpandedChild(BlockId, usize),
    RejectExpandedChild(BlockId, usize),
    AcceptAllExpandedChildren(BlockId),
    DiscardExpansion(BlockId),
    AddChild(BlockId),
    AddSibling(BlockId),
    DuplicateBlock(BlockId),
    ArchiveBlock(BlockId),
    ToggleOverflow(BlockId),
    CloseOverflow,
    ToggleFold(BlockId),
    ExpandMount(BlockId),
    CollapseMount(BlockId),
    SaveToFile(BlockId),
    SaveToFilePicked(BlockId, Option<std::path::PathBuf>),
    LoadFromFile(BlockId),
    LoadFromFilePicked(BlockId, Option<std::path::PathBuf>),
    SystemThemeChanged(Mode),
}

/// Process one message and return a follow-up task (if any).
pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    match message {
        | Message::Undo => {
            state.sync_drafts_to_store();
            let current = UndoSnapshot {
                store: state.store.clone(),
                expansion_drafts: state.expansion_drafts.clone(),
                reduction_drafts: state.reduction_drafts.clone(),
            };
            if let Some(previous) = state.undo_history.undo(current) {
                tracing::info!("undo applied");
                state.restore_snapshot(previous);
            }
            Task::none()
        }
        | Message::Redo => {
            state.sync_drafts_to_store();
            let current = UndoSnapshot {
                store: state.store.clone(),
                expansion_drafts: state.expansion_drafts.clone(),
                reduction_drafts: state.reduction_drafts.clone(),
            };
            if let Some(next) = state.undo_history.redo(current) {
                tracing::info!("redo applied");
                state.restore_snapshot(next);
            }
            Task::none()
        }
        | Message::Shortcut(action_id) => {
            let Some(block_id) = state.current_block_for_shortcuts() else {
                return Task::none();
            };
            run_shortcut_for_block(state, block_id, action_id)
        }
        | Message::ShortcutFor(block_id, action_id) => {
            state.focused_block_id = Some(block_id);
            run_shortcut_for_block(state, block_id, action_id)
        }
        | Message::PointEdited(block_id, action) => {
            state.set_active_block(&block_id);
            state.focused_block_id = Some(block_id);
            if state.editing_block_id.as_ref() != Some(&block_id) {
                state.snapshot_for_undo();
                state.editing_block_id = Some(block_id);
            }
            state.editors.ensure_block(&state.store, &block_id);

            // Detect vertical direction of the action (if any) before
            // performing it, so we can check visual edge after the move.
            let vertical_direction = match &action {
                | text_editor::Action::Move(text_editor::Motion::Up) => Some(VerticalDir::Up),
                | text_editor::Action::Move(text_editor::Motion::Down) => Some(VerticalDir::Down),
                | _ => None,
            };

            // Phase 1: perform the action and detect if navigation is needed.
            // We must drop the mutable borrow on `state.editors` before
            // calling `widget_id` (immutable borrow) in phase 2.
            let mut navigate_to: Option<BlockId> = None;
            if let Some(content) = state.editors.get_mut(&block_id) {
                let cursor_before = content.cursor().position;
                content.perform(action);
                let cursor_after = content.cursor().position;

                // Edge-detection: if a vertical move did not change the cursor
                // position, we are at the visual boundary (accounting for
                // wrapped lines) and should navigate to the adjacent block.
                if let Some(dir) = vertical_direction {
                    if cursor_before == cursor_after {
                        navigate_to = match dir {
                            | VerticalDir::Up => {
                                state.store.prev_visible_in_dfs(&block_id, &state.collapsed)
                            }
                            | VerticalDir::Down => {
                                state.store.next_visible_in_dfs(&block_id, &state.collapsed)
                            }
                        };
                    }
                }

                // If we are NOT navigating away, persist the text change.
                if navigate_to.is_none() {
                    let next_text = content.text();
                    tracing::debug!(block_id = ?block_id, chars = next_text.len(), "point edited");
                    state.store.update_point(&block_id, next_text);
                    if let Err(err) = state.save_tree() {
                        tracing::error!(%err, "failed to save tree after edit");
                    }
                }
            } // mutable borrow on `state.editors` dropped here

            // Phase 2: navigate to the adjacent block (immutable borrow).
            if let Some(target_id) = navigate_to {
                if let Some(wid) = state.editors.widget_id(&target_id) {
                    state.focused_block_id = Some(target_id);
                    tracing::debug!(
                        from = ?block_id,
                        to = ?target_id,
                        "keyboard traversal"
                    );
                    return widget::operation::focus(wid.clone());
                }
            }
            Task::none()
        }
        | Message::Reduce(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if state.is_reducing(&block_id) {
                return Task::none();
            }
            let lineage = state.store.lineage_points_for_id(&block_id);
            let config = match &state.llm_config {
                | Ok(config) => config.clone(),
                | Err(err) => {
                    let ui_err = UiError::from_message(err);
                    state.error = Some(AppError::Configuration(ui_err.clone()));
                    state.reduce_states.insert(block_id, ReduceState::Error { reason: ui_err });
                    return Task::none();
                }
            };
            tracing::info!(block_id = ?block_id, "reduce request started");
            let Some(request_signature) = RequestSignature::from_lineage(&lineage) else {
                return Task::none();
            };
            state.reduce_states.insert(block_id, ReduceState::Loading);
            state.pending_reduce_signatures.insert(block_id, request_signature);
            Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    client.reduce_lineage(&lineage).await.map_err(UiError::from_message)
                },
                move |result| Message::ReduceDone(block_id, request_signature, result),
            )
        }
        | Message::ReduceDone(block_id, request_signature, result) => {
            state.reduce_states.remove(block_id);
            if state.store.node(&block_id).is_none() {
                state.pending_reduce_signatures.remove(block_id);
                return Task::none();
            }
            let pending_signature = state.pending_reduce_signatures.remove(block_id);
            if pending_signature != Some(request_signature)
                || state.is_stale_response(&block_id, request_signature)
            {
                tracing::info!(
                    block_id = ?block_id,
                    "discarded stale reduce response after point changed"
                );
                return Task::none();
            }
            match result {
                | Ok(reduction) => {
                    tracing::info!(block_id = ?block_id, chars = reduction.len(), "reduce request succeeded");
                    state.snapshot_for_undo();
                    state.reduction_drafts.insert(block_id, ReductionDraft::new(reduction));
                    state.error = None;
                    if let Err(err) = state.save_tree() {
                        tracing::error!(%err, "failed to save tree after creating reduction draft");
                    }
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "reduce request failed");
                    state
                        .reduce_states
                        .insert(block_id, ReduceState::Error { reason: reason.clone() });
                    state.error = Some(AppError::Reduce(reason));
                }
            }
            Task::none()
        }
        | Message::ApplyReduction(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            let mut should_save = false;
            if let Some(draft) = state.reduction_drafts.remove(block_id) {
                tracing::info!(block_id = ?block_id, chars = draft.reduction.len(), "applied reduction");
                state.store.update_point(&block_id, draft.reduction.clone());
                state.editors.set_text(&block_id, &draft.reduction);
                should_save = true;
            }
            if should_save {
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after applying reduction");
                }
            }
            Task::none()
        }
        | Message::RejectReduction(block_id) => {
            state.set_active_block(&block_id);
            tracing::info!(block_id = ?block_id, "rejected reduction");
            state.reduction_drafts.remove(block_id);
            if let Err(err) = state.save_tree() {
                tracing::error!(%err, "failed to save tree after rejecting reduction");
            }
            Task::none()
        }
        | Message::Expand(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if state.is_expanding(&block_id) {
                return Task::none();
            }
            let lineage = state.store.lineage_points_for_id(&block_id);
            let config = match &state.llm_config {
                | Ok(config) => config.clone(),
                | Err(err) => {
                    let ui_err = UiError::from_message(err);
                    state.error = Some(AppError::Configuration(ui_err.clone()));
                    state.expand_states.insert(block_id, ExpandState::Error { reason: ui_err });
                    return Task::none();
                }
            };

            tracing::info!(block_id = ?block_id, "expand request started");
            let Some(request_signature) = RequestSignature::from_lineage(&lineage) else {
                return Task::none();
            };
            state.expand_states.insert(block_id, ExpandState::Loading);
            state.pending_expand_signatures.insert(block_id, request_signature);
            Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    client.expand_lineage(&lineage).await.map_err(UiError::from_message)
                },
                move |result| Message::ExpandDone(block_id, request_signature, result),
            )
        }
        | Message::ExpandDone(block_id, request_signature, result) => {
            state.expand_states.remove(block_id);
            if state.store.node(&block_id).is_none() {
                state.pending_expand_signatures.remove(block_id);
                return Task::none();
            }
            let pending_signature = state.pending_expand_signatures.remove(block_id);
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
                    let draft = ExpansionDraft::from_expand_result(raw_result);
                    tracing::info!(
                        block_id = ?block_id,
                        has_rewrite = draft.rewrite.is_some(),
                        child_count = draft.children.len(),
                        "expand request succeeded"
                    );
                    if draft.is_empty() {
                        let reason = UiError::from_message("expand returned no usable suggestions");
                        state
                            .expand_states
                            .insert(block_id, ExpandState::Error { reason: reason.clone() });
                        state.error = Some(AppError::Expand(reason));
                        return Task::none();
                    }
                    state.snapshot_for_undo();
                    state.expansion_drafts.insert(block_id, draft);
                    state.error = None;
                    if let Err(err) = state.save_tree() {
                        tracing::error!(%err, "failed to save tree after creating expansion draft");
                    }
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "expand request failed");
                    state
                        .expand_states
                        .insert(block_id, ExpandState::Error { reason: reason.clone() });
                    state.error = Some(AppError::Expand(reason));
                }
            }
            Task::none()
        }
        | Message::ApplyExpandedRewrite(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            let mut should_save = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.expansion_drafts.get_mut(block_id) {
                if let Some(rewrite) = draft.rewrite.take() {
                    tracing::info!(block_id = ?block_id, chars = rewrite.len(), "applied expanded rewrite");
                    state.store.update_point(&block_id, rewrite.clone());
                    state.editors.set_text(&block_id, &rewrite);
                    should_save = true;
                }
                if draft.is_empty() {
                    should_remove_draft = true;
                }
            }
            if should_remove_draft {
                state.expansion_drafts.remove(block_id);
            }
            if should_save {
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after applying rewrite");
                }
            }
            Task::none()
        }
        | Message::RejectExpandedRewrite(block_id) => {
            state.set_active_block(&block_id);
            let mut changed = false;
            if let Some(draft) = state.expansion_drafts.get_mut(block_id) {
                draft.rewrite = None;
                tracing::info!(block_id = ?block_id, "rejected expanded rewrite");
                if draft.is_empty() {
                    state.expansion_drafts.remove(block_id);
                }
                changed = true;
            }
            if changed {
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after rejecting rewrite");
                }
            }
            Task::none()
        }
        | Message::AcceptExpandedChild(block_id, child_index) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            let mut should_save = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.expansion_drafts.get_mut(block_id) {
                if child_index < draft.children.len() {
                    let point = draft.children.remove(child_index);
                    if let Some(child_id) = state.store.append_child(&block_id, point.clone()) {
                        tracing::info!(
                            parent_block_id = ?block_id,
                            child_block_id = ?child_id,
                            chars = point.len(),
                            "accepted expanded child"
                        );
                        state.editors.set_text(&child_id, &point);
                        should_save = true;
                    }
                }
                if draft.is_empty() {
                    should_remove_draft = true;
                }
            }
            if should_remove_draft {
                state.expansion_drafts.remove(block_id);
            }
            if should_save {
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after accepting expanded child");
                }
            }
            Task::none()
        }
        | Message::RejectExpandedChild(block_id, child_index) => {
            state.set_active_block(&block_id);
            let mut changed = false;
            if let Some(draft) = state.expansion_drafts.get_mut(block_id) {
                if child_index < draft.children.len() {
                    draft.children.remove(child_index);
                    tracing::info!(block_id = ?block_id, child_index, "rejected expanded child");
                    changed = true;
                }
                if draft.is_empty() {
                    state.expansion_drafts.remove(block_id);
                }
            }
            if changed {
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after rejecting expanded child");
                }
            }
            Task::none()
        }
        | Message::AcceptAllExpandedChildren(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            if let Some(mut draft) = state.expansion_drafts.remove(block_id) {
                for point in draft.children.drain(..) {
                    if let Some(child_id) = state.store.append_child(&block_id, point.clone()) {
                        tracing::info!(
                            parent_block_id = ?block_id,
                            child_block_id = ?child_id,
                            chars = point.len(),
                            "accepted expanded child (bulk)"
                        );
                        state.editors.set_text(&child_id, &point);
                    }
                }
                if draft.rewrite.is_some() {
                    state.expansion_drafts.insert(block_id, draft);
                }
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after accepting expanded children");
                }
            }
            Task::none()
        }
        | Message::DiscardExpansion(block_id) => {
            state.set_active_block(&block_id);
            tracing::info!(block_id = ?block_id, "discarded expansion draft");
            if state.expansion_drafts.remove(block_id).is_some() {
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after discarding expansion draft");
                }
            }
            Task::none()
        }
        | Message::ToggleOverflow(block_id) => {
            state.set_active_block(&block_id);
            if state.overflow_open_for == Some(block_id) {
                state.overflow_open_for = None;
            } else {
                state.overflow_open_for = Some(block_id);
            }
            Task::none()
        }
        | Message::CloseOverflow => {
            state.overflow_open_for = None;
            state.focused_block_id = None;
            Task::none()
        }
        | Message::AddChild(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            state.snapshot_for_undo();
            if let Some(child_id) = state.store.append_child(&block_id, String::new()) {
                tracing::info!(parent_block_id = ?block_id, child_block_id = ?child_id, "added child block");
                state.editors.set_text(&child_id, "");
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after adding child");
                }
            }
            Task::none()
        }
        | Message::AddSibling(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            if let Some(sibling_id) = state.store.append_sibling(&block_id, String::new()) {
                tracing::info!(block_id = ?block_id, sibling_block_id = ?sibling_id, "added sibling block");
                state.editors.set_text(&sibling_id, "");
                state.overflow_open_for = None;
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after adding sibling");
                }
            }
            Task::none()
        }
        | Message::DuplicateBlock(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            if let Some(duplicate_id) = state.store.duplicate_subtree_after(&block_id) {
                tracing::info!(block_id = ?block_id, duplicate_block_id = ?duplicate_id, "duplicated block subtree");
                state.editors.ensure_subtree(&state.store, &duplicate_id);
                state.overflow_open_for = None;
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after duplicating subtree");
                }
            }
            Task::none()
        }
        | Message::ArchiveBlock(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            if let Some(removed_ids) = state.store.remove_block_subtree(&block_id) {
                tracing::info!(block_id = ?block_id, removed = removed_ids.len(), "archived block subtree");
                state.editors.remove_blocks(&removed_ids);
                for id in &removed_ids {
                    state.pending_reduce_signatures.remove(*id);
                    state.pending_expand_signatures.remove(*id);
                    state.reduce_states.remove(*id);
                    state.expand_states.remove(*id);
                }
                if removed_ids.iter().any(|id| Some(*id) == state.focused_block_id) {
                    state.focused_block_id = None;
                }
                // Ensure editor buffers exist for any roots created by removal
                // (e.g. when the last root is archived, a fresh empty root is inserted).
                for root_id in state.store.roots() {
                    state.editors.ensure_block(&state.store, root_id);
                }
                state.overflow_open_for = None;
                if state.active_block_id == Some(block_id) {
                    state.active_block_id = state.store.roots().first().copied();
                }
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after archiving subtree");
                }
            }
            Task::none()
        }
        | Message::ToggleFold(block_id) => {
            if !state.collapsed.remove(&block_id) {
                state.collapsed.insert(block_id);
            }
            Task::none()
        }
        | Message::ExpandMount(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            let base_dir = AppPaths::data_dir().unwrap_or_default();
            match state.store.expand_mount(&block_id, &base_dir) {
                | Ok(new_roots) => {
                    tracing::info!(block_id = ?block_id, children = new_roots.len(), "expanded mount");
                    for &id in &new_roots {
                        state.editors.ensure_subtree(&state.store, &id);
                    }
                    if let Err(err) = state.save_tree() {
                        tracing::error!(%err, "failed to save tree after expanding mount");
                    }
                }
                | Err(err) => {
                    tracing::error!(block_id = ?block_id, %err, "failed to expand mount");
                    state.error = Some(AppError::Mount(UiError::from_message(&err)));
                }
            }
            Task::none()
        }
        | Message::CollapseMount(block_id) => {
            state.set_active_block(&block_id);
            state.snapshot_for_undo();
            if let Some(()) = state.store.collapse_mount(&block_id) {
                tracing::info!(block_id = ?block_id, "collapsed mount");
                state.editors = EditorStore::from_store(&state.store);
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after collapsing mount");
                }
            }
            Task::none()
        }
        | Message::SaveToFile(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title("Save block to file")
                        .add_filter("JSON", &["json"])
                        .save_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| Message::SaveToFilePicked(block_id, path),
            )
        }
        | Message::SaveToFilePicked(block_id, path) => {
            if let Some(path) = path {
                state.snapshot_for_undo();
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                match state.store.save_subtree_to_file(&block_id, &path, &base_dir) {
                    | Ok(()) => {
                        tracing::info!(block_id = ?block_id, path = %path.display(), "saved subtree to file");
                        // Immediately expand the mount so the user sees no disruption.
                        match state.store.expand_mount(&block_id, &base_dir) {
                            | Ok(new_roots) => {
                                for &id in &new_roots {
                                    state.editors.ensure_subtree(&state.store, &id);
                                }
                            }
                            | Err(err) => {
                                tracing::error!(block_id = ?block_id, %err, "failed to re-expand after save-to-file");
                                state.error = Some(AppError::Mount(UiError::from_message(&err)));
                            }
                        }
                        if let Err(err) = state.save_tree() {
                            tracing::error!(%err, "failed to save tree after save-to-file");
                        }
                    }
                    | Err(err) => {
                        tracing::error!(block_id = ?block_id, %err, "failed to save subtree to file");
                        state.error = Some(AppError::Mount(UiError::from_message(&err)));
                    }
                }
            }
            Task::none()
        }
        | Message::LoadFromFile(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title("Load block from file")
                        .add_filter("JSON", &["json"])
                        .pick_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| Message::LoadFromFilePicked(block_id, path),
            )
        }
        | Message::SystemThemeChanged(mode) => {
            let dark = matches!(mode, Mode::Dark);
            if state.is_dark != dark {
                tracing::info!(is_dark = dark, "system theme changed");
                state.is_dark = dark;
            }
            Task::none()
        }
        | Message::LoadFromFilePicked(block_id, path) => {
            if let Some(path) = path {
                state.snapshot_for_undo();
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                let rel_path = path
                    .strip_prefix(&base_dir)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| path.clone());
                if state.store.set_mount_path(&block_id, rel_path).is_none() {
                    tracing::error!(block_id = ?block_id, "block has children or does not exist; cannot load");
                    return Task::none();
                }
                match state.store.expand_mount(&block_id, &base_dir) {
                    | Ok(new_roots) => {
                        tracing::info!(block_id = ?block_id, path = %path.display(), children = new_roots.len(), "loaded file into block");
                        for &id in &new_roots {
                            state.editors.ensure_subtree(&state.store, &id);
                        }
                    }
                    | Err(err) => {
                        tracing::error!(block_id = ?block_id, %err, "failed to expand after load-from-file");
                        state.error = Some(AppError::Mount(UiError::from_message(&err)));
                    }
                }
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after load-from-file");
                }
            }
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::llm;
    use crate::store::BlockStore;
    use crate::undo::UndoHistory;
    use slotmap::SecondaryMap;
    use std::collections::HashSet;

    fn test_state() -> (AppState, crate::store::BlockId) {
        let store = BlockStore::default();
        let root = *store.roots().first().expect("default store has a root");
        let state = AppState {
            editors: super::EditorStore::from_store(&store),
            store,
            undo_history: UndoHistory::with_capacity(64),
            llm_config: Ok(llm::LlmConfig::default()),
            error: None,
            reduce_states: SecondaryMap::new(),
            expand_states: SecondaryMap::new(),
            pending_reduce_signatures: SecondaryMap::new(),
            pending_expand_signatures: SecondaryMap::new(),
            expansion_drafts: SecondaryMap::new(),
            reduction_drafts: SecondaryMap::new(),
            overflow_open_for: None,
            active_block_id: None,
            focused_block_id: None,
            editing_block_id: None,
            collapsed: HashSet::new(),
            is_dark: false,
        };
        (state, root)
    }

    #[test]
    fn response_is_stale_after_point_change() {
        let (mut state, root) = test_state();
        let request_signature = state.lineage_signature(&root).expect("root has lineage");
        state.store.update_point(&root, "changed".to_string());
        assert!(state.is_stale_response(&root, request_signature));
    }

    #[test]
    fn response_is_not_stale_without_point_change() {
        let (state, root) = test_state();
        let request_signature = state.lineage_signature(&root).expect("root has lineage");
        assert!(!state.is_stale_response(&root, request_signature));
    }

    #[test]
    fn request_signature_changes_when_lineage_changes() {
        let (mut state, root) = test_state();
        let child =
            state.store.append_child(&root, "child".to_string()).expect("append child succeeds");
        let before = state.lineage_signature(&child).expect("child has lineage");
        state.store.update_point(&root, "root changed".to_string());
        let after = state.lineage_signature(&child).expect("child has lineage");
        assert_ne!(before, after);
    }
}

/// Global event subscription: keyboard shortcuts, mouse clicks, escape,
/// and system theme changes.
pub fn subscription(_state: &AppState) -> Subscription<Message> {
    Subscription::batch([
        event::listen_with(handle_event),
        system::theme_changes().map(Message::SystemThemeChanged),
    ])
}

fn handle_event(event: Event, status: event::Status, _window: iced::window::Id) -> Option<Message> {
    if status == event::Status::Captured {
        return None;
    }

    match event {
        | Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
            if key == keyboard::Key::Named(keyboard::key::Named::Escape) =>
        {
            Some(Message::CloseOverflow)
        }
        | Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            if modifiers.command() {
                match &key {
                    | keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") => {
                        return if modifiers.shift() {
                            Some(Message::Redo)
                        } else {
                            Some(Message::Undo)
                        };
                    }
                    | _ => {}
                }
            }
            shortcut_to_action(key, modifiers).map(Message::Shortcut)
        }
        | Event::Mouse(mouse::Event::ButtonPressed(_)) => Some(Message::CloseOverflow),
        | _ => None,
    }
}

fn run_shortcut_for_block(
    state: &mut AppState, block_id: BlockId, action_id: ActionId,
) -> Task<Message> {
    state.set_active_block(&block_id);

    let point_text =
        state.editors.get(&block_id).map(text_editor::Content::text).unwrap_or_default();
    let draft = state.expansion_drafts.get(block_id);
    let row_context = RowContext {
        block_id,
        point_text,
        has_draft: draft.is_some(),
        draft_suggestion_count: draft.map(|d| d.children.len()).unwrap_or(0),
        has_expand_error: state
            .expand_states
            .get(block_id)
            .is_some_and(|s| matches!(s, ExpandState::Error { .. })),
        has_reduce_error: state
            .reduce_states
            .get(block_id)
            .is_some_and(|s| matches!(s, ReduceState::Error { .. })),
        is_expanding: state.is_expanding(&block_id),
        is_reducing: state.is_reducing(&block_id),
        is_mounted: state.store.mount_table().entry(block_id).is_some(),
        has_children: !state.store.children(&block_id).is_empty(),
        is_unexpanded_mount: state.store.node(&block_id).is_some_and(|n| n.mount_path().is_some()),
    };
    let vm = project_for_viewport(build_action_bar_vm(&row_context), ViewportBucket::Wide);

    let is_enabled = vm
        .primary
        .iter()
        .chain(vm.contextual.iter())
        .chain(vm.overflow.iter())
        .find(|item| item.id == action_id)
        .is_some_and(|descriptor| descriptor.availability == ActionAvailability::Enabled);

    if is_enabled {
        if let Some(next) = action_to_message_by_id(state, &block_id, action_id) {
            return update(state, next);
        }
    }

    Task::none()
}

/// Top-level view: error banner + scrollable block tree.
pub fn view(state: &AppState) -> Element<'_, Message> {
    let mut layout = column![].spacing(theme::LAYOUT_GAP);
    if let Some(error) = &state.error {
        layout = layout.push(
            container(text(format!("Error: {}", error.message())))
                .style(theme::error_banner)
                .padding(theme::BANNER_PAD),
        );
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
