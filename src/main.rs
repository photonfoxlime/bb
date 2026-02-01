mod app;
mod llm;

fn main() {
    dioxus::logger::initialize_default();
    let config = dioxus::desktop::Config::new();
    dioxus::LaunchBuilder::new()
        .with_cfg(config)
        .launch(app::App);
}
