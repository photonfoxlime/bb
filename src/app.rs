use crate::llm;
#[allow(unused)]
use dioxus::{logger::tracing, prelude::*};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf, sync::LazyLock};
use uuid::Uuid;

static APP_CSS: Asset = asset!("/assets/app.css");
static FONTS_CSS: Asset = asset!("/assets/fonts.css");
static TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");
static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from("app", "miorin", "bb"));

const _: Asset = asset!("/assets/fonts/Inter-300.woff2");
const _: Asset = asset!("/assets/fonts/Inter-400.woff2");
const _: Asset = asset!("/assets/fonts/Inter-500.woff2");
const _: Asset = asset!("/assets/fonts/LXGWWenKai-Light.ttf");
const _: Asset = asset!("/assets/fonts/LXGWWenKai-Regular.ttf");
const _: Asset = asset!("/assets/fonts/LXGWWenKai-Medium.ttf");

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
struct AppState {
    tree: BlockForest,
    llm_config: Result<llm::LlmConfig, llm::LlmConfigError>,
}

impl AppState {
    fn load() -> Self {
        Self { tree: BlockForest::load(), llm_config: llm::LlmConfig::load() }
    }

    fn save_tree(&self) -> io::Result<()> {
        self.tree.save()
    }
}

#[component]
pub fn App() -> Element {
    use_effect(|| {
        // dioxus::desktop::window().devtool(); // opens the webview devtools
    });

    let app_state = use_signal(AppState::load);

    {
        let app_state = app_state.clone();
        use_effect(move || {
            let snapshot = app_state.read().clone();
            let _ = snapshot.save_tree();
        });
    }

    let tree_snapshot = app_state.read().tree.clone();
    rsx! {
        document::Stylesheet { href: TAILWIND_CSS }
        document::Stylesheet { href: APP_CSS }
        document::Stylesheet { href: FONTS_CSS }
        main { class: "min-h-screen",
            div { class: "bb-canvas",
                Line { blocks: tree_snapshot.blocks, path: vec![], app_state }
            }
        }
    }
}

#[component]
fn Line(blocks: Vec<BlockData>, path: Vec<usize>, app_state: Signal<AppState>) -> Element {
    let items: Vec<(usize, BlockData, Vec<usize>)> = blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| {
            let mut next_path = path.clone();
            next_path.push(index);
            (index, block, next_path)
        })
        .collect();
    rsx! {
        section { class: "bb-line",
            ul { class: "bb-children",
                for (index, block, next_path) in items {
                    Block { key: "{index}", block, path: next_path, app_state }
                }
            }
        }
    }
}

#[component]
fn Block(block: BlockData, path: Vec<usize>, app_state: Signal<AppState>) -> Element {
    let BlockData { point, children, is_root } = block;
    let block_class = if is_root { "bb-block bb-block-root" } else { "bb-block" };
    let point_text = point.clone();

    let id = use_hook(|| format!("ta-{}", Uuid::new_v4()));
    let mut summary_state = use_signal(SummaryState::default);

    fn update_height(id: &str) {
        document::eval(&format!(
            r#"
            const ta = document.getElementById("{id}");
            if (ta) {{
              ta.style.height = "auto";
              ta.style.height = ta.scrollHeight + "px";
            }}
            "#
        ));
    }

    {
        let id = id.clone();
        use_effect(move || {
            // run once on mount
            update_height(&id);
        });
    }

    let path_for_children = path.clone();
    let path_for_input = path.clone();
    let path_for_summarize = path.clone();
    let id_for_input = id.clone();
    let id_for_summary = id.clone();
    let summarize_disabled = matches!(*summary_state.read(), SummaryState::Loading);
    let summarize_title = match &*summary_state.read() {
        | SummaryState::Idle => "Summarize this point".to_string(),
        | SummaryState::Loading => "Summarizing...".to_string(),
        | SummaryState::Error(message) => format!("Summary failed: {message}"),
    };

    let on_summarize = move |_| {
        if matches!(*summary_state.read(), SummaryState::Loading) {
            return;
        }
        summary_state.set(SummaryState::Loading);
        let (lineage, config) = {
            let snapshot = app_state.read();
            (snapshot.tree.lineage_points(&path_for_summarize), snapshot.llm_config.clone())
        };
        let mut app_state = app_state.clone();
        let path = path_for_summarize.clone();
        let mut summary_state = summary_state.clone();
        let id = id_for_summary.clone();
        spawn(async move {
            match config {
                | Ok(config) => {
                    let client = llm::LlmClient::new(config);
                    match client.summarize_lineage(&lineage).await {
                        | Ok(summary) => {
                            app_state.with_mut(|state| {
                                state.tree.update_point(&path, summary);
                            });
                            update_height(&id);
                            summary_state.set(SummaryState::Idle);
                        }
                        | Err(err) => {
                            summary_state.set(SummaryState::Error(err.to_string()));
                        }
                    }
                }
                | Err(err) => {
                    summary_state.set(SummaryState::Error(err.to_string()));
                }
            }
        });
    };

    rsx! {
        li { class: "{block_class}",
            span { class: "bb-dot", "aria-hidden": "true" }
            div { class: "bb-content",
                textarea {
                    id: id_for_input,
                    class: "bb-point",
                    rows: 1,
                    value: point_text,
                    oninput: move |evt| {
                        let next_value = evt.value();
                        app_state.with_mut(|state| {
                            state.tree.update_point(&path_for_input, next_value.clone());
                        });
                        update_height(&id);
                    },
                }
                Actions {
                    summarize_disabled,
                    summarize_title,
                    on_summarize,
                }
            }
            if !children.is_empty() {
                Line { blocks: children, path: path_for_children, app_state }
            }
        }
    }
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

#[derive(Clone, PartialEq)]
enum SummaryState {
    Idle,
    Loading,
    Error(String),
}

impl Default for SummaryState {
    fn default() -> Self {
        Self::Idle
    }
}

#[component]
fn Actions(
    on_summarize: EventHandler<MouseEvent>, summarize_disabled: bool, summarize_title: String,
) -> Element {
    rsx! {
        div { class: "bb-actions", "aria-hidden": "true",
            button { class: "bb-action-btn", r#type: "button", "+" }
            button {
                class: "bb-action-btn",
                r#type: "button",
                disabled: summarize_disabled,
                title: summarize_title,
                onclick: on_summarize,
                "-"
            }
            button { class: "bb-action-btn", r#type: "button", "o" }
        }
    }
}
