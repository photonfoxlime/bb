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

    fn data_file_path() -> Option<PathBuf> {
        PROJECT_DIRS
            .as_ref()
            .map(|project| project.data_dir().join("blocks.json"))
    }

    fn default_blocks() -> Vec<BlockData> {
        vec![BlockData::new(
            "Notes on liberating productivity",
            true,
            vec![
                BlockData::new("马克思：《资本论》", false, vec![]),
                BlockData::new("马克思·韦伯：《新教伦理与资本主义精神》", false, vec![]),
                BlockData::new(
                    "Ivan Zhao: Steam, Steel, and Invisible Minds",
                    false,
                    vec![],
                ),
            ],
        )]
    }
}

impl Default for BlockForest {
    fn default() -> Self {
        Self::new(Self::default_blocks())
    }
}

#[component]
pub fn App() -> Element {
    use_effect(|| {
        dioxus::desktop::window().set_always_on_top(false);
        dioxus::desktop::window().set_maximized(true);
        dioxus::desktop::window().devtool(); // opens the webview devtools
    });

    let tree = use_signal(BlockForest::load);

    {
        let tree = tree.clone();
        use_effect(move || {
            let snapshot = tree.read().clone();
            let _ = snapshot.save();
        });
    }

    let tree_snapshot = tree.read().clone();
    rsx! {
        document::Stylesheet { href: TAILWIND_CSS }
        document::Stylesheet { href: APP_CSS }
        document::Stylesheet { href: FONTS_CSS }
        main { class: "min-h-screen",
            div { class: "bb-canvas",
                Line { blocks: tree_snapshot.blocks, path: vec![], tree }
            }
        }
    }
}

#[component]
fn Line(blocks: Vec<BlockData>, path: Vec<usize>, tree: Signal<BlockForest>) -> Element {
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
                    Block { key: "{index}", block, path: next_path, tree }
                }
            }
        }
    }
}

#[component]
fn Block(block: BlockData, path: Vec<usize>, tree: Signal<BlockForest>) -> Element {
    let BlockData {
        point,
        children,
        is_root,
    } = block;
    let block_class = if is_root {
        "bb-block bb-block-root"
    } else {
        "bb-block"
    };
    let point_text = point.clone();

    let id = use_hook(|| format!("ta-{}", Uuid::new_v4()));

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
    let id_for_input = id.clone();
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
                        tree.with_mut(|tree| tree.update_point(&path, next_value.clone()));
                        update_height(&id);
                    },
                }
                Actions {}
            }
            if !children.is_empty() {
                Line { blocks: children, path: path_for_children, tree }
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
        Self {
            point: point.to_string(),
            children,
            is_root,
        }
    }
}

#[component]
fn Actions() -> Element {
    rsx! {
        div { class: "bb-actions", "aria-hidden": "true",
            button { class: "bb-action-btn", r#type: "button", "+" }
            button { class: "bb-action-btn", r#type: "button", "-" }
            button { class: "bb-action-btn", r#type: "button", "o" }
        }
    }
}
