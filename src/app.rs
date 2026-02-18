//! Application state, messages, update and view for the iced UI.
//!
//! The underlying document is a graph of blocks (each with an id); the UI presents
//! the same content as a tree (roots and ordered children per node).

use crate::llm;
use iced::widget::{button, column, container, row, scrollable, text, text_editor};
use iced::{Element, Fill, Length, Task};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::PathBuf, sync::LazyLock};
use uuid::Uuid;

fn path_key(path: &[usize]) -> String {
    path.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("_")
}

static PROJECT_DIRS: LazyLock<Option<directories::ProjectDirs>> =
    LazyLock::new(|| directories::ProjectDirs::from("app", "miorin", "bb"));

/// Unique id for a block; used to refer to blocks in the graph.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(String);

impl BlockId {
    fn new() -> Self {
        Self(Uuid::new_v4().to_string())
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
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
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

    fn node_mut(&mut self, id: &BlockId) -> Option<&mut BlockNode> {
        self.nodes.get_mut(id)
    }

    /// Resolve a UI path (indices from root) to a block id.
    fn block_id_at_path(&self, path: &[usize]) -> Option<BlockId> {
        let mut ids: &[BlockId] = &self.roots;
        let mut out_id = None;
        for &index in path {
            let id = ids.get(index)?.clone();
            out_id = Some(id.clone());
            ids = self.node(&id).map(|n| n.children.as_slice()).unwrap_or(&[]);
        }
        out_id
    }

    fn update_point(&mut self, id: &BlockId, value: String) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.point = value;
        }
    }

    fn point_at_path(&self, path: &[usize]) -> Option<String> {
        self.block_id_at_path(path).and_then(|id| self.node(&id).map(|n| n.point.clone()))
    }

    fn lineage_points(&self, path: &[usize]) -> llm::Lineage {
        let mut lineage = Vec::new();
        let mut ids: &[BlockId] = &self.roots;
        for &index in path {
            let Some(id) = ids.get(index) else {
                break;
            };
            if let Some(node) = self.node(id) {
                lineage.push(node.point.clone());
                ids = node.children.as_slice();
            } else {
                break;
            }
        }
        llm::Lineage::from_points(lineage)
    }

    fn default_graph() -> Self {
        let root_id = BlockId::new();
        let child_ids = [
            BlockId::new(),
            BlockId::new(),
            BlockId::new(),
        ];
        let mut nodes = HashMap::new();
        nodes.insert(
            child_ids[0].clone(),
            BlockNode::new("马克思：《资本论》", vec![]),
        );
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

#[derive(Clone)]
pub struct AppState {
    graph: BlockGraph,
    llm_config: Result<llm::LlmConfig, llm::LlmConfigError>,
    error_message: Option<String>,
    summary_state: SummaryState,
    /// Editor content per block path (key from path_key). Kept in sync with graph.
    editor_contents: HashMap<String, text_editor::Content>,
}

#[derive(Clone, PartialEq)]
enum SummaryState {
    Idle,
    Loading(Vec<usize>),
    Error(String),
}

impl Default for SummaryState {
    fn default() -> Self {
        Self::Idle
    }
}

impl AppState {
    pub fn load() -> Self {
        let llm_config = llm::LlmConfig::load();
        let error_message = llm_config.as_ref().err().map(|err| err.to_string());
        let graph = BlockGraph::load();
        let editor_contents = Self::build_editor_contents(&graph, &graph.roots, &mut vec![]);
        Self {
            graph,
            llm_config,
            error_message,
            summary_state: SummaryState::Idle,
            editor_contents,
        }
    }

    fn build_editor_contents(
        graph: &BlockGraph, ids: &[BlockId], path: &mut Vec<usize>,
    ) -> HashMap<String, text_editor::Content> {
        let mut out = HashMap::new();
        for (i, id) in ids.iter().enumerate() {
            path.push(i);
            if let Some(node) = graph.node(id) {
                out.insert(path_key(path), text_editor::Content::with_text(&node.point));
                out.extend(Self::build_editor_contents(graph, &node.children, path));
            }
            path.pop();
        }
        out
    }

    fn save_tree(&self) -> io::Result<()> {
        self.graph.save()
    }

    fn is_summarizing(&self, path: &[usize]) -> bool {
        matches!(&self.summary_state, SummaryState::Loading(p) if p == path)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    PointEdited(Vec<usize>, text_editor::Action),
    Summarize(Vec<usize>),
    SummarizeDone(Vec<usize>, Result<String, String>),
}

pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    match message {
        | Message::PointEdited(path, action) => {
            let key = path_key(&path);
            if !state.editor_contents.contains_key(&key) {
                let point = state.graph.point_at_path(&path).unwrap_or_default();
                state.editor_contents.insert(key.clone(), text_editor::Content::with_text(&point));
            }
            if let Some(content) = state.editor_contents.get_mut(&key) {
                content.perform(action);
                if let Some(id) = state.graph.block_id_at_path(&path) {
                    state.graph.update_point(&id, content.text());
                }
                let _ = state.save_tree();
            }
            Task::none()
        }
        | Message::Summarize(path) => {
            if state.is_summarizing(&path) {
                return Task::none();
            }
            let lineage = state.graph.lineage_points(&path);
            let config = match &state.llm_config {
                | Ok(c) => c.clone(),
                | Err(e) => {
                    let msg = e.to_string();
                    state.error_message = Some(msg.clone());
                    state.summary_state = SummaryState::Error(msg);
                    return Task::none();
                }
            };
            state.summary_state = SummaryState::Loading(path.clone());
            let path_done = path.clone();
            Task::perform(
                async move {
                    let client = llm::LlmClient::new(config);
                    client.summarize_lineage(&lineage).await.map_err(|e| e.to_string())
                },
                move |result| Message::SummarizeDone(path_done, result),
            )
        }
        | Message::SummarizeDone(path, result) => {
            state.summary_state = SummaryState::Idle;
            match result {
                | Ok(summary) => {
                    if let Some(id) = state.graph.block_id_at_path(&path) {
                        state.graph.update_point(&id, summary.clone());
                    }
                    state
                        .editor_contents
                        .insert(path_key(&path), text_editor::Content::with_text(&summary));
                    let _ = state.save_tree();
                }
                | Err(e) => {
                    tracing::error!("llm summarize error: {}", e);
                    state.error_message = Some(e.clone());
                    state.summary_state = SummaryState::Error(e);
                }
            }
            Task::none()
        }
    }
}

pub fn view(state: &AppState) -> Element<'_, Message> {
    let mut col = column![].spacing(8);
    if let Some(msg) = &state.error_message {
        col = col
            .push(container(text(format!("Error: {}", msg))).style(container::danger).padding(8));
    }
    let content = view_line(state, &state.graph.roots, vec![]);
    col = col
        .push(scrollable(container(content).padding(16).width(Fill).center_x(Fill)).height(Fill));
    container(col).width(Fill).height(Fill).into()
}

fn view_line<'a>(
    state: &'a AppState, ids: &'a [BlockId], path: Vec<usize>,
) -> Element<'a, Message> {
    let mut col = column![].spacing(4);
    for (index, id) in ids.iter().enumerate() {
        let Some(node) = state.graph.node(id) else {
            continue;
        };
        let mut next_path = path.clone();
        next_path.push(index);
        col = col.push(view_block(state, id, node, next_path));
    }
    col.into()
}

fn view_block<'a>(
    state: &'a AppState, _id: &'a BlockId, node: &'a BlockNode, path: Vec<usize>,
) -> Element<'a, Message> {
    let path_for_edit = path.clone();
    let path_for_summarize = path.clone();
    let _summarizing = state.is_summarizing(&path);
    let summary_label = match &state.summary_state {
        | SummaryState::Loading(p) if p == &path => "Summarizing...",
        | SummaryState::Error(e) if state.error_message.as_deref() == Some(e.as_str()) => {
            "Summary failed"
        }
        | _ => "Summarize",
    };
    let key = path_key(&path);
    let content = state.editor_contents.get(&key).expect("editor content built at load");
    let path_for_edit2 = path_for_edit.clone();
    let row_content = row![]
        .spacing(8)
        .push(
            container(
                text_editor(content)
                    .placeholder("point")
                    .on_action(move |action| Message::PointEdited(path_for_edit2.clone(), action))
                    .height(Length::Shrink),
            )
            .width(Length::Fill),
        )
        .push(button(summary_label).on_press(Message::Summarize(path_for_summarize)));
    let mut col = column![].spacing(4).push(row_content);
    if !node.children.is_empty() {
        col = col.push(
            container(view_line(state, &node.children, path.clone()))
                .padding(iced::Padding::from([0.0, 24.0])),
        );
    }
    col.into()
}
