//! Binary entry point for `bb`.
//!
//! Runtime wiring in this file:
//! - load app state (`AppState::load`),
//! - route `update` + `view` + `subscription`,
//! - load fonts and theme from app state.

mod app;
mod mount;
mod store;
mod llm;
mod paths;
mod theme;
mod undo;

// const DEFAULT_FONT: iced::Font = iced::Font::with_name("Inter");
const DEFAULT_FONT: iced::Font = iced::Font::with_name("LXGW WenKai");

fn main() -> iced::Result {
    iced::application(app::AppState::load, app::update, app::view)
        .subscription(app::subscription)
        .font(include_bytes!("../assets/fonts/Inter-300.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-400.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-500.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Light.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf").as_slice())
        .font(lucide_icons::LUCIDE_FONT_BYTES)
        .default_font(DEFAULT_FONT)
        .theme(|state: &app::AppState| theme::app_theme(state.is_dark))
        .title("Block Bunny")
        .run()
}
