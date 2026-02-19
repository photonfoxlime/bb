//! Application state, messages, update, and view for the iced UI.
//!
//! The underlying document is a graph of blocks (each with a UUID id); the UI presents
//! the same content as a tree (roots and ordered children per node).

use crate::llm;
use crate::theme;
mod action_bar;

use action_bar::{
    ActionAvailability, ActionDescriptor, ActionId, RowContext, StatusChipVm, ViewportBucket,
    action_to_message, action_to_message_by_id, build_action_bar_vm, project_for_viewport,
    shortcut_to_action,
};
use iced::widget::{button, column, container, row, rule, scrollable, text, text_editor};
use iced::{Element, Event, Fill, Length, Subscription, Task, event, keyboard, mouse};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::PathBuf, sync::LazyLock};
use uuid::Uuid;

static PROJECT_DIRS: LazyLock<Option<directories::ProjectDirs>> =
    LazyLock::new(|| directories::ProjectDirs::from("app", "miorin", "bb"));

/// Unique id for a block in the graph.
///
/// Invariant: this id is always a valid UUID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(Uuid);

impl BlockId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// One node in the block graph: a point (text) and ordered child ids.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct BlockNode {
    point: String,
    children: Vec<BlockId>,
}

impl BlockNode {
    fn new(point: impl ToString, children: Vec<BlockId>) -> Self {
        Self { point: point.to_string(), children }
    }
}

/// Graph representation: roots and a map from block id to node.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct BlockGraph {
    roots: Vec<BlockId>,
    nodes: HashMap<BlockId, BlockNode>,
}

impl BlockGraph {
    fn new(roots: Vec<BlockId>, nodes: HashMap<BlockId, BlockNode>) -> Self {
        Self { roots, nodes }
    }

    fn load() -> Self {
        let Some(path) = Self::data_file_path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            | Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            | Err(_) => Self::default(),
        }
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = Self::data_file_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, contents)
    }

    fn data_file_path() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|project| project.data_dir().join("blocks.json"))
    }

    fn node(&self, id: &BlockId) -> Option<&BlockNode> {
        self.nodes.get(id)
    }

    fn point(&self, id: &BlockId) -> Option<String> {
        self.node(id).map(|node| node.point.clone())
    }

    fn update_point(&mut self, id: &BlockId, value: String) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.point = value;
        }
    }

    /// Add one child block under the parent and return the new child id.
    fn append_child(&mut self, parent_id: &BlockId, point: String) -> Option<BlockId> {
        if !self.nodes.contains_key(parent_id) {
            return None;
        }

        let child_id = BlockId::new();
        self.nodes.insert(child_id.clone(), BlockNode::new(point, vec![]));
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            parent.children.push(child_id.clone());
        }
        Some(child_id)
    }

    fn append_sibling(&mut self, block_id: &BlockId, point: String) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let sibling_id = BlockId::new();
        self.nodes.insert(sibling_id.clone(), BlockNode::new(point, vec![]));

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            parent.children.insert(index + 1, sibling_id.clone());
        } else {
            self.roots.insert(index + 1, sibling_id.clone());
        }
        Some(sibling_id)
    }

    fn duplicate_subtree_after(&mut self, block_id: &BlockId) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let duplicate_id = self.clone_subtree_with_new_ids(block_id)?;

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            parent.children.insert(index + 1, duplicate_id.clone());
        } else {
            self.roots.insert(index + 1, duplicate_id.clone());
        }
        Some(duplicate_id)
    }

    fn remove_block_subtree(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.children.remove(index);
            }
        } else {
            self.roots.remove(index);
        }

        let mut removed_ids = Vec::new();
        self.collect_subtree_ids(block_id, &mut removed_ids);
        for id in &removed_ids {
            self.nodes.remove(id);
        }

        if self.roots.is_empty() {
            let root_id = BlockId::new();
            self.nodes.insert(root_id.clone(), BlockNode::new(String::new(), vec![]));
            self.roots.push(root_id);
        }

        Some(removed_ids)
    }

    fn parent_and_index_of(&self, target: &BlockId) -> Option<(Option<BlockId>, usize)> {
        if let Some(index) = self.roots.iter().position(|id| id == target) {
            return Some((None, index));
        }

        for (parent_id, node) in &self.nodes {
            if let Some(index) = node.children.iter().position(|id| id == target) {
                return Some((Some(parent_id.clone()), index));
            }
        }
        None
    }

    fn clone_subtree_with_new_ids(&mut self, source_id: &BlockId) -> Option<BlockId> {
        let source_node = self.node(source_id)?.clone();
        let mut child_ids = Vec::with_capacity(source_node.children.len());
        for child in source_node.children {
            child_ids.push(self.clone_subtree_with_new_ids(&child)?);
        }

        let next_id = BlockId::new();
        self.nodes.insert(next_id.clone(), BlockNode::new(source_node.point, child_ids));
        Some(next_id)
    }

    fn collect_subtree_ids(&self, current: &BlockId, out: &mut Vec<BlockId>) {
        let Some(node) = self.node(current) else {
            return;
        };
        out.push(current.clone());
        for child in &node.children {
            self.collect_subtree_ids(child, out);
        }
    }

    /// Return lineage points from one root to the target id.
    ///
    /// If the target id is not reachable from any root, returns an empty lineage.
    fn lineage_points_for_id(&self, target: &BlockId) -> llm::Lineage {
        for root in &self.roots {
            let mut points = Vec::new();
            if self.collect_lineage_points(root, target, &mut points) {
                return llm::Lineage::from_points(points);
            }
        }
        llm::Lineage::from_points(vec![])
    }

    /// DFS helper for `lineage_points_for_id`.
    fn collect_lineage_points(
        &self, current: &BlockId, target: &BlockId, points: &mut Vec<String>,
    ) -> bool {
        let Some(node) = self.node(current) else {
            return false;
        };

        points.push(node.point.clone());
        if current == target {
            return true;
        }

        for child in &node.children {
            if self.collect_lineage_points(child, target, points) {
                return true;
            }
        }

        points.pop();
        false
    }

    fn default_graph() -> Self {
        let root_id = BlockId::new();
        let child_ids = [BlockId::new(), BlockId::new(), BlockId::new()];
        let mut nodes = HashMap::new();
        nodes.insert(child_ids[0].clone(), BlockNode::new("马克思：《资本论》", vec![]));
        nodes.insert(
            child_ids[1].clone(),
            BlockNode::new("马克思·韦伯：《新教伦理与资本主义精神》", vec![]),
        );
        nodes.insert(
            child_ids[2].clone(),
            BlockNode::new("Ivan Zhao: Steam, Steel, and Invisible Minds", vec![]),
        );
        nodes.insert(
            root_id.clone(),
            BlockNode::new("Notes on liberating productivity", child_ids.to_vec()),
        );
        BlockGraph::new(vec![root_id], nodes)
    }
}

impl Default for BlockGraph {
    fn default() -> Self {
        Self::default_graph()
    }
}

/// A display-safe error value used by UI state and messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError {
    message: String,
}

impl UiError {
    fn from_message(message: impl ToString) -> Self {
        Self { message: message.to_string() }
    }

    fn as_str(&self) -> &str {
        self.message.as_str()
    }
}

/// Global application-level error source used by the top banner.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AppError {
    Configuration(UiError),
    Summary(UiError),
    Expand(UiError),
}

impl AppError {
    fn message(&self) -> &str {
        match self {
            | Self::Configuration(err) | Self::Summary(err) | Self::Expand(err) => err.as_str(),
        }
    }
}

/// Per-row summarize lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SummaryState {
    Idle,
    Loading(BlockId),
    Error { block_id: BlockId, reason: UiError },
}

impl Default for SummaryState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Per-row expand lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpandState {
    Idle,
    Loading(BlockId),
    Error { block_id: BlockId, reason: UiError },
}

impl Default for ExpandState {
    fn default() -> Self {
        Self::Idle
    }
}

/// One pending expansion draft for one block.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpansionDraft {
    rewrite: Option<String>,
    children: Vec<String>,
}

impl ExpansionDraft {
    fn new(rewrite: Option<String>, children: Vec<String>) -> Self {
        Self { rewrite, children }
    }

    fn from_expand_result(result: llm::ExpandResult) -> Self {
        let (rewrite, children) = result.into_parts();
        let children =
            children.into_iter().map(llm::ExpandSuggestion::into_point).collect::<Vec<_>>();
        Self::new(rewrite, children)
    }

    fn is_empty(&self) -> bool {
        self.rewrite.is_none() && self.children.is_empty()
    }
}

/// Stores text editor buffers indexed by block id.
///
/// Invariant: every visible block id should have one content entry.
#[derive(Clone, Default)]
struct EditorStore {
    buffers: HashMap<BlockId, text_editor::Content>,
}

impl EditorStore {
    fn from_graph(graph: &BlockGraph) -> Self {
        let mut store = Self::default();
        store.populate(graph, &graph.roots);
        store
    }

    fn populate(&mut self, graph: &BlockGraph, ids: &[BlockId]) {
        for id in ids {
            if let Some(node) = graph.node(id) {
                self.buffers.insert(id.clone(), text_editor::Content::with_text(&node.point));
                self.populate(graph, &node.children);
            }
        }
    }

    fn ensure_block(&mut self, graph: &BlockGraph, block_id: &BlockId) {
        if self.buffers.contains_key(block_id) {
            return;
        }
        let point = graph.point(block_id).unwrap_or_default();
        self.buffers.insert(block_id.clone(), text_editor::Content::with_text(&point));
    }

    fn get(&self, block_id: &BlockId) -> Option<&text_editor::Content> {
        self.buffers.get(block_id)
    }

    fn get_mut(&mut self, block_id: &BlockId) -> Option<&mut text_editor::Content> {
        self.buffers.get_mut(block_id)
    }

    fn set_text(&mut self, block_id: &BlockId, value: &str) {
        self.buffers.insert(block_id.clone(), text_editor::Content::with_text(value));
    }

    fn ensure_subtree(&mut self, graph: &BlockGraph, block_id: &BlockId) {
        if let Some(node) = graph.node(block_id) {
            self.buffers
                .entry(block_id.clone())
                .or_insert_with(|| text_editor::Content::with_text(&node.point));
            for child in &node.children {
                self.ensure_subtree(graph, child);
            }
        }
    }

    fn remove_blocks(&mut self, block_ids: &[BlockId]) {
        for id in block_ids {
            self.buffers.remove(id);
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    graph: BlockGraph,
    llm_config: Result<llm::LlmConfig, llm::LlmConfigError>,
    error: Option<AppError>,
    summary_state: SummaryState,
    expand_state: ExpandState,
    expansion_drafts: HashMap<BlockId, ExpansionDraft>,
    editors: EditorStore,
    overflow_open_for: Option<BlockId>,
    active_block_id: Option<BlockId>,
}

impl AppState {
    pub fn load() -> Self {
        let llm_config = llm::LlmConfig::load();
        let error = llm_config
            .as_ref()
            .err()
            .map(|err| AppError::Configuration(UiError::from_message(err)));
        let graph = BlockGraph::load();
        let editors = EditorStore::from_graph(&graph);
        Self {
            graph,
            llm_config,
            error,
            summary_state: SummaryState::Idle,
            expand_state: ExpandState::Idle,
            expansion_drafts: HashMap::new(),
            editors,
            overflow_open_for: None,
            active_block_id: None,
        }
    }

    fn save_tree(&self) -> io::Result<()> {
        self.graph.save()
    }

    fn is_summarizing(&self, block_id: &BlockId) -> bool {
        matches!(&self.summary_state, SummaryState::Loading(id) if id == block_id)
    }

    fn is_expanding(&self, block_id: &BlockId) -> bool {
        matches!(&self.expand_state, ExpandState::Loading(id) if id == block_id)
    }

    fn current_block_for_shortcuts(&self) -> Option<BlockId> {
        self.active_block_id.clone().or_else(|| self.graph.roots.first().cloned())
    }

    fn set_active_block(&mut self, block_id: &BlockId) {
        self.active_block_id = Some(block_id.clone());
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    PointEdited(BlockId, text_editor::Action),
    Shortcut(ActionId),
    Summarize(BlockId),
    SummarizeDone(BlockId, Result<String, UiError>),
    Expand(BlockId),
    ExpandDone(BlockId, Result<llm::ExpandResult, UiError>),
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
}

pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    match message {
        | Message::Shortcut(action_id) => {
            let Some(block_id) = state.current_block_for_shortcuts() else {
                return Task::none();
            };

            let point_text =
                state.editors.get(&block_id).map(text_editor::Content::text).unwrap_or_default();
            let draft = state.expansion_drafts.get(&block_id);
            let row_context = RowContext {
                block_id: block_id.clone(),
                point_text,
                has_draft: draft.is_some(),
                draft_suggestion_count: draft.map(|d| d.children.len()).unwrap_or(0),
                has_expand_error: matches!(&state.expand_state, ExpandState::Error { block_id: id, .. } if id == &block_id),
                has_reduce_error: matches!(&state.summary_state, SummaryState::Error { block_id: id, .. } if id == &block_id),
                is_expanding: state.is_expanding(&block_id),
                is_reducing: state.is_summarizing(&block_id),
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
        | Message::PointEdited(block_id, action) => {
            state.set_active_block(&block_id);
            state.editors.ensure_block(&state.graph, &block_id);
            if let Some(content) = state.editors.get_mut(&block_id) {
                content.perform(action);
                let next_text = content.text();
                tracing::debug!(block_id = ?block_id, chars = next_text.len(), "point edited");
                state.graph.update_point(&block_id, next_text);
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after edit");
                }
            }
            Task::none()
        }
        | Message::Summarize(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if state.is_summarizing(&block_id) {
                return Task::none();
            }
            let lineage = state.graph.lineage_points_for_id(&block_id);
            let config = match &state.llm_config {
                | Ok(config) => config.clone(),
                | Err(err) => {
                    let ui_err = UiError::from_message(err);
                    state.error = Some(AppError::Configuration(ui_err.clone()));
                    state.summary_state = SummaryState::Error { block_id, reason: ui_err };
                    return Task::none();
                }
            };
            tracing::info!(block_id = ?block_id, "summary request started");
            state.summary_state = SummaryState::Loading(block_id.clone());
            Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    client.summarize_lineage(&lineage).await.map_err(UiError::from_message)
                },
                move |result| Message::SummarizeDone(block_id, result),
            )
        }
        | Message::SummarizeDone(block_id, result) => {
            state.summary_state = SummaryState::Idle;
            match result {
                | Ok(summary) => {
                    tracing::info!(block_id = ?block_id, chars = summary.len(), "summary request succeeded");
                    state.graph.update_point(&block_id, summary.clone());
                    state.editors.set_text(&block_id, &summary);
                    if let Err(err) = state.save_tree() {
                        tracing::error!(%err, "failed to save tree after summarize");
                    }
                    state.error = None;
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "summary request failed");
                    state.summary_state = SummaryState::Error { block_id, reason: reason.clone() };
                    state.error = Some(AppError::Summary(reason));
                }
            }
            Task::none()
        }
        | Message::Expand(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if state.is_expanding(&block_id) {
                return Task::none();
            }
            let lineage = state.graph.lineage_points_for_id(&block_id);
            let config = match &state.llm_config {
                | Ok(config) => config.clone(),
                | Err(err) => {
                    let ui_err = UiError::from_message(err);
                    state.error = Some(AppError::Configuration(ui_err.clone()));
                    state.expand_state = ExpandState::Error { block_id, reason: ui_err };
                    return Task::none();
                }
            };

            tracing::info!(block_id = ?block_id, "expand request started");
            state.expand_state = ExpandState::Loading(block_id.clone());
            Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    client.expand_lineage(&lineage).await.map_err(UiError::from_message)
                },
                move |result| Message::ExpandDone(block_id, result),
            )
        }
        | Message::ExpandDone(block_id, result) => {
            state.expand_state = ExpandState::Idle;
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
                        state.expand_state =
                            ExpandState::Error { block_id, reason: reason.clone() };
                        state.error = Some(AppError::Expand(reason));
                        return Task::none();
                    }
                    state.expansion_drafts.insert(block_id, draft);
                    state.error = None;
                }
                | Err(reason) => {
                    tracing::error!(block_id = ?block_id, reason = %reason.as_str(), "expand request failed");
                    state.expand_state = ExpandState::Error { block_id, reason: reason.clone() };
                    state.error = Some(AppError::Expand(reason));
                }
            }
            Task::none()
        }
        | Message::ApplyExpandedRewrite(block_id) => {
            state.set_active_block(&block_id);
            let mut should_save = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.expansion_drafts.get_mut(&block_id) {
                if let Some(rewrite) = draft.rewrite.take() {
                    tracing::info!(block_id = ?block_id, chars = rewrite.len(), "applied expanded rewrite");
                    state.graph.update_point(&block_id, rewrite.clone());
                    state.editors.set_text(&block_id, &rewrite);
                    should_save = true;
                }
                if draft.is_empty() {
                    should_remove_draft = true;
                }
            }
            if should_remove_draft {
                state.expansion_drafts.remove(&block_id);
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
            if let Some(draft) = state.expansion_drafts.get_mut(&block_id) {
                draft.rewrite = None;
                tracing::info!(block_id = ?block_id, "rejected expanded rewrite");
                if draft.is_empty() {
                    state.expansion_drafts.remove(&block_id);
                }
            }
            Task::none()
        }
        | Message::AcceptExpandedChild(block_id, child_index) => {
            state.set_active_block(&block_id);
            let mut should_save = false;
            let mut should_remove_draft = false;
            if let Some(draft) = state.expansion_drafts.get_mut(&block_id) {
                if child_index < draft.children.len() {
                    let point = draft.children.remove(child_index);
                    if let Some(child_id) = state.graph.append_child(&block_id, point.clone()) {
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
                state.expansion_drafts.remove(&block_id);
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
            if let Some(draft) = state.expansion_drafts.get_mut(&block_id) {
                if child_index < draft.children.len() {
                    draft.children.remove(child_index);
                    tracing::info!(block_id = ?block_id, child_index, "rejected expanded child");
                }
                if draft.is_empty() {
                    state.expansion_drafts.remove(&block_id);
                }
            }
            Task::none()
        }
        | Message::AcceptAllExpandedChildren(block_id) => {
            state.set_active_block(&block_id);
            if let Some(mut draft) = state.expansion_drafts.remove(&block_id) {
                for point in draft.children.drain(..) {
                    if let Some(child_id) = state.graph.append_child(&block_id, point.clone()) {
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
                    state.expansion_drafts.insert(block_id.clone(), draft);
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
            state.expansion_drafts.remove(&block_id);
            Task::none()
        }
        | Message::ToggleOverflow(block_id) => {
            state.set_active_block(&block_id);
            if state.overflow_open_for.as_ref() == Some(&block_id) {
                state.overflow_open_for = None;
            } else {
                state.overflow_open_for = Some(block_id);
            }
            Task::none()
        }
        | Message::CloseOverflow => {
            state.overflow_open_for = None;
            Task::none()
        }
        | Message::AddChild(block_id) => {
            state.set_active_block(&block_id);
            state.overflow_open_for = None;
            if let Some(child_id) = state.graph.append_child(&block_id, String::new()) {
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
            if let Some(sibling_id) = state.graph.append_sibling(&block_id, String::new()) {
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
            if let Some(duplicate_id) = state.graph.duplicate_subtree_after(&block_id) {
                tracing::info!(block_id = ?block_id, duplicate_block_id = ?duplicate_id, "duplicated block subtree");
                state.editors.ensure_subtree(&state.graph, &duplicate_id);
                state.overflow_open_for = None;
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after duplicating subtree");
                }
            }
            Task::none()
        }
        | Message::ArchiveBlock(block_id) => {
            state.set_active_block(&block_id);
            if let Some(removed_ids) = state.graph.remove_block_subtree(&block_id) {
                tracing::info!(block_id = ?block_id, removed = removed_ids.len(), "archived block subtree");
                state.editors.remove_blocks(&removed_ids);
                state.overflow_open_for = None;
                if state.active_block_id.as_ref() == Some(&block_id) {
                    state.active_block_id = state.graph.roots.first().cloned();
                }
                if let Err(err) = state.save_tree() {
                    tracing::error!(%err, "failed to save tree after archiving subtree");
                }
            }
            Task::none()
        }
    }
}

pub fn subscription(_state: &AppState) -> Subscription<Message> {
    event::listen_with(handle_event)
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
            shortcut_to_action(key, modifiers).map(Message::Shortcut)
        }
        | Event::Mouse(mouse::Event::ButtonPressed(_)) => Some(Message::CloseOverflow),
        | _ => None,
    }
}

pub fn view(state: &AppState) -> Element<'_, Message> {
    let mut layout = column![].spacing(12);
    if let Some(error) = &state.error {
        layout = layout.push(
            container(text(format!("Error: {}", error.message())))
                .style(theme::error_banner)
                .padding(8),
        );
    }

    let tree = TreeView::new(state).render_roots();
    layout = layout
        .push(scrollable(container(tree).padding(24).max_width(900).center_x(Fill)).height(Fill));

    container(layout).style(theme::canvas).width(Fill).height(Fill).into()
}

/// Pure renderer from immutable state into tree widgets.
struct TreeView<'a> {
    state: &'a AppState,
}

impl<'a> TreeView<'a> {
    fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    fn render_roots(&self) -> Element<'a, Message> {
        self.render_line(&self.state.graph.roots)
    }

    fn render_line(&self, ids: &'a [BlockId]) -> Element<'a, Message> {
        let mut col = column![].spacing(10);
        for id in ids {
            let Some(node) = self.state.graph.node(id) else {
                continue;
            };
            col = col.push(self.render_block(id, node));
        }
        col.into()
    }

    fn render_block(&self, block_id: &BlockId, node: &'a BlockNode) -> Element<'a, Message> {
        let editor_content =
            self.state.editors.get(block_id).expect("editor content is populated from graph");

        let block_id_for_edit = block_id.clone();
        let row_context = self.action_row_context(block_id, editor_content.text(), node);
        let action_bar =
            project_for_viewport(build_action_bar_vm(&row_context), self.viewport_bucket());

        let spine = container(rule::vertical(1).style(theme::spine_rule))
            .width(Length::Fixed(4.0))
            .align_x(iced::alignment::Horizontal::Center);
        let marker = container(text("•").size(12).style(theme::spine_text))
            .width(Length::Fixed(12.0))
            .align_x(iced::alignment::Horizontal::Center);

        let row_content = row![]
            .spacing(6)
            .width(Fill)
            .align_y(iced::Alignment::Start)
            .push(spine)
            .push(marker)
            .push(
                text_editor(editor_content)
                    .placeholder("point")
                    .style(theme::point_editor)
                    .on_action(move |action| {
                        Message::PointEdited(block_id_for_edit.clone(), action)
                    })
                    .height(Length::Shrink),
            )
            .push(self.render_status_chip(&action_bar))
            .push(self.render_action_buttons(block_id, &action_bar));

        let mut block = column![].spacing(8).push(row_content);
        if let Some(draft) = self.state.expansion_drafts.get(block_id) {
            block = block.push(self.render_expansion_panel(block_id, draft));
        }

        if !node.children.is_empty() {
            block = block.push(
                container(self.render_line(&node.children)).padding(iced::Padding::ZERO.left(16.0)),
            );
        }
        block.into()
    }

    fn render_expansion_panel(
        &self, block_id: &BlockId, draft: &'a ExpansionDraft,
    ) -> Element<'a, Message> {
        let mut panel = column![].spacing(6);

        if let Some(rewrite) = &draft.rewrite {
            panel = panel.push(
                row![]
                    .spacing(8)
                    .push(container(text(format!("Rewrite: {}", rewrite))).width(Length::Fill))
                    .push(
                        button(text("Apply rewrite").font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .on_press(Message::ApplyExpandedRewrite(block_id.clone())),
                    )
                    .push(
                        button(text("Dismiss rewrite").font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .on_press(Message::RejectExpandedRewrite(block_id.clone())),
                    ),
            );
        }

        if !draft.children.is_empty() {
            panel = panel.push(
                row![]
                    .spacing(8)
                    .push(container(text("Child suggestions")).width(Length::Fill))
                    .push(
                        button(text("Accept all").font(theme::INTER).size(13))
                            .style(theme::action_button)
                            .on_press(Message::AcceptAllExpandedChildren(block_id.clone())),
                    )
                    .push(
                        button(text("Discard all").font(theme::INTER).size(13))
                            .style(theme::destructive_button)
                            .on_press(Message::DiscardExpansion(block_id.clone())),
                    ),
            );

            for (index, child) in draft.children.iter().enumerate() {
                panel = panel.push(
                    row![]
                        .spacing(8)
                        .push(container(text(child.as_str())).width(Length::Fill))
                        .push(
                            button(text("Keep").font(theme::INTER).size(13))
                                .style(theme::action_button)
                                .on_press(Message::AcceptExpandedChild(block_id.clone(), index)),
                        )
                        .push(
                            button(text("Drop").font(theme::INTER).size(13))
                                .style(theme::destructive_button)
                                .on_press(Message::RejectExpandedChild(block_id.clone(), index)),
                        ),
                );
            }
        }

        container(panel).padding(iced::Padding::from([8.0, 16.0])).style(theme::draft_panel).into()
    }

    fn action_row_context(
        &self, block_id: &BlockId, point_text: String, _node: &BlockNode,
    ) -> RowContext {
        let draft = self.state.expansion_drafts.get(block_id);
        RowContext {
            block_id: block_id.clone(),
            point_text,
            has_draft: draft.is_some(),
            draft_suggestion_count: draft.map(|d| d.children.len()).unwrap_or(0),
            has_expand_error: matches!(&self.state.expand_state, ExpandState::Error { block_id: id, .. } if id == block_id),
            has_reduce_error: matches!(&self.state.summary_state, SummaryState::Error { block_id: id, .. } if id == block_id),
            is_expanding: self.state.is_expanding(block_id),
            is_reducing: self.state.is_summarizing(block_id),
        }
    }

    fn viewport_bucket(&self) -> ViewportBucket {
        ViewportBucket::Wide
    }

    fn render_status_chip(&self, vm: &action_bar::ActionBarVm) -> Element<'a, Message> {
        let label = match &vm.status_chip {
            | Some(StatusChipVm::Loading { op: ActionId::Expand }) => {
                Some("Expanding...".to_string())
            }
            | Some(StatusChipVm::Loading { op: ActionId::Reduce }) => {
                Some("Summarizing...".to_string())
            }
            | Some(StatusChipVm::Loading { .. }) => Some("Working...".to_string()),
            | Some(StatusChipVm::Error { message, .. }) => Some(message.clone()),
            | Some(StatusChipVm::DraftActive { suggestion_count }) if *suggestion_count > 0 => {
                Some("Draft ready".to_string())
            }
            | Some(StatusChipVm::DraftActive { .. }) => Some("Draft".to_string()),
            | None => None,
        };

        let chip = match label {
            | Some(label) => {
                container(text(label).size(12).font(theme::INTER).style(theme::status_text))
                    .padding(iced::Padding::from([2.0, 8.0]))
            }
            | None => container(text(" ")).padding(iced::Padding::from([2.0, 8.0])),
        };
        chip.width(Length::Shrink).into()
    }

    fn render_action_buttons(
        &self, block_id: &BlockId, vm: &action_bar::ActionBarVm,
    ) -> Element<'a, Message> {
        let mut actions_row = row![].spacing(6);

        for descriptor in vm.visible_actions() {
            actions_row = actions_row.push(self.render_action_button(block_id, &descriptor));
        }

        if !vm.overflow.is_empty() {
            let is_open = self.state.overflow_open_for.as_ref() == Some(block_id);
            let label = if is_open { "Close" } else { "More" };
            actions_row = actions_row.push(
                button(text(label).font(theme::INTER).size(13))
                    .style(theme::action_button)
                    .on_press(Message::ToggleOverflow(block_id.clone())),
            );
        }

        let mut layout = column![].spacing(4).push(actions_row);
        if self.state.overflow_open_for.as_ref() == Some(block_id) {
            let mut overflow = row![].spacing(6);
            for descriptor in &vm.overflow {
                overflow = overflow.push(self.render_action_button(block_id, descriptor));
            }
            layout = layout.push(container(overflow).padding(iced::Padding::from([4.0, 0.0])));
        }

        layout.into()
    }

    fn render_action_button(
        &self, block_id: &BlockId, descriptor: &ActionDescriptor,
    ) -> Element<'a, Message> {
        let style = if descriptor.destructive {
            theme::destructive_button as fn(&iced::Theme, button::Status) -> button::Style
        } else {
            theme::action_button
        };
        let label = text(descriptor.label).font(theme::INTER).size(13);
        let base = button(label).style(style);
        let button = if descriptor.availability == ActionAvailability::Enabled {
            if let Some(message) = action_to_message(self.state, block_id, descriptor) {
                base.on_press(message)
            } else {
                base
            }
        } else {
            base
        };
        button.into()
    }
}
