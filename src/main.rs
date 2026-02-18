mod app;
mod llm;

// const DEFAULT_FONT: iced::Font = iced::Font::with_name("Inter");
const DEFAULT_FONT: iced::Font = iced::Font::with_name("LXGW WenKai");

fn main() -> iced::Result {
    iced::application(app::AppState::load, app::update, app::view)
        .font(include_bytes!("../assets/fonts/Inter-300.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-400.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-500.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Light.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf").as_slice())
        .default_font(DEFAULT_FONT)
        .run()
}
