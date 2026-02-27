//! Settings view: multi-provider LLM configuration, appearance toggle, and data path display.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! The settings view is an alternative screen accessible from the document view
//! via a gear icon button. It exposes editable LLM provider configurations with
//! CRUD support (add, edit, delete, switch active), a light/dark theme toggle,
//! and read-only display of resolved data and config paths.
//!
//! # Preset vs custom providers
//!
//! [`llm::LlmProviders`] separates providers into two categories:
//!
//! - Preset providers (OpenAI, OpenRouter, etc.) are always present and
//!   cannot be deleted. Their base URL is fixed; the user only supplies an
//!   API key and optionally overrides the model. Saving a preset skips
//!   `from_raw` validation — an empty API key is allowed (the user just
//!   hasn't configured this preset yet).
//! - Custom providers are fully user-managed. All fields (name, base URL,
//!   API key, model) are editable and validated via `from_raw` before save.
//!   Users can add and delete custom providers.
//!
//! The settings view drafts edits against the currently *selected* provider
//! (which may differ from the *active* provider). Changes are non-destructive
//! until the user explicitly saves.
//!
//! # Architecture
//!
//! - [`SettingsState`] stores draft form values so edits are non-destructive
//!   until the user explicitly saves.
//! - [`SettingsMessage`] variants drive all settings interactions through the
//!   standard Elm-architecture `update` cycle.

use super::config::{self, AppConfig};
use super::{AppState, Message, ViewMode};
use crate::i18n;
use crate::llm;
use crate::paths::AppPaths;
use crate::theme;
use iced::widget::{button, column, container, pick_list, row, text, text_input, toggler};
use iced::{Element, Fill, Length, Task};
use rust_i18n::t;

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
    ///
    /// Read-only in the UI for preset providers (derived from the variant).
    pub base_url: String,
    /// Draft API key for the selected provider.
    pub api_key: String,
    /// Draft model identifier for the selected provider.
    pub model: String,
    /// Names of all providers, kept in sync for the picker UI.
    pub provider_names: Vec<String>,
    /// Name of the provider designated as active for LLM requests.
    pub active_provider: String,
    /// Draft name for a new custom provider being added.
    pub new_provider_name: String,
    /// Transient status message shown after save attempts.
    pub status: Option<SettingsStatus>,
    /// Whether the currently selected provider is a preset.
    ///
    /// Drives UI decisions: base URL read-only, delete hidden, save skips
    /// `from_raw` validation.
    pub selected_is_preset: bool,
    /// Draft app configuration (e.g. locale override).
    pub config: AppConfig,
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
    /// Add a new custom provider with the name from `new_provider_name`.
    AddProvider,
    /// Delete a custom provider by name (presets cannot be deleted).
    DeleteProvider(String),
    /// Draft new-provider name changed.
    NewProviderNameChanged(String),
    /// Draft base URL changed (only effective for custom providers).
    BaseUrlChanged(String),
    /// Draft API key changed.
    ApiKeyChanged(String),
    /// Draft model name changed.
    ModelChanged(String),
    /// Persist draft values to the TOML config file and reload.
    Save,
    /// Toggle between light and dark appearance.
    ToggleTheme(bool),
    /// Change the locale override.
    SetLocale(Option<String>),
}

impl SettingsState {
    /// Initialize draft values from the current provider collection and app config.
    ///
    /// Selects the active provider's fields for initial editing.
    pub fn from_providers(providers: &llm::LlmProviders, config: &AppConfig) -> Self {
        let active = providers.active().to_string();
        let selected = active.clone();
        let (base_url, api_key, model) = providers.raw_fields(&selected).unwrap_or_default();
        let selected_is_preset = providers.is_preset(&selected);
        Self {
            selected_provider: selected,
            base_url,
            api_key,
            model,
            provider_names: providers.provider_names(),
            active_provider: active,
            new_provider_name: String::new(),
            status: None,
            selected_is_preset,
            config: config.clone(),
        }
    }

    /// Reload draft fields from the provider collection for the currently selected provider.
    fn load_selected_fields(&mut self, providers: &llm::LlmProviders) {
        if let Some((base_url, api_key, model)) = providers.raw_fields(&self.selected_provider) {
            self.base_url = base_url;
            self.api_key = api_key;
            self.model = model;
        }
        self.selected_is_preset = providers.is_preset(&self.selected_provider);
        self.provider_names = providers.provider_names();
        self.active_provider = providers.active().to_string();
    }
}

/// Handle a settings message, returning any follow-up task.
pub fn handle(state: &mut AppState, message: SettingsMessage) -> Task<Message> {
    match message {
        | SettingsMessage::Open => {
            state.settings = SettingsState::from_providers(&state.providers, &state.config);
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
            if state.providers.provider_exists(&name) {
                state.settings.status =
                    Some(SettingsStatus::Error(format!("provider \"{name}\" already exists")));
                return Task::none();
            }
            match state.providers.upsert_custom(name.clone(), llm::CustomProvider::default()) {
                | Ok(()) => {
                    state.settings.new_provider_name.clear();
                    state.settings.selected_provider = name.clone();
                    state.settings.load_selected_fields(&state.providers);
                    state.settings.status = None;
                    tracing::info!(provider = %name, "added new custom provider");
                }
                | Err(err) => {
                    state.settings.status = Some(SettingsStatus::Error(format!("{err}")));
                }
            }
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
                        tracing::info!(provider = %name, "deleted custom provider");
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
            let provider_name = state.settings.selected_provider.clone();
            if state.providers.is_preset(&provider_name) {
                // Preset: save api_key and model directly, no from_raw validation.
                // The user may save an empty api_key (not yet configured).
                let preset = llm::PresetProvider::from_name(&provider_name)
                    .expect("is_preset returned true");
                let config = llm::PresetConfig {
                    api_key: state.settings.api_key.clone(),
                    model: state.settings.model.clone(),
                };
                state.providers.update_preset(preset, config);
            } else {
                // Custom: validate all fields before saving.
                let draft = llm::LlmConfig::from_raw(
                    state.settings.base_url.clone(),
                    state.settings.api_key.clone(),
                    state.settings.model.clone(),
                );
                match draft {
                    | Ok(_config) => {
                        let custom = llm::CustomProvider {
                            base_url: state.settings.base_url.clone(),
                            api_key: state.settings.api_key.clone(),
                            model: state.settings.model.clone(),
                        };
                        if let Err(err) =
                            state.providers.upsert_custom(provider_name.clone(), custom)
                        {
                            state.settings.status = Some(SettingsStatus::Error(format!("{err}")));
                            return Task::none();
                        }
                    }
                    | Err(err) => {
                        state.settings.status =
                            Some(SettingsStatus::Error(format!("invalid config: {err}")));
                        return Task::none();
                    }
                }
            }
            // Persist to disk.
            match state.providers.save_to_file() {
                | Ok(()) => {
                    state.settings.status = Some(SettingsStatus::Saved);
                    state.errors.retain(|e| !matches!(e, super::error::AppError::Configuration(_)));
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
            Task::none()
        }
        | SettingsMessage::ToggleTheme(is_dark) => {
            state.is_dark = is_dark;
            tracing::info!(is_dark, "theme toggled from settings");
            Task::none()
        }
        | SettingsMessage::SetLocale(locale) => {
            // Update both the main config and settings config so effective_locale()
            // returns the new locale for immediate UI re-render.
            state.config.locale = locale.clone();
            state.settings.config.locale = locale.clone();
            // Save config to disk.
            if let Err(err) = config::save(&state.config) {
                state.settings.status =
                    Some(SettingsStatus::Error(format!("failed to save config: {err}")));
                tracing::error!(%err, "failed to save app config");
            } else {
                // Apply the new locale immediately for the current session.
                let effective = i18n::resolved_locale_from_config(&state.config);
                i18n::set_app_locale(&effective);
                tracing::info!(locale = %effective, "locale changed from settings");
            }
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
///
/// Preset providers show a read-only base URL and hide the delete button.
/// Custom providers have all fields editable and can be deleted.
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

    let header =
        row![back_button, text(t!("settings_title").to_string()).size(20).font(theme::INTER),]
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
            text(t!("settings_active").to_string()).size(12).color(palette.success).into()
        } else {
            button(text(t!("settings_set_active").to_string()).size(12).font(theme::INTER))
                .on_press(Message::Settings(SettingsMessage::SetActiveProvider(
                    settings.selected_provider.clone(),
                )))
                .style(theme::action_button)
                .padding(iced::Padding::new(4.0).left(10.0).right(10.0))
                .into()
        };

    let selector_row =
        row![provider_picker, active_indicator].spacing(12).align_y(iced::Alignment::Center);

    let new_provider_placeholder = t!("settings_new_provider_placeholder").to_string();
    let new_provider_input = text_input(&new_provider_placeholder, &settings.new_provider_name)
        .on_input(|v| Message::Settings(SettingsMessage::NewProviderNameChanged(v)))
        .size(14)
        .padding(8);

    let add_button = button(text(t!("settings_add").to_string()).font(theme::INTER).size(13))
        .on_press(Message::Settings(SettingsMessage::AddProvider))
        .style(theme::action_button)
        .padding(iced::Padding::new(6.0).left(12.0).right(12.0));

    let add_row = row![new_provider_input, add_button].spacing(8).align_y(iced::Alignment::Center);

    let mut provider_management = column![selector_row, add_row].spacing(10);

    // Only custom providers can be deleted (presets are always available).
    let can_delete =
        !settings.selected_is_preset && settings.selected_provider != settings.active_provider;
    if can_delete {
        let delete_btn =
            button(text(t!("settings_delete_provider").to_string()).size(12).color(palette.danger))
                .on_press(Message::Settings(SettingsMessage::DeleteProvider(
                    settings.selected_provider.clone(),
                )))
                .style(theme::action_button)
                .padding(iced::Padding::new(4.0).left(10.0).right(10.0));
        provider_management = provider_management.push(delete_btn);
    }

    let provider_section = section(t!("settings_providers").to_string(), provider_management);

    // ── Provider config editing section ──────────────────────────────
    let editing_title =
        t!("settings_configuration", name = settings.selected_provider.as_str()).to_string();

    // For preset providers, base URL is fixed and shown as read-only text.
    // For custom providers, base URL is an editable input field.
    let base_url_label = t!("settings_base_url").to_string();
    let base_url_placeholder = t!("settings_base_url_placeholder").to_string();
    let api_key_label = t!("settings_api_key").to_string();
    let api_key_placeholder = t!("settings_api_key_placeholder").to_string();
    let model_label = t!("settings_model").to_string();
    let model_placeholder = t!("settings_model_placeholder").to_string();
    let base_url_field: Element<'_, Message> = if settings.selected_is_preset {
        labeled_readonly(base_url_label, &settings.base_url)
    } else {
        labeled_input(base_url_label, &settings.base_url, base_url_placeholder, |v| {
            Message::Settings(SettingsMessage::BaseUrlChanged(v))
        })
    };
    let api_key_input = labeled_input(api_key_label, &settings.api_key, api_key_placeholder, |v| {
        Message::Settings(SettingsMessage::ApiKeyChanged(v))
    });
    let model_input = labeled_input(model_label, &settings.model, model_placeholder, |v| {
        Message::Settings(SettingsMessage::ModelChanged(v))
    });

    let save_button =
        button(row![text(t!("settings_save").to_string()).font(theme::INTER).size(14),])
            .on_press(Message::Settings(SettingsMessage::Save))
            .style(theme::action_button)
            .padding(iced::Padding::new(6.0).left(16.0).right(16.0));

    let mut save_row = row![save_button].spacing(12).align_y(iced::Alignment::Center);
    if let Some(status) = &settings.status {
        let status_text = match status {
            | SettingsStatus::Saved => {
                text(t!("settings_saved").to_string()).size(13).color(palette.success)
            }
            | SettingsStatus::Error(msg) => text(msg.as_str()).size(13).color(palette.danger),
        };
        save_row = save_row.push(status_text);
    }

    let config_section = container(
        column![
            text(editing_title).size(16).font(theme::INTER),
            column![base_url_field, api_key_input, model_input, save_row,].spacing(10),
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
        .label(t!("settings_dark_mode").to_string())
        .text_size(14);

    // Locale picker: None = system default, Some("en-US") = override.
    let locale_labels: Vec<String> = vec![t!("settings_system_default").to_string()]
        .into_iter()
        .chain(i18n::SUPPORTED_LOCALES.iter().map(|s| s.to_string()))
        .collect();
    let current_locale_idx = if state.settings.config.locale.is_none() {
        0
    } else {
        i18n::SUPPORTED_LOCALES
            .iter()
            .position(|s| Some(s.to_string()) == state.settings.config.locale)
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    let locale_picker = pick_list(
        locale_labels.clone(),
        Some(locale_labels[current_locale_idx].clone()),
        move |label| {
            let idx = locale_labels.iter().position(|l| *l == label).unwrap_or(0);
            let locale = if idx == 0 { None } else { Some(locale_labels[idx].clone()) };
            Message::Settings(SettingsMessage::SetLocale(locale))
        },
    )
    .text_size(14)
    .padding(8);

    let appearance_title = t!("settings_appearance").to_string();
    let appearance_section =
        section(appearance_title, column![locale_picker, theme_toggler].spacing(10));

    // ── Data Paths section ───────────────────────────────────────────
    let data_path = AppPaths::data_file()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| t!("settings_not_available").to_string());
    let config_path = AppPaths::llm_config()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| t!("settings_not_available").to_string());

    let paths_title = t!("settings_data_paths").to_string();
    let data_file_label = t!("settings_data_file").to_string();
    let llm_config_label = t!("settings_llm_config").to_string();
    let paths_section = section(
        paths_title,
        column![path_row(data_file_label, data_path), path_row(llm_config_label, config_path),]
            .spacing(6),
    );

    // ── Assemble ─────────────────────────────────────────────────────
    let max_width = theme::canvas_max_width(state.window_size.width);
    let content =
        column![header, provider_section, config_section, appearance_section, paths_section]
            .spacing(24)
            .max_width(max_width);

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
/// Takes owned strings so the returned element does not borrow from the view.
fn labeled_input(
    label: String, value: &str, placeholder: String, on_input: impl Fn(String) -> Message + 'static,
) -> Element<'static, Message> {
    column![
        text(label).size(13).font(theme::INTER).color(theme::LIGHT.accent_muted),
        text_input(placeholder.as_str(), value).on_input(on_input).size(14).padding(8),
    ]
    .spacing(4)
    .into()
}

/// A labeled read-only text display (used for preset base URLs).
fn labeled_readonly(label: String, value: &str) -> Element<'static, Message> {
    column![
        text(label).size(13).font(theme::INTER).color(theme::LIGHT.accent_muted),
        text(value.to_string()).size(14),
    ]
    .spacing(4)
    .into()
}

/// A section with a title and content.
fn section(
    title: String, content: impl Into<Element<'static, Message>>,
) -> Element<'static, Message> {
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
fn path_row(label: String, path: String) -> Element<'static, Message> {
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
