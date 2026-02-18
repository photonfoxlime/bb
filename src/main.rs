mod app;
mod llm;

fn main() -> iced::Result {
    iced::application(app::AppState::load, app::update, app::view).run()
}
