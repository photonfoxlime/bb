//! Settings view: LLM configuration, appearance toggle, and data path display.
//!
//! The settings view is an alternative screen accessible from the document view
//! via a gear icon button. It exposes editable LLM configuration (base URL,
//! API key, model) with save-to-file support, a light/dark theme toggle, and
//! read-only display of resolved data and config paths.
//!
//! # Architecture
//!
//! - [`ViewMode`] selects between the document tree view and the settings view.
//!   `AppState` holds an `active_view: ViewMode` field that `view()` branches on.
//! - [`SettingsState`] stores draft form values so edits are non-destructive
//!   until the user explicitly saves.
//! - [`SettingsMessage`] variants drive all settings interactions through the
//!   standard Elm-architecture `update` cycle.

use super::{AppState, Message};
use crate::llm::{self, LlmConfig};
use crate::paths::AppPaths;
use crate::theme;
use iced::widget::{button, column, container, row, text, text_input, toggler};
use iced::{Element, Fill, Length, Task};

/// Which top-level screen is active.
///
/// The document view is the default; settings is reached via a gear icon button
/// and dismissed with a back arrow or Escape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// The main tree-structured document editor.
    #[default]
    Document,
    /// The settings configuration screen.
    Settings,
}

/// Draft form values for the settings screen.
///
/// Populated from the current `LlmConfig` (or defaults) when the settings
/// screen opens, and written back on explicit save.
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// Draft base URL for the LLM endpoint.
    pub base_url: String,
    /// Draft API key.
    pub api_key: String,
    /// Draft model identifier.
    pub model: String,
    /// Transient status message shown after save attempts.
    pub status: Option<SettingsStatus>,
}

/// Outcome of the last settings save attempt.
#[derive(Debug, Clone)]
pub enum SettingsStatus {
    /// Config saved and reloaded successfully.
    Saved,
    /// Save or validation failed with an error message.
    Error(String),
}

/// Messages produced by the settings view.
#[derive(Debug, Clone)]
pub enum SettingsMessage {
    /// Navigate to the settings screen.
    Open,
    /// Return to the document view.
    Close,
    /// Draft base URL changed.
    BaseUrlChanged(String),
    /// Draft API key changed.
    ApiKeyChanged(String),
    /// Draft model name changed.
    ModelChanged(String),
    /// Persist draft values to the TOML config file and reload.
    Save,
    /// Toggle between light and dark appearance.
    ToggleTheme(bool),
}

impl SettingsState {
    /// Initialize draft values from the current LLM config or defaults.
    pub fn from_config(config: &Result<LlmConfig, llm::LlmConfigError>) -> Self {
        match config {
            | Ok(cfg) => Self {
                base_url: cfg.base_url().to_string(),
                api_key: cfg.api_key().to_string(),
                model: cfg.model().to_string(),
                status: None,
            },
            | Err(_) => Self {
                base_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                status: None,
            },
        }
    }
}

/// Handle a settings message, returning any follow-up task.
pub fn handle(state: &mut AppState, message: SettingsMessage) -> Task<Message> {
    match message {
        | SettingsMessage::Open => {
            state.settings = SettingsState::from_config(&state.llm_config);
            state.active_view = ViewMode::Settings;
            tracing::info!("settings view opened");
            Task::none()
        }
        | SettingsMessage::Close => {
            state.active_view = ViewMode::Document;
            tracing::info!("settings view closed");
            Task::none()
        }
        | SettingsMessage::BaseUrlChanged(value) => {
            state.settings.base_url = value;
            state.settings.status = None;
            Task::none()
        }
        | SettingsMessage::ApiKeyChanged(value) => {
            state.settings.api_key = value;
            state.settings.status = None;
            Task::none()
        }
        | SettingsMessage::ModelChanged(value) => {
            state.settings.model = value;
            state.settings.status = None;
            Task::none()
        }
        | SettingsMessage::Save => {
            let draft = LlmConfig::from_raw(
                state.settings.base_url.clone(),
                state.settings.api_key.clone(),
                state.settings.model.clone(),
            );
            match draft {
                | Ok(config) => match config.save_to_file() {
                    | Ok(()) => {
                        state.llm_config = Ok(config);
                        state.settings.status = Some(SettingsStatus::Saved);
                        // Clear any prior configuration errors from the error stack.
                        state
                            .errors
                            .retain(|e| !matches!(e, super::error::AppError::Configuration(_)));
                        tracing::info!("LLM config saved to file");
                    }
                    | Err(err) => {
                        state.settings.status =
                            Some(SettingsStatus::Error(format!("save failed: {err}")));
                        tracing::error!(%err, "failed to save LLM config");
                    }
                },
                | Err(err) => {
                    state.settings.status =
                        Some(SettingsStatus::Error(format!("invalid config: {err}")));
                }
            }
            Task::none()
        }
        | SettingsMessage::ToggleTheme(is_dark) => {
            state.is_dark = is_dark;
            tracing::info!(is_dark, "theme toggled from settings");
            Task::none()
        }
    }
}

// ── Settings view ────────────────────────────────────────────────────

/// Render the settings screen.
///
/// Layout: back button + title, then sections for LLM config, appearance,
/// and data paths, all within a centered scrollable container matching
/// the document canvas width.
pub fn view(state: &AppState) -> Element<'_, Message> {
    let settings = &state.settings;

    // ── Header ───────────────────────────────────────────────────────
    let back_button = button(
        lucide_icons::iced::icon_arrow_left()
            .size(16)
            .line_height(iced::widget::text::LineHeight::Relative(1.0)),
    )
    .on_press(Message::Settings(SettingsMessage::Close))
    .style(theme::action_button)
    .padding(theme::BUTTON_PAD);

    let header = row![back_button, text("Settings").size(20).font(theme::INTER),]
        .spacing(12)
        .align_y(iced::Alignment::Center);

    // ── LLM Configuration section ────────────────────────────────────
    let llm_status = match &state.llm_config {
        | Ok(_) => text("Configured").size(13).color(theme::LIGHT.success),
        | Err(err) => text(format!("Error: {err}")).size(13).color(theme::LIGHT.danger),
    };

    let base_url_input =
        labeled_input("Base URL", &settings.base_url, "https://api.example.com/v1", |v| {
            Message::Settings(SettingsMessage::BaseUrlChanged(v))
        });
    let api_key_input = labeled_input("API Key", &settings.api_key, "sk-...", |v| {
        Message::Settings(SettingsMessage::ApiKeyChanged(v))
    });
    let model_input = labeled_input("Model", &settings.model, "gpt-4o", |v| {
        Message::Settings(SettingsMessage::ModelChanged(v))
    });

    let save_button = button(row![text("Save").font(theme::INTER).size(14),])
        .on_press(Message::Settings(SettingsMessage::Save))
        .style(theme::action_button)
        .padding(iced::Padding::new(6.0).left(16.0).right(16.0));

    let mut save_row = row![save_button].spacing(12).align_y(iced::Alignment::Center);
    if let Some(status) = &settings.status {
        let status_text = match status {
            | SettingsStatus::Saved => text("Saved").size(13).color(theme::LIGHT.success),
            | SettingsStatus::Error(msg) => text(msg.as_str()).size(13).color(theme::LIGHT.danger),
        };
        save_row = save_row.push(status_text);
    }

    let llm_section = section(
        "LLM Configuration",
        column![llm_status, base_url_input, api_key_input, model_input, save_row,].spacing(10),
    );

    // ── Appearance section ───────────────────────────────────────────
    let theme_toggler = toggler(state.is_dark)
        .on_toggle(|v| Message::Settings(SettingsMessage::ToggleTheme(v)))
        .label("Dark mode")
        .text_size(14);

    let appearance_section = section("Appearance", column![theme_toggler].spacing(10));

    // ── Data Paths section ───────────────────────────────────────────
    let data_path = AppPaths::data_file()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not available)".to_string());
    let config_path = AppPaths::llm_config()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not available)".to_string());

    let paths_section = section(
        "Data Paths",
        column![path_row("Data file", data_path), path_row("LLM config", config_path),].spacing(6),
    );

    // ── Assemble ─────────────────────────────────────────────────────
    let content = column![header, llm_section, appearance_section, paths_section]
        .spacing(24)
        .max_width(theme::CANVAS_MAX_WIDTH);

    let padded = container(content).padding(theme::CANVAS_PAD).width(Fill).center_x(Fill);

    container(
        iced::widget::scrollable(
            container(padded)
                .width(Fill)
                .center_x(Fill)
                .padding(iced::Padding::ZERO.top(theme::CANVAS_TOP)),
        )
        .height(Fill),
    )
    .style(theme::canvas)
    .width(Fill)
    .height(Fill)
    .into()
}

// ── Helpers ──────────────────────────────────────────────────────────

/// A labeled text input field.
fn labeled_input<'a>(
    label: &'a str, value: &'a str, placeholder: &'a str, on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(13).font(theme::INTER).color(theme::LIGHT.accent_muted),
        text_input(placeholder, value).on_input(on_input).size(14).padding(8),
    ]
    .spacing(4)
    .into()
}

/// A section with a title and content.
fn section<'a>(title: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(column![text(title).size(16).font(theme::INTER), content.into(),].spacing(12))
        .style(theme::draft_panel)
        .padding(
            iced::Padding::new(theme::PANEL_PAD_V)
                .left(theme::PANEL_PAD_H)
                .right(theme::PANEL_PAD_H),
        )
        .width(Fill)
        .into()
}

/// A read-only key-value row for path display.
///
/// Takes owned strings so the row can outlive any local temporaries.
fn path_row(label: &'static str, path: String) -> Element<'static, Message> {
    row![
        text(label)
            .size(13)
            .font(theme::INTER)
            .color(theme::LIGHT.accent_muted)
            .width(Length::Fixed(90.0)),
        text(path).size(13),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}
