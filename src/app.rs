//! Application state, messages, update and view for the iced UI.

use crate::llm;
use iced::widget::{button, column, container, row, scrollable, text, text_editor};
use iced::{Element, Fill, Length, Task};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::PathBuf, sync::LazyLock};

fn path_key(path: &[usize]) -> String {
    path.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("_")
}

static PROJECT_DIRS: LazyLock<Option<directories::ProjectDirs>> =
    LazyLock::new(|| directories::ProjectDirs::from("app", "miorin", "bb"));

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct BlockForest {
    blocks: Vec<BlockData>,
}

impl BlockForest {
    fn new(blocks: Vec<BlockData>) -> Self {
        Self { blocks }
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

    fn update_point(&mut self, path: &[usize], value: String) {
        Self::update_point_in(&mut self.blocks, path, value);
    }

    fn update_point_in(tree: &mut [BlockData], path: &[usize], value: String) {
        if path.is_empty() {
            return;
        }
        let Some((head, tail)) = path.split_first() else {
            return;
        };
        let Some(node) = tree.get_mut(*head) else {
            return;
        };
        if tail.is_empty() {
            node.point = value;
            return;
        }
        Self::update_point_in(&mut node.children, tail, value);
    }

    fn point_at_path(&self, path: &[usize]) -> Option<String> {
        let mut cursor = self.blocks.as_slice();
        let mut node = None;
        for index in path {
            node = cursor.get(*index);
            cursor = node.as_ref().map(|n| n.children.as_slice()).unwrap_or(&[]);
        }
        node.map(|n| n.point.clone())
    }

    fn lineage_points(&self, path: &[usize]) -> llm::Lineage {
        let mut lineage = Vec::new();
        let mut cursor = self.blocks.as_slice();
        for index in path {
            let Some(node) = cursor.get(*index) else {
                break;
            };
            lineage.push(node.point.clone());
            cursor = node.children.as_slice();
        }
        llm::Lineage::from_points(lineage)
    }

    fn data_file_path() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|project| project.data_dir().join("blocks.json"))
    }

    fn default_blocks() -> Vec<BlockData> {
        vec![BlockData::new(
            "Notes on liberating productivity",
            true,
            vec![
                BlockData::new("马克思：《资本论》", false, vec![]),
                BlockData::new("马克思·韦伯：《新教伦理与资本主义精神》", false, vec![]),
                BlockData::new("Ivan Zhao: Steam, Steel, and Invisible Minds", false, vec![]),
            ],
        )]
    }
}

impl Default for BlockForest {
    fn default() -> Self {
        Self::new(Self::default_blocks())
    }
}

#[derive(Clone)]
pub struct AppState {
    tree: BlockForest,
    llm_config: Result<llm::LlmConfig, llm::LlmConfigError>,
    error_message: Option<String>,
    summary_state: SummaryState,
    /// Editor content per block path (key from path_key). Kept in sync with tree.point.
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
        let tree = BlockForest::load();
        let editor_contents = Self::build_editor_contents(&tree.blocks, &mut vec![]);
        Self {
            tree,
            llm_config,
            error_message,
            summary_state: SummaryState::Idle,
            editor_contents,
        }
    }

    fn build_editor_contents(blocks: &[BlockData], path: &mut Vec<usize>) -> HashMap<String, text_editor::Content> {
        let mut out = HashMap::new();
        for (i, block) in blocks.iter().enumerate() {
            path.push(i);
            out.insert(path_key(path), text_editor::Content::with_text(&block.point));
            out.extend(Self::build_editor_contents(&block.children, path));
            path.pop();
        }
        out
    }

    fn save_tree(&self) -> io::Result<()> {
        self.tree.save()
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
                let point = state.tree.point_at_path(&path).unwrap_or_default();
                state.editor_contents.insert(key.clone(), text_editor::Content::with_text(&point));
            }
            if let Some(content) = state.editor_contents.get_mut(&key) {
                content.perform(action);
                state.tree.update_point(&path, content.text());
                let _ = state.save_tree();
            }
            Task::none()
        }
        | Message::Summarize(path) => {
            if state.is_summarizing(&path) {
                return Task::none();
            }
            let lineage = state.tree.lineage_points(&path);
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
                    state.tree.update_point(&path, summary.clone());
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
    let content = view_line(state, &state.tree.blocks, vec![]);
    col = col
        .push(scrollable(container(content).padding(16).width(Fill).center_x(Fill)).height(Fill));
    container(col).width(Fill).height(Fill).into()
}

fn view_line<'a>(
    state: &'a AppState, blocks: &'a [BlockData], path: Vec<usize>,
) -> Element<'a, Message> {
    let mut col = column![].spacing(4);
    for (index, block) in blocks.iter().enumerate() {
        let mut next_path = path.clone();
        next_path.push(index);
        col = col.push(view_block(state, block, next_path));
    }
    col.into()
}

fn view_block<'a>(
    state: &'a AppState, block: &'a BlockData, path: Vec<usize>,
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
    let content = state
        .editor_contents
        .get(&key)
        .expect("editor content built at load");
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
    if !block.children.is_empty() {
        col = col.push(
            container(view_line(state, &block.children, path.clone()))
                .padding(iced::Padding::from([0.0, 24.0])),
        );
    }
    col.into()
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct BlockData {
    point: String,
    children: Vec<BlockData>,
    is_root: bool,
}

impl BlockData {
    fn new(point: impl ToString, is_root: bool, children: Vec<BlockData>) -> Self {
        Self { point: point.to_string(), children, is_root }
    }
}
