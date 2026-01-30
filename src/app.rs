use leptos::prelude::*;

#[derive(Clone)]
struct BlockData {
    point: RwSignal<String>,
    children: Vec<BlockData>,
    is_root: bool,
}

#[component]
pub fn App() -> impl IntoView {
    let tree = vec![BlockData::new(
        "Notes on liberating productivity",
        true,
        vec![
            BlockData::new("马克思：《资本论》", false, vec![]),
            BlockData::new("马克思·韦伯：《新教伦理与资本主义精神》", false, vec![]),
            BlockData::new("Ivan Zhao: Steam, Steel, and Invisible Minds", false, vec![]),
        ],
    )];

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
                <input
                    class="point"
                    type="text"
                    prop:value=move || point.get()
                    style:width=move || format!("{}ch", point.get().chars().count().max(1))
                    on:input=move |ev| point.set(event_target_value(&ev))
                />
                <Actions />
            </div>
            {children_view}
        </li>
    }
}

impl BlockData {
    fn new(point: impl ToString, is_root: bool, children: Vec<BlockData>) -> Self {
        Self { point: RwSignal::new(point.to_string()), children, is_root }
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
