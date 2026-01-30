use leptos::prelude::*;

#[derive(Clone)]
struct BlockData {
    point: String,
    children: Vec<BlockData>,
    is_root: bool,
}

#[component]
pub fn App() -> impl IntoView {
    let tree = vec![BlockData {
        point: "Notes on liberating productivity".into(),
        is_root: true,
        children: vec![
            BlockData { point: "马克思：《资本论》".into(), children: vec![], is_root: false },
            BlockData {
                point: "马克思·韦伯：《新教伦理与资本主义精神》".into(),
                children: vec![],
                is_root: false,
            },
            BlockData {
                point: "Ivan Zhao: Steam, Steel, and Invisible Minds".into(),
                children: vec![],
                is_root: false,
            },
        ],
    }];

    view! {
        <main class="app">
            <div class="canvas">
                <Line blocks=tree />
            </div>
        </main>
    }
}

#[component]
fn Line(blocks: Vec<BlockData>) -> impl IntoView {
    view! {
        <section class="line">
            <ul class="children">
                {blocks
                    .into_iter()
                    .map(|block| view! { <Block block /> })
                    .collect_view()}
            </ul>
        </section>
    }
    .into_any()
}

#[component]
fn Block(block: BlockData) -> impl IntoView {
    let BlockData { point, children, is_root } = block;
    let block_class = if is_root { "block root" } else { "block" };
    let children_view =
        if children.is_empty() { None } else { Some(view! { <Line blocks=children /> }) };

    view! {
        <li class=block_class>
            <span class="dot" aria-hidden="true"></span>
            <div class="content">
                <span class="point">{point}</span>
                <Actions />
            </div>
            {children_view}
        </li>
    }
}

#[component]
fn Actions() -> impl IntoView {
    view! {
        <div class="actions" aria-hidden="true">
            <button class="action" type="button">"+"</button>
            <button class="action" type="button">"-"</button>
            <button class="action" type="button">"o"</button>
        </div>
    }
}
