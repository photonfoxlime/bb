#[allow(unused)]
use dioxus::{logger::tracing, prelude::*};
use uuid::Uuid;

static APP_CSS: Asset = asset!("/assets/app.css");
static FONTS_CSS: Asset = asset!("/assets/fonts.css");
static TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

const _: Asset = asset!("/assets/fonts/Inter-300.woff2");
const _: Asset = asset!("/assets/fonts/Inter-400.woff2");
const _: Asset = asset!("/assets/fonts/Inter-500.woff2");
const _: Asset = asset!("/assets/fonts/LXGWWenKai-Light.ttf");
const _: Asset = asset!("/assets/fonts/LXGWWenKai-Regular.ttf");
const _: Asset = asset!("/assets/fonts/LXGWWenKai-Medium.ttf");

#[derive(Clone, PartialEq)]
struct BlockData {
    point: String,
    children: Vec<BlockData>,
    is_root: bool,
}

#[component]
pub fn App() -> Element {
    #[css_module("/assets/app.module.css")]
    struct AppStyles;

    use_effect(|| {
        dioxus::desktop::window().set_always_on_top(false);
        dioxus::desktop::window().set_maximized(true);
        dioxus::desktop::window().devtool(); // opens the webview devtools
    });

    let tree = vec![BlockData::new(
        "Notes on liberating productivity",
        true,
        vec![
            BlockData::new("马克思：《资本论》", false, vec![]),
            BlockData::new("马克思·韦伯：《新教伦理与资本主义精神》", false, vec![]),
            BlockData::new("Ivan Zhao: Steam, Steel, and Invisible Minds", false, vec![]),
        ],
    )];
    rsx! {
        document::Stylesheet { href: TAILWIND_CSS }
        document::Stylesheet { href: APP_CSS }
        document::Stylesheet { href: FONTS_CSS }
        main { class: AppStyles::app,
            div { class: AppStyles::canvas,
                Line { blocks: tree }
            }
        }
    }
}

#[component]
fn Line(blocks: Vec<BlockData>) -> Element {
    #[css_module("/assets/line.module.css")]
    struct LineStyles;

    rsx! {
        section { class: LineStyles::line,
            ul { class: LineStyles::children,
                for (index, block) in blocks.into_iter().enumerate() {
                    Block { key: "{index}", block }
                }
            }
        }
    }
}

#[component]
fn Block(block: BlockData) -> Element {
    #[css_module("/assets/block.module.css")]
    struct BlockStyles;

    let BlockData { point, children, is_root } = block;
    let block_class = if is_root {
        format!("{} {}", BlockStyles::block, BlockStyles::root)
    } else {
        BlockStyles::block.to_string()
    };
    let mut point = use_signal(|| point);
    let point_text = point.read().clone();

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

    {
        let id = id.clone();
        let point = point.clone();
        use_effect(move || {
            // rerun when value changes
            let _point = point.read();
            update_height(&id);
        });
    }

    rsx! {
        li { class: "{block_class}",
            span { class: BlockStyles::dot, "aria-hidden": "true" }
            div { class: BlockStyles::content,
                textarea {
                    id,
                    class: BlockStyles::point,
                    rows: 1,
                    value: point_text,
                    oninput: move |evt| {
                        point.set(evt.value());
                    },
                }
                Actions {}
            }
            if !children.is_empty() {
                Line { blocks: children }
            }
        }
    }
}

impl BlockData {
    fn new(point: impl ToString, is_root: bool, children: Vec<BlockData>) -> Self {
        Self { point: point.to_string(), children, is_root }
    }
}

#[component]
fn Actions() -> Element {
    #[css_module("/assets/actions.module.css")]
    struct ActionsStyles;

    rsx! {
        div { class: ActionsStyles::actions, "aria-hidden": "true",
            button { class: ActionsStyles::action, r#type: "button", "+" }
            button { class: ActionsStyles::action, r#type: "button", "-" }
            button { class: ActionsStyles::action, r#type: "button", "o" }
        }
    }
}
