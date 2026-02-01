mod app;
mod llm;

fn main() {
    dioxus::logger::initialize_default();
    let config = dioxus::desktop::Config::new().with_on_window(|win, _| {
        win.set_always_on_top(false);
        win.set_maximized(true);
    });
    dioxus::LaunchBuilder::new().with_cfg(config).launch(app::App);
}
