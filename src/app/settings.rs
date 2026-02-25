//! Settings view: multi-provider LLM configuration, appearance toggle, and data path display.
//!
//! The settings view is an alternative screen accessible from the document view
//! via a gear icon button. It exposes editable LLM provider configurations with
//! CRUD support (add, edit, delete, switch active), a light/dark theme toggle,
//! and read-only display of resolved data and config paths.
//!
//! # Multi-provider model
//!
//! [`LlmProviders`] holds a named `BTreeMap<String, LlmConfig>` with one
//! designated active provider. The settings view drafts edits against the
//! currently *selected* provider (which may differ from the *active* provider).
//! Changes are non-destructive until the user explicitly saves.
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
use crate::llm;
use crate::paths::AppPaths;
use crate::theme;
use iced::widget::{button, column, container, pick_list, row, text, text_input, toggler};
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
/// Populated from the current [`LlmProviders`] when the settings screen opens,
/// and written back on explicit save. The `selected_provider` tracks which
/// provider's fields are being edited; it may differ from `active_provider`.
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// Name of the provider currently being edited in the form.
    pub selected_provider: String,
    /// Draft base URL for the selected provider's LLM endpoint.
    pub base_url: String,
    /// Draft API key for the selected provider.
    pub api_key: String,
    /// Draft model identifier for the selected provider.
    pub model: String,
    /// Names of all providers, kept in sync for the picker UI.
    pub provider_names: Vec<String>,
    /// Name of the provider designated as active for LLM requests.
    pub active_provider: String,
    /// Draft name for a new provider being added.
    pub new_provider_name: String,
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
    /// Switch the form to edit a different provider's fields.
    SelectProvider(String),
    /// Designate a provider as the active one for LLM requests.
    SetActiveProvider(String),
    /// Add a new provider with the name from `new_provider_name`.
    AddProvider,
    /// Delete a provider by name (must not be the active one).
    DeleteProvider(String),
    /// Draft new-provider name changed.
    NewProviderNameChanged(String),
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
    /// Initialize draft values from the current provider collection.
    ///
    /// Selects the active provider's fields for initial editing.
    pub fn from_providers(providers: &llm::LlmProviders) -> Self {
        let active = providers.active().to_string();
        let selected = active.clone();
        let config = providers.get(&selected).cloned().unwrap_or_default();
        Self {
            selected_provider: selected,
            base_url: config.base_url().to_string(),
            api_key: config.api_key().to_string(),
            model: config.model().to_string(),
            provider_names: providers.provider_names(),
            active_provider: active,
            new_provider_name: String::new(),
            status: None,
        }
    }

    /// Reload draft fields from the provider collection for the currently selected provider.
    fn load_selected_fields(&mut self, providers: &llm::LlmProviders) {
        if let Some(config) = providers.get(&self.selected_provider) {
            self.base_url = config.base_url().to_string();
            self.api_key = config.api_key().to_string();
            self.model = config.model().to_string();
        }
        self.provider_names = providers.provider_names();
        self.active_provider = providers.active().to_string();
    }
}

/// Handle a settings message, returning any follow-up task.
pub fn handle(state: &mut AppState, message: SettingsMessage) -> Task<Message> {
    match message {
        | SettingsMessage::Open => {
            state.settings = SettingsState::from_providers(&state.providers);
            state.active_view = ViewMode::Settings;
            tracing::info!("settings view opened");
            Task::none()
        }
        | SettingsMessage::Close => {
            state.active_view = ViewMode::Document;
            tracing::info!("settings view closed");
            Task::none()
        }
        | SettingsMessage::SelectProvider(name) => {
            state.settings.selected_provider = name;
            state.settings.load_selected_fields(&state.providers);
            state.settings.status = None;
            tracing::info!(
                provider = %state.settings.selected_provider,
                "switched settings form to provider"
            );
            Task::none()
        }
        | SettingsMessage::SetActiveProvider(name) => {
            match state.providers.set_active(&name) {
                | Ok(()) => {
                    state.settings.active_provider = name.clone();
                    state.settings.status = None;
                    if let Err(err) = state.providers.save_to_file() {
                        state.settings.status =
                            Some(SettingsStatus::Error(format!("save failed: {err}")));
                        tracing::error!(%err, "failed to save active provider change");
                    } else {
                        tracing::info!(provider = %name, "active provider changed");
                    }
                }
                | Err(err) => {
                    state.settings.status =
                        Some(SettingsStatus::Error(format!("invalid provider: {err}")));
                }
            }
            Task::none()
        }
        | SettingsMessage::AddProvider => {
            let name = state.settings.new_provider_name.trim().to_string();
            if name.is_empty() {
                state.settings.status =
                    Some(SettingsStatus::Error("provider name cannot be empty".to_string()));
                return Task::none();
            }
            if state.providers.get(&name).is_some() {
                state.settings.status =
                    Some(SettingsStatus::Error(format!("provider \"{name}\" already exists")));
                return Task::none();
            }
            state.providers.upsert_provider(name.clone(), llm::LlmConfig::default());
            state.settings.new_provider_name.clear();
            state.settings.selected_provider = name.clone();
            state.settings.load_selected_fields(&state.providers);
            state.settings.status = None;
            tracing::info!(provider = %name, "added new provider");
            Task::none()
        }
        | SettingsMessage::DeleteProvider(name) => {
            match state.providers.remove_provider(&name) {
                | Ok(()) => {
                    if state.settings.selected_provider == name {
                        state.settings.selected_provider = state.providers.active().to_string();
                    }
                    state.settings.load_selected_fields(&state.providers);
                    state.settings.status = None;
                    if let Err(err) = state.providers.save_to_file() {
                        state.settings.status =
                            Some(SettingsStatus::Error(format!("save failed: {err}")));
                        tracing::error!(%err, "failed to save provider deletion");
                    } else {
                        tracing::info!(provider = %name, "deleted provider");
                    }
                }
                | Err(err) => {
                    state.settings.status = Some(SettingsStatus::Error(format!("{err}")));
                }
            }
            Task::none()
        }
        | SettingsMessage::NewProviderNameChanged(value) => {
            state.settings.new_provider_name = value;
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
            let draft = llm::LlmConfig::from_raw(
                state.settings.base_url.clone(),
                state.settings.api_key.clone(),
                state.settings.model.clone(),
            );
            match draft {
                | Ok(config) => {
                    let provider_name = state.settings.selected_provider.clone();
                    state.providers.upsert_provider(provider_name.clone(), config);
                    match state.providers.save_to_file() {
                        | Ok(()) => {
                            state.settings.status = Some(SettingsStatus::Saved);
                            state
                                .errors
                                .retain(|e| !matches!(e, super::error::AppError::Configuration(_)));
                            tracing::info!(
                                provider = %provider_name,
                                "provider config saved to file"
                            );
                        }
                        | Err(err) => {
                            state.settings.status =
                                Some(SettingsStatus::Error(format!("save failed: {err}")));
                            tracing::error!(%err, "failed to save provider config");
                        }
                    }
                }
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
/// Layout: back button + title, then sections for provider management,
/// provider config editing, appearance, and data paths, all within a
/// centered scrollable container matching the document canvas width.
pub fn view(state: &AppState) -> Element<'_, Message> {
    let settings = &state.settings;
    let palette = if state.is_dark { &theme::DARK } else { &theme::LIGHT };

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

    // ── Provider selector section ────────────────────────────────────
    let provider_picker = pick_list(
        settings.provider_names.clone(),
        Some(settings.selected_provider.clone()),
        |name| Message::Settings(SettingsMessage::SelectProvider(name)),
    )
    .text_size(14)
    .padding(8);

    let active_indicator: Element<'_, Message> =
        if settings.selected_provider == settings.active_provider {
            text("Active").size(12).color(palette.success).into()
        } else {
            button(text("Set Active").size(12).font(theme::INTER))
                .on_press(Message::Settings(SettingsMessage::SetActiveProvider(
                    settings.selected_provider.clone(),
                )))
                .style(theme::action_button)
                .padding(iced::Padding::new(4.0).left(10.0).right(10.0))
                .into()
        };

    let selector_row =
        row![provider_picker, active_indicator].spacing(12).align_y(iced::Alignment::Center);

    let new_provider_input = text_input("New provider name...", &settings.new_provider_name)
        .on_input(|v| Message::Settings(SettingsMessage::NewProviderNameChanged(v)))
        .size(14)
        .padding(8);

    let add_button = button(text("Add").font(theme::INTER).size(13))
        .on_press(Message::Settings(SettingsMessage::AddProvider))
        .style(theme::action_button)
        .padding(iced::Padding::new(6.0).left(12.0).right(12.0));

    let add_row = row![new_provider_input, add_button].spacing(8).align_y(iced::Alignment::Center);

    let mut provider_management = column![selector_row, add_row].spacing(10);

    let can_delete =
        settings.provider_names.len() > 1 && settings.selected_provider != settings.active_provider;
    if can_delete {
        let delete_btn = button(text("Delete this provider").size(12).color(palette.danger))
            .on_press(Message::Settings(SettingsMessage::DeleteProvider(
                settings.selected_provider.clone(),
            )))
            .style(theme::action_button)
            .padding(iced::Padding::new(4.0).left(10.0).right(10.0));
        provider_management = provider_management.push(delete_btn);
    }

    let provider_section = section("Providers", provider_management);

    // ── Provider config editing section ──────────────────────────────
    let editing_title = format!("Configuration: {}", settings.selected_provider);

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
            | SettingsStatus::Saved => text("Saved").size(13).color(palette.success),
            | SettingsStatus::Error(msg) => text(msg.as_str()).size(13).color(palette.danger),
        };
        save_row = save_row.push(status_text);
    }

    let config_section = container(
        column![
            text(editing_title).size(16).font(theme::INTER),
            column![base_url_input, api_key_input, model_input, save_row,].spacing(10),
        ]
        .spacing(12),
    )
    .style(theme::draft_panel)
    .padding(
        iced::Padding::new(theme::PANEL_PAD_V).left(theme::PANEL_PAD_H).right(theme::PANEL_PAD_H),
    )
    .width(Fill);

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
    let content =
        column![header, provider_section, config_section, appearance_section, paths_section]
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
