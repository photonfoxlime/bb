//! Settings view: multi-provider LLM configuration, per-task model selection, and system controls.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! The settings view is an alternative screen accessible from the document view
//! via a gear icon button. It exposes:
//!
//! 1. **Provider management** — add, edit, delete LLM providers. Each provider
//!    stores only a URL and API key. Preset providers have a fixed URL.
//! 2. **Per-task LLM settings** — each [`TaskKind`] selects provider, model,
//!    and prompts independently.
//! 3. **System settings** — locale, appearance mode, Enter key behavior.
//! 4. **Data paths** — read-only display of resolved data/config file paths.
//!
//! # Preset vs custom providers
//!
//! [`llm::LlmProviders`] separates providers into two categories:
//!
//! - Preset providers (OpenAI, OpenRouter, etc.) are always present and
//!   cannot be deleted. Their base URL is fixed; the user only supplies an
//!   API key. Saving a preset skips `from_raw` validation — an empty API key
//!   is allowed (the user just hasn't configured this preset yet).
//! - Custom providers are fully user-managed. Name, base URL, and API key
//!   are editable and validated before save. Users can add and delete them.
//!
//! Note: model selection is **not** part of the provider config. Each task
//! kind picks its own provider + model independently.
//!
//! # Architecture
//!
//! - [`SettingsState`] stores draft form values so edits are non-destructive
//!   until the user explicitly saves.
//! - [`SettingsMessage`] variants drive all settings interactions through the
//!   standard Elm-architecture `update` cycle.
//! - Provider edits remain explicit-save, while per-task text fields debounce
//!   persistence so typing does not rewrite `app.toml` on every keystroke.

use super::config::{AppConfig, MaxTokens, TaskConfig};
use super::{AppState, Message, ViewMode};
use crate::component::icon_button::IconButton;
use crate::component::text_button::TextButton;
use crate::i18n;
use crate::llm::{self, TaskKind};
use crate::paths::AppPaths;
use crate::theme;
use iced::alignment::Horizontal;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, slider, text, text_input, tooltip,
};
use iced::{Alignment, Element, Fill, Length, Task};
use lucide_icons::iced as icons;
use rust_i18n::t;
use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

/// Delay after the last task-settings text edit before persisting `app.toml`.
const TASK_SETTINGS_PERSIST_DEBOUNCE_MS: u64 = 400;

/// Draft form values for the settings screen.
///
/// Populated from the current [`LlmProviders`] when the settings screen opens,
/// and written back on explicit save. The `selected_provider` tracks which
/// provider's URL and API key are being edited.
///
/// Per-task settings (provider, model, token limit) are managed independently
/// via `task_drafts`.
///
/// Provider changes and explicit system toggles persist immediately. Text-field
/// edits use a short debounce so the live config mirrors stay current without
/// rewriting `app.toml` on every keystroke.
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// Name of the provider currently being edited in the provider config form.
    pub selected_provider: String,
    /// Draft base URL for the selected provider's LLM endpoint.
    ///
    /// Read-only in the UI for preset providers (derived from the variant).
    pub base_url: String,
    /// Draft API key for the selected provider.
    pub api_key: String,
    /// Draft API style for the selected provider.
    ///
    /// Read-only for preset providers (derived from the variant).
    /// Editable for custom providers via a pick list.
    pub api_style: llm::ApiStyle,
    /// Names of all providers, kept in sync for the picker UI.
    pub provider_names: Vec<String>,
    /// Draft name for a new custom provider being added.
    pub new_provider_name: String,
    /// Transient status message shown after save attempts.
    pub status: Option<SettingsStatus>,
    /// Whether the currently selected provider is a preset.
    ///
    /// Drives UI decisions: base URL read-only, delete hidden, save skips
    /// `from_raw` validation.
    pub selected_is_preset: bool,
    /// Draft app configuration (locale, appearance, enter behavior, tasks).
    pub config: AppConfig,
    /// Per-task draft form values (provider name, model text, token limit text).
    pub task_drafts: TaskDrafts,
    /// Which tasks have the system-prompt default hint expanded.
    pub system_prompt_hints_expanded: BTreeSet<TaskKind>,
    /// Which tasks have the user-prompt default hint expanded.
    pub user_prompt_hints_expanded: BTreeSet<TaskKind>,
    /// Debounce revisions for per-task text-field persistence.
    ///
    /// Each text edit advances the matching task revision. Delayed persistence
    /// tasks only write when their revision still matches the current one.
    task_persist_revisions: TaskPersistRevisions,
}

/// Per-task draft values for the settings UI.
///
/// Each [`TaskKind`] has its own draft provider selection, model text input,
/// and token limit state, mirroring the persisted [`TaskConfig`].
#[derive(Debug, Clone)]
pub struct TaskDrafts {
    pub amplify: TaskDraft,
    pub distill: TaskDraft,
    pub atomize: TaskDraft,
    pub probe: TaskDraft,
}

/// Draft values for a single task's settings in the UI.
#[derive(Debug, Clone)]
pub struct TaskDraft {
    /// Name of the selected provider for this task.
    pub provider: String,
    /// Draft model identifier text input.
    pub model: String,
    /// Current token-limit mode mirrored from the persisted config.
    ///
    /// `UNLIMITED` is controlled by the dedicated checkbox; the text field only
    /// edits a finite numeric value.
    pub token_limit: MaxTokens,
    /// Draft text for the token-limit input. Empty when unlimited.
    pub max_tokens_text: String,
    /// Custom system prompt.
    pub system_prompt: String,
    /// Custom user prompt template.
    pub user_prompt: String,
}

impl TaskDraft {
    /// Create from a persisted [`TaskConfig`].
    pub fn from_config(config: &TaskConfig) -> Self {
        Self {
            provider: config.provider.clone(),
            model: config.model.clone(),
            token_limit: config.token_limit,
            max_tokens_text: if config.token_limit.is_unlimited() {
                String::new()
            } else {
                config.token_limit.raw().to_string()
            },
            system_prompt: config.system_prompt.clone(),
            user_prompt: config.user_prompt.clone(),
        }
    }
}

impl TaskDrafts {
    /// Create from persisted task settings.
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            amplify: TaskDraft::from_config(config.tasks.config(TaskKind::Amplify)),
            distill: TaskDraft::from_config(config.tasks.config(TaskKind::Distill)),
            atomize: TaskDraft::from_config(config.tasks.config(TaskKind::Atomize)),
            probe: TaskDraft::from_config(config.tasks.config(TaskKind::Probe)),
        }
    }

    /// Get an immutable reference to the draft for a specific [`TaskKind`].
    pub fn get(&self, kind: TaskKind) -> &TaskDraft {
        match kind {
            | TaskKind::Amplify => &self.amplify,
            | TaskKind::Distill => &self.distill,
            | TaskKind::Atomize => &self.atomize,
            | TaskKind::Probe => &self.probe,
        }
    }

    /// Get a mutable reference to the draft for a specific [`TaskKind`].
    pub fn get_mut(&mut self, kind: TaskKind) -> &mut TaskDraft {
        match kind {
            | TaskKind::Amplify => &mut self.amplify,
            | TaskKind::Distill => &mut self.distill,
            | TaskKind::Atomize => &mut self.atomize,
            | TaskKind::Probe => &mut self.probe,
        }
    }
}

/// Outcome of the last settings save attempt.
#[derive(Debug, Clone)]
pub enum SettingsStatus {
    /// Config saved and reloaded successfully.
    Saved,
    /// Save or validation failed with an error message.
    Error(String),
}

/// User-facing appearance preference presented by the settings slider.
///
/// This type wraps persisted `Option<bool>` dark-mode semantics with explicit
/// named variants:
/// - [`Self::Light`] => `Some(false)`
/// - [`Self::System`] => `None`
/// - [`Self::Dark`] => `Some(true)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreference {
    /// Always render light appearance.
    Light,
    /// Follow current system appearance and system theme change events.
    System,
    /// Always render dark appearance.
    Dark,
}

impl ThemePreference {
    const LIGHT_SLIDER_VALUE: i32 = 0;
    const SYSTEM_SLIDER_VALUE: i32 = 1;
    const DARK_SLIDER_VALUE: i32 = 2;

    /// Construct preference from persisted dark-mode override.
    fn from_dark_mode(dark_mode: Option<bool>) -> Self {
        match dark_mode {
            | Some(false) => Self::Light,
            | None => Self::System,
            | Some(true) => Self::Dark,
        }
    }

    /// Convert preference into persisted dark-mode override.
    fn as_dark_mode(self) -> Option<bool> {
        match self {
            | Self::Light => Some(false),
            | Self::System => None,
            | Self::Dark => Some(true),
        }
    }

    /// Resolve concrete dark/light rendering using current system appearance.
    fn resolve_dark(self, system_is_dark: bool) -> bool {
        self.as_dark_mode().unwrap_or(system_is_dark)
    }

    /// Slider coordinate representing this preference.
    fn slider_value(self) -> i32 {
        match self {
            | Self::Light => Self::LIGHT_SLIDER_VALUE,
            | Self::System => Self::SYSTEM_SLIDER_VALUE,
            | Self::Dark => Self::DARK_SLIDER_VALUE,
        }
    }

    /// Construct a preference from a slider coordinate.
    fn from_slider_value(value: i32) -> Self {
        match value {
            | Self::LIGHT_SLIDER_VALUE => Self::Light,
            | Self::SYSTEM_SLIDER_VALUE => Self::System,
            | Self::DARK_SLIDER_VALUE => Self::Dark,
            | _ => {
                tracing::error!(value, "invalid appearance slider value; defaulting to system");
                Self::System
            }
        }
    }
}

/// Point-editor behavior for plain Enter on one-line points.
///
/// This setting does not affect shortcut chords:
/// - `Cmd/Ctrl+Enter` still inserts an empty first child.
/// - `Cmd/Ctrl+Shift+Enter` still inserts an empty sibling after the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstLineEnterBehavior {
    /// At line end, plain Enter inserts an empty child at index 0.
    AddChild,
    /// Plain Enter always inserts a newline.
    InsertNewline,
}

impl FirstLineEnterBehavior {
    /// Convert persisted config flag to a concrete behavior variant.
    fn from_flag(first_line_enter_add_child: bool) -> Self {
        if first_line_enter_add_child { Self::AddChild } else { Self::InsertNewline }
    }

    /// Convert behavior variant to persisted config flag.
    fn as_flag(self) -> bool {
        match self {
            | Self::AddChild => true,
            | Self::InsertNewline => false,
        }
    }
}

impl std::fmt::Display for FirstLineEnterBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            | Self::AddChild => t!("settings_enter_behavior_add_child").to_string(),
            | Self::InsertNewline => t!("settings_enter_behavior_newline").to_string(),
        };
        f.write_str(&label)
    }
}

/// Typed locale choice for the settings locale picker.
///
/// This keeps the picker data stable even though the rendered labels are
/// localized at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocaleChoice {
    /// Follow environment/default locale resolution.
    SystemDefault,
    /// Force English (`en-US`).
    EnUs,
    /// Force Simplified Chinese (`zh-CN`).
    ZhCn,
    /// Force Japanese (`ja`).
    Ja,
}

impl LocaleChoice {
    /// All locale choices shown in the picker, in UI order.
    const ALL: [Self; 4] = [Self::SystemDefault, Self::EnUs, Self::ZhCn, Self::Ja];

    /// Convert persisted config locale text into a typed picker value.
    fn from_config_locale(locale: Option<&str>) -> Self {
        match locale {
            | Some("en-US") => Self::EnUs,
            | Some("zh-CN") => Self::ZhCn,
            | Some("ja") => Self::Ja,
            | Some(other) => {
                tracing::warn!(
                    locale = other,
                    "unknown locale in settings config; using system default"
                );
                Self::SystemDefault
            }
            | None => Self::SystemDefault,
        }
    }

    /// Convert the picker value into the persisted locale override.
    fn into_config_locale(self) -> Option<String> {
        match self {
            | Self::SystemDefault => None,
            | Self::EnUs => Some("en-US".to_string()),
            | Self::ZhCn => Some("zh-CN".to_string()),
            | Self::Ja => Some("ja".to_string()),
        }
    }
}

impl fmt::Display for LocaleChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            | Self::SystemDefault => t!("settings_system_default").to_string(),
            | Self::EnUs => t!("settings_locale_en_us").to_string(),
            | Self::ZhCn => t!("settings_locale_zh_cn").to_string(),
            | Self::Ja => t!("settings_locale_ja").to_string(),
        };
        f.write_str(&label)
    }
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
    /// Draft API style changed (only effective for custom providers).
    ApiStyleChanged(llm::ApiStyle),
    /// Persist draft provider values (URL + API key + API style) to the TOML config file.
    Save,
    /// Update appearance mode via the three-state slider.
    SetThemePreference(ThemePreference),
    /// Update plain Enter behavior for one-line points.
    SetFirstLineEnterBehavior(FirstLineEnterBehavior),
    /// Change the locale override.
    SetLocale(Option<String>),
    /// Copy a resolved settings path to the system clipboard.
    CopyPath(String),
    /// Change the provider selection for a specific [`TaskKind`].
    ///
    /// Immediately persisted to `app.toml`.
    TaskProviderChanged(TaskKind, String),
    /// Change the model text for a specific [`TaskKind`].
    ///
    /// Persisted to `app.toml` after a short debounce.
    TaskModelChanged(TaskKind, String),
    /// Update the max-tokens text input for a specific [`TaskKind`].
    ///
    /// Valid finite values are persisted after a short debounce.
    MaxTokensChanged(TaskKind, String),
    /// Toggle the "unlimited" checkbox for a specific [`TaskKind`].
    ToggleMaxTokensUnlimited(TaskKind, bool),
    /// Change the custom system prompt for a specific [`TaskKind`].
    ///
    /// Persisted to `app.toml` after a short debounce.
    TaskSystemPromptChanged(TaskKind, String),
    /// Change the custom user prompt for a specific [`TaskKind`].
    ///
    /// Persisted to `app.toml` after a short debounce.
    TaskUserPromptChanged(TaskKind, String),
    /// Debounced persistence tick for one task text field.
    PersistTaskDraft(TaskKind, u64),
    /// Toggle expansion of the system-prompt default hint for a task.
    ToggleSystemPromptHintExpanded(TaskKind),
    /// Toggle expansion of the user-prompt default hint for a task.
    ToggleUserPromptHintExpanded(TaskKind),
}

impl SettingsState {
    /// Initialize draft values from the current provider collection and app config.
    ///
    /// Selects the first provider's fields for initial editing.
    pub fn from_providers(providers: &llm::LlmProviders, config: &AppConfig) -> Self {
        let names = providers.provider_names();
        let selected = names.first().cloned().unwrap_or_default();
        let (base_url, api_key, api_style) = providers.raw_fields(&selected).unwrap_or_default();
        let selected_is_preset = providers.is_preset(&selected);
        Self {
            selected_provider: selected,
            base_url,
            api_key,
            api_style,
            provider_names: names,
            new_provider_name: String::new(),
            status: None,
            selected_is_preset,
            task_drafts: TaskDrafts::from_config(config),
            config: config.clone(),
            system_prompt_hints_expanded: BTreeSet::new(),
            user_prompt_hints_expanded: BTreeSet::new(),
            task_persist_revisions: TaskPersistRevisions::default(),
        }
    }

    /// Reload draft fields from the provider collection for the currently selected provider.
    fn load_selected_fields(&mut self, providers: &llm::LlmProviders) {
        if let Some((base_url, api_key, api_style)) = providers.raw_fields(&self.selected_provider)
        {
            self.base_url = base_url;
            self.api_key = api_key;
            self.api_style = api_style;
        }
        self.selected_is_preset = providers.is_preset(&self.selected_provider);
        self.provider_names = providers.provider_names();
    }

    /// Format a [`MaxTokens`] value for the text input field.
    ///
    /// Returns an empty string for unlimited (the checkbox handles that state),
    /// otherwise the raw numeric value as a string.
    fn max_tokens_display_text(mt: MaxTokens) -> String {
        if mt.is_unlimited() { String::new() } else { mt.raw().to_string() }
    }

    /// Toggle whether the system-prompt default hint is expanded for one task.
    fn toggle_system_prompt_hint(&mut self, kind: TaskKind) {
        if !self.system_prompt_hints_expanded.remove(&kind) {
            self.system_prompt_hints_expanded.insert(kind);
        }
    }

    /// Toggle whether the user-prompt default hint is expanded for one task.
    fn toggle_user_prompt_hint(&mut self, kind: TaskKind) {
        if !self.user_prompt_hints_expanded.remove(&kind) {
            self.user_prompt_hints_expanded.insert(kind);
        }
    }

    /// Advance the debounce revision for one task text field.
    fn bump_task_persist_revision(&mut self, kind: TaskKind) -> u64 {
        self.task_persist_revisions.bump(kind)
    }

    /// Whether a delayed persist task is still current for `kind`.
    fn is_current_task_persist_revision(&self, kind: TaskKind, revision: u64) -> bool {
        self.task_persist_revisions.is_current(kind, revision)
    }
}

/// Per-task debounce revision counters for delayed settings persistence.
#[derive(Debug, Clone, Default)]
struct TaskPersistRevisions {
    amplify: u64,
    distill: u64,
    atomize: u64,
    probe: u64,
}

impl TaskPersistRevisions {
    /// Advance and return the revision for one task.
    fn bump(&mut self, kind: TaskKind) -> u64 {
        let revision = self.revision_mut(kind);
        *revision = revision.wrapping_add(1);
        *revision
    }

    /// Whether `revision` still matches the current value for `kind`.
    fn is_current(&self, kind: TaskKind, revision: u64) -> bool {
        self.revision(kind) == revision
    }

    /// Borrow the current revision for one task.
    fn revision(&self, kind: TaskKind) -> u64 {
        match kind {
            | TaskKind::Amplify => self.amplify,
            | TaskKind::Distill => self.distill,
            | TaskKind::Atomize => self.atomize,
            | TaskKind::Probe => self.probe,
        }
    }

    /// Mutably borrow the current revision for one task.
    fn revision_mut(&mut self, kind: TaskKind) -> &mut u64 {
        match kind {
            | TaskKind::Amplify => &mut self.amplify,
            | TaskKind::Distill => &mut self.distill,
            | TaskKind::Atomize => &mut self.atomize,
            | TaskKind::Probe => &mut self.probe,
        }
    }
}

/// Mutable access to one task's draft and both config mirrors.
///
/// The settings screen keeps the live app config and the settings-screen draft
/// config in sync so the view re-renders immediately while persistence still
/// writes through the canonical app config object.
struct TaskSettingsBinding<'a> {
    draft: &'a mut TaskDraft,
    app_config: &'a mut TaskConfig,
    settings_config: &'a mut TaskConfig,
}

impl<'a> TaskSettingsBinding<'a> {
    /// Apply one mutation to the draft and a mirrored mutation to both configs.
    fn update(
        &mut self, update_draft: impl FnOnce(&mut TaskDraft),
        update_config: impl Fn(&mut TaskConfig),
    ) {
        update_draft(self.draft);
        update_config(self.app_config);
        update_config(self.settings_config);
    }
}

/// Shared settings persistence helper.
///
/// Note: settings writes use the top-level app config file only. Provider
/// settings are persisted separately in `llm.toml`.
struct SettingsPersistence;

impl SettingsPersistence {
    /// Persist `app.toml`, updating UI error state on failure.
    fn save_app_config(state: &mut AppState, failure_log: &'static str) -> bool {
        if let Err(err) = state.save_app_config() {
            state.settings.status = Some(SettingsStatus::Error(
                t!("error_config_save_failed", error = err.to_string()).to_string(),
            ));
            tracing::error!(%err, message = failure_log);
            false
        } else {
            true
        }
    }
}

impl AppState {
    /// Borrow one task's draft and both config mirrors for the settings screen.
    fn settings_task_binding(&mut self, kind: TaskKind) -> TaskSettingsBinding<'_> {
        let settings = &mut self.settings;
        let draft = settings.task_drafts.get_mut(kind);
        let settings_config = settings.config.tasks.config_mut(kind);
        let app_config = self.config.tasks.config_mut(kind);
        TaskSettingsBinding { draft, app_config, settings_config }
    }
}

/// Debounced persistence scheduler for task text fields in settings.
struct TaskSettingsPersistenceScheduler;

impl TaskSettingsPersistenceScheduler {
    /// Schedule a delayed `app.toml` write for one task text field.
    fn schedule(state: &mut AppState, kind: TaskKind) -> Task<Message> {
        let revision = state.settings.bump_task_persist_revision(kind);
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(TASK_SETTINGS_PERSIST_DEBOUNCE_MS)).await;
                (kind, revision)
            },
            |(kind, revision)| Message::Settings(SettingsMessage::PersistTaskDraft(kind, revision)),
        )
    }
}

/// Handle a settings message, returning any follow-up task.
pub fn handle(state: &mut AppState, message: SettingsMessage) -> Task<Message> {
    match message {
        | SettingsMessage::Open => {
            state.settings = SettingsState::from_providers(&state.providers, &state.config);
            state.ui_mut().active_view = ViewMode::Settings;
            tracing::info!("settings view opened");
            Task::none()
        }
        | SettingsMessage::Close => {
            let _ = SettingsPersistence::save_app_config(
                state,
                "failed to persist task settings while closing settings view",
            );
            state.ui_mut().active_view = ViewMode::Document;
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
        | SettingsMessage::AddProvider => {
            let name = state.settings.new_provider_name.trim().to_string();
            if name.is_empty() {
                state.settings.status =
                    Some(SettingsStatus::Error(t!("error_provider_name_empty").to_string()));
                return Task::none();
            }
            if state.providers.provider_exists(&name) {
                state.settings.status = Some(SettingsStatus::Error(
                    t!("error_provider_already_exists", name = name).to_string(),
                ));
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
                        state.settings.selected_provider =
                            state.providers.provider_names().first().cloned().unwrap_or_default();
                    }
                    state.settings.load_selected_fields(&state.providers);
                    state.settings.status = None;
                    if let Err(err) = state.providers.save_to_file() {
                        state.settings.status = Some(SettingsStatus::Error(
                            t!("error_save_failed", error = err.to_string()).to_string(),
                        ));
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
        | SettingsMessage::ApiStyleChanged(style) => {
            state.settings.api_style = style;
            state.settings.status = None;
            Task::none()
        }
        | SettingsMessage::Save => {
            let provider_name = state.settings.selected_provider.clone();
            if state.providers.is_preset(&provider_name) {
                // Preset: save api_key directly, no from_raw validation.
                // The user may save an empty api_key (not yet configured).
                let preset = llm::PresetProvider::from_name(&provider_name)
                    .expect("is_preset returned true");
                let config = llm::PresetConfig { api_key: state.settings.api_key.clone() };
                state.providers.update_preset(preset, config);
            } else {
                // Custom: validate URL + API key before saving.
                // We use a dummy model for validation since model lives in per-task config.
                let custom = llm::CustomProvider {
                    base_url: state.settings.base_url.clone(),
                    api_key: state.settings.api_key.clone(),
                    api_style: state.settings.api_style,
                };
                // Validate base_url is https and api_key is non-empty.
                if let Err(err) = llm::LlmConfig::from_raw(
                    custom.base_url.clone(),
                    custom.api_key.clone(),
                    "validation-placeholder".to_string(),
                    custom.api_style,
                ) {
                    state.settings.status = Some(SettingsStatus::Error(
                        t!("error_invalid_config", error = err.to_string()).to_string(),
                    ));
                    return Task::none();
                }
                if let Err(err) = state.providers.upsert_custom(provider_name.clone(), custom) {
                    state.settings.status = Some(SettingsStatus::Error(format!("{err}")));
                    return Task::none();
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
                    state.settings.status = Some(SettingsStatus::Error(
                        t!("error_save_failed", error = err.to_string()).to_string(),
                    ));
                    tracing::error!(%err, "failed to save provider config");
                }
            }
            Task::none()
        }
        | SettingsMessage::SetThemePreference(preference) => {
            let dark_mode = preference.as_dark_mode();
            let system_is_dark = matches!(dark_light::detect(), Ok(dark_light::Mode::Dark));
            let is_dark = preference.resolve_dark(system_is_dark);
            state.ui_mut().is_dark = is_dark;
            state.config.dark_mode = dark_mode;
            state.settings.config.dark_mode = dark_mode;
            if SettingsPersistence::save_app_config(
                state,
                "failed to persist appearance mode preference",
            ) {
                tracing::info!(?preference, is_dark, "appearance mode changed and persisted");
            }
            Task::none()
        }
        | SettingsMessage::SetFirstLineEnterBehavior(behavior) => {
            let first_line_enter_add_child = behavior.as_flag();
            state.config.first_line_enter_add_child = first_line_enter_add_child;
            state.settings.config.first_line_enter_add_child = first_line_enter_add_child;
            if SettingsPersistence::save_app_config(
                state,
                "failed to persist first-line enter behavior setting",
            ) {
                tracing::info!(?behavior, "first-line enter behavior changed and persisted");
            }
            Task::none()
        }
        | SettingsMessage::SetLocale(locale) => {
            // Update both the main config and settings config so effective_locale()
            // returns the new locale for immediate UI re-render.
            state.config.locale = locale.clone();
            state.settings.config.locale = locale.clone();
            // Save config to disk.
            if SettingsPersistence::save_app_config(state, "failed to save app config") {
                // Apply the new locale immediately for the current session.
                let effective = i18n::resolved_locale_from_config(&state.config);
                i18n::set_app_locale(&effective);
                tracing::info!(locale = %effective, "locale changed from settings");
            }
            Task::none()
        }
        | SettingsMessage::CopyPath(path) => {
            tracing::info!(path = %path, "copied settings path to clipboard");
            iced::clipboard::write(path)
        }
        | SettingsMessage::TaskProviderChanged(kind, provider) => {
            state.settings_task_binding(kind).update(
                |draft| draft.provider = provider.clone(),
                |task_config| task_config.provider = provider.clone(),
            );
            if SettingsPersistence::save_app_config(state, "failed to persist task provider change")
            {
                tracing::info!(?kind, %provider, "task provider changed and persisted");
            }
            Task::none()
        }
        | SettingsMessage::TaskModelChanged(kind, model) => {
            state.settings_task_binding(kind).update(
                |draft| draft.model = model.clone(),
                |task_config| task_config.model = model.clone(),
            );
            tracing::debug!(?kind, "scheduled debounced task model persistence");
            TaskSettingsPersistenceScheduler::schedule(state, kind)
        }
        | SettingsMessage::MaxTokensChanged(kind, value) => {
            // Update the draft text field regardless of validity.
            state.settings.task_drafts.get_mut(kind).max_tokens_text = value.clone();
            if let Ok(n) = value.parse::<u32>() {
                if n > 0 {
                    let mt = MaxTokens::new(n);
                    state.settings_task_binding(kind).update(
                        |draft| draft.token_limit = mt,
                        |task_config| task_config.token_limit = mt,
                    );
                    tracing::debug!(?kind, n, "scheduled debounced token-limit persistence");
                    return TaskSettingsPersistenceScheduler::schedule(state, kind);
                }
            }
            let _ = state.settings.bump_task_persist_revision(kind);
            Task::none()
        }
        | SettingsMessage::ToggleMaxTokensUnlimited(kind, unlimited) => {
            let mt = if unlimited {
                MaxTokens::UNLIMITED
            } else {
                // Switch to a sensible non-zero cap so the checkbox
                // visually unchecks and the user can adjust from there.
                MaxTokens::FALLBACK_LIMIT
            };
            state.settings_task_binding(kind).update(
                |draft| {
                    draft.token_limit = mt;
                    draft.max_tokens_text = SettingsState::max_tokens_display_text(mt);
                },
                |task_config| task_config.token_limit = mt,
            );
            if SettingsPersistence::save_app_config(state, "failed to persist token limit toggle") {
                tracing::info!(?kind, unlimited, "token limit unlimited toggled and persisted");
            }
            Task::none()
        }
        | SettingsMessage::TaskSystemPromptChanged(kind, prompt) => {
            state.settings_task_binding(kind).update(
                |draft| draft.system_prompt = prompt.clone(),
                |task_config| task_config.system_prompt = prompt.clone(),
            );
            tracing::debug!(?kind, "scheduled debounced system-prompt persistence");
            TaskSettingsPersistenceScheduler::schedule(state, kind)
        }
        | SettingsMessage::TaskUserPromptChanged(kind, prompt) => {
            state.settings_task_binding(kind).update(
                |draft| draft.user_prompt = prompt.clone(),
                |task_config| task_config.user_prompt = prompt.clone(),
            );
            tracing::debug!(?kind, "scheduled debounced user-prompt persistence");
            TaskSettingsPersistenceScheduler::schedule(state, kind)
        }
        | SettingsMessage::PersistTaskDraft(kind, revision) => {
            if !state.settings.is_current_task_persist_revision(kind, revision) {
                tracing::debug!(?kind, revision, "ignored stale debounced task-settings persist");
                return Task::none();
            }
            if SettingsPersistence::save_app_config(
                state,
                "failed to persist debounced task settings",
            ) {
                tracing::info!(?kind, revision, "persisted debounced task settings");
            }
            Task::none()
        }
        | SettingsMessage::ToggleSystemPromptHintExpanded(kind) => {
            state.settings.toggle_system_prompt_hint(kind);
            Task::none()
        }
        | SettingsMessage::ToggleUserPromptHintExpanded(kind) => {
            state.settings.toggle_user_prompt_hint(kind);
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
    let palette = if state.ui().is_dark { &theme::DARK } else { &theme::LIGHT };

    // ── Header ───────────────────────────────────────────────────────
    let back_button = IconButton::action(
        lucide_icons::iced::icon_arrow_left()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
    )
    .on_press(Message::Settings(SettingsMessage::Close));

    let header = row![
        back_button,
        text(t!("settings_title").to_string()).size(theme::PAGE_TITLE_SIZE).font(theme::INTER),
    ]
    .spacing(theme::FORM_SECTION_GAP)
    .align_y(iced::Alignment::Center);

    // ── Provider selector section ────────────────────────────────────
    let provider_picker = pick_list(
        settings.provider_names.clone(),
        Some(settings.selected_provider.clone()),
        |name| Message::Settings(SettingsMessage::SelectProvider(name)),
    )
    .text_size(theme::INPUT_TEXT_SIZE)
    .padding(theme::PANEL_PAD_V);

    let new_provider_placeholder = t!("settings_new_provider_placeholder").to_string();
    let new_provider_input = text_input(&new_provider_placeholder, &settings.new_provider_name)
        .on_input(|v| Message::Settings(SettingsMessage::NewProviderNameChanged(v)))
        .size(theme::INPUT_TEXT_SIZE)
        .padding(theme::PANEL_PAD_V);

    let add_button = TextButton::action(t!("settings_add").to_string(), theme::LABEL_TEXT_SIZE)
        .on_press(Message::Settings(SettingsMessage::AddProvider))
        .padding(
            iced::Padding::new(theme::COMPACT_PAD_V)
                .left(theme::FORM_ROW_GAP)
                .right(theme::FORM_ROW_GAP),
        );

    let provider_row = row![
        provider_picker.width(Length::FillPortion(2)),
        new_provider_input.width(Length::FillPortion(2)),
        add_button,
    ]
    .spacing(theme::FORM_ROW_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    let mut provider_management = column![provider_row].spacing(theme::FORM_ROW_GAP);

    // Only custom providers can be deleted (presets are always available).
    if !settings.selected_is_preset {
        let delete_btn = TextButton::action_with_color(
            t!("settings_delete_provider").to_string(),
            theme::SMALL_TEXT_SIZE,
            palette.danger,
        )
        .on_press(Message::Settings(SettingsMessage::DeleteProvider(
            settings.selected_provider.clone(),
        )))
        .padding(
            iced::Padding::new(theme::INLINE_GAP)
                .left(theme::COMPACT_PAD_H)
                .right(theme::COMPACT_PAD_H),
        );
        provider_management = provider_management.push(delete_btn);
    }

    // ── System Settings section ─────────────────────────────────────
    let language_label =
        text(t!("settings_language").to_string()).size(theme::INPUT_TEXT_SIZE).font(theme::INTER);
    let locale_choices = LocaleChoice::ALL.to_vec();
    let selected_locale = LocaleChoice::from_config_locale(state.settings.config.locale.as_deref());
    let locale_picker = pick_list(locale_choices, Some(selected_locale), |choice| {
        Message::Settings(SettingsMessage::SetLocale(choice.into_config_locale()))
    })
    .text_size(theme::INPUT_TEXT_SIZE)
    .padding(theme::PANEL_PAD_V);
    let locale_row = row![
        language_label.width(Fill),
        container(locale_picker).align_x(Alignment::from(Horizontal::Right))
    ]
    .spacing(theme::ROW_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    let theme_preference = ThemePreference::from_dark_mode(state.settings.config.dark_mode);
    let appearance_mode_label = text(t!("settings_appearance_mode").to_string())
        .size(theme::INPUT_TEXT_SIZE)
        .font(theme::INTER);
    let appearance_mode_slider = slider(
        ThemePreference::LIGHT_SLIDER_VALUE..=ThemePreference::DARK_SLIDER_VALUE,
        theme_preference.slider_value(),
        |value| {
            Message::Settings(SettingsMessage::SetThemePreference(
                ThemePreference::from_slider_value(value),
            ))
        },
    )
    .width(Length::Fixed(theme::SETTINGS_APPEARANCE_SLIDER_WIDTH))
    .step(1);
    let appearance_mode_labels = row![
        container(text(t!("settings_appearance_light").to_string()).size(theme::SMALL_TEXT_SIZE))
            .width(Fill)
            .align_x(Alignment::from(Horizontal::Left)),
        container(text(t!("settings_appearance_system").to_string()).size(theme::SMALL_TEXT_SIZE))
            .width(Fill)
            .align_x(Alignment::from(Horizontal::Center)),
        container(text(t!("settings_appearance_dark").to_string()).size(theme::SMALL_TEXT_SIZE))
            .width(Fill)
            .align_x(Alignment::from(Horizontal::Right)),
    ]
    .spacing(theme::ROW_GAP)
    .width(Length::Fixed(theme::SETTINGS_APPEARANCE_SLIDER_WIDTH));
    let appearance_mode_control = row![
        appearance_mode_label.width(Fill),
        column![appearance_mode_slider, appearance_mode_labels]
            .width(Fill)
            .align_x(Alignment::from(Horizontal::Right))
    ]
    .spacing(theme::ROW_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    let enter_behavior =
        FirstLineEnterBehavior::from_flag(state.settings.config.first_line_enter_add_child);
    let enter_behavior_options =
        vec![FirstLineEnterBehavior::AddChild, FirstLineEnterBehavior::InsertNewline];
    let enter_behavior_control = row![
        text(t!("settings_first_line_enter_behavior").to_string())
            .size(theme::INPUT_TEXT_SIZE)
            .font(theme::INTER)
            .width(Fill),
        container(
            pick_list(enter_behavior_options, Some(enter_behavior), |behavior| {
                Message::Settings(SettingsMessage::SetFirstLineEnterBehavior(behavior))
            })
            .text_size(theme::INPUT_TEXT_SIZE)
            .padding(theme::PANEL_PAD_V)
        )
        .align_x(Alignment::from(Horizontal::Right)),
    ]
    .spacing(theme::ROW_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    let system_settings_title = t!("settings_system").to_string();
    let system_settings_section = section(
        system_settings_title,
        column![locale_row, appearance_mode_control, enter_behavior_control]
            .spacing(theme::FORM_ROW_GAP)
            .width(Fill),
    );

    // ── Providers section ─────────────────────────────────────
    let provider_section = section(t!("settings_providers").to_string(), provider_management);

    let editing_title =
        t!("settings_configuration", name = settings.selected_provider.as_str()).to_string();

    // For preset providers, base URL is fixed and shown as read-only text.
    // For custom providers, base URL is an editable input field.
    let base_url_label = t!("settings_base_url").to_string();
    let base_url_placeholder = t!("settings_base_url_placeholder").to_string();
    let api_key_label = t!("settings_api_key").to_string();
    let api_key_placeholder = t!("settings_api_key_placeholder").to_string();
    let base_url_field: Element<'_, Message> = if settings.selected_is_preset {
        labeled_readonly(palette, base_url_label, &settings.base_url)
    } else {
        labeled_input(palette, base_url_label, &settings.base_url, base_url_placeholder, |v| {
            Message::Settings(SettingsMessage::BaseUrlChanged(v))
        })
    };

    // API style: read-only for presets, editable pick list for custom providers.
    let api_style_label = t!("settings_api_style").to_string();
    let api_style_field: Element<'_, Message> = if settings.selected_is_preset {
        labeled_readonly(palette, api_style_label, settings.api_style.label())
    } else {
        let current_style = settings.api_style;
        let api_style_options: Vec<String> =
            llm::ApiStyle::ALL.iter().map(|s| s.label().to_string()).collect();
        let api_style_picker =
            pick_list(api_style_options, Some(current_style.label().to_string()), |label| {
                let style = llm::ApiStyle::ALL
                    .iter()
                    .find(|s| s.label() == label)
                    .copied()
                    .unwrap_or_default();
                Message::Settings(SettingsMessage::ApiStyleChanged(style))
            })
            .text_size(theme::INPUT_TEXT_SIZE)
            .padding(theme::PANEL_PAD_V);
        column![
            text(api_style_label)
                .size(theme::LABEL_TEXT_SIZE)
                .font(theme::INTER)
                .color(palette.accent_muted),
            api_style_picker,
        ]
        .spacing(theme::INLINE_GAP)
        .into()
    };

    let api_key_input =
        labeled_input(palette, api_key_label, &settings.api_key, api_key_placeholder, |v| {
            Message::Settings(SettingsMessage::ApiKeyChanged(v))
        });

    let save_button = TextButton::action(t!("settings_save").to_string(), theme::INPUT_TEXT_SIZE)
        .on_press(Message::Settings(SettingsMessage::Save))
        .padding(
            iced::Padding::new(theme::COMPACT_PAD_V)
                .left(theme::PANEL_PAD_H)
                .right(theme::PANEL_PAD_H),
        );

    let mut save_row =
        row![save_button].spacing(theme::FORM_SECTION_GAP).align_y(iced::Alignment::Center);
    if let Some(status) = &settings.status {
        let status_text = match status {
            | SettingsStatus::Saved => text(t!("settings_saved").to_string())
                .size(theme::LABEL_TEXT_SIZE)
                .color(palette.success),
            | SettingsStatus::Error(msg) => {
                text(msg.as_str()).size(theme::LABEL_TEXT_SIZE).color(palette.danger)
            }
        };
        save_row = save_row.push(status_text);
    }

    let config_section = container(
        column![
            text(editing_title).size(theme::SECTION_TITLE_SIZE).font(theme::INTER),
            column![base_url_field, api_style_field, api_key_input, save_row,]
                .spacing(theme::FORM_ROW_GAP),
        ]
        .spacing(theme::FORM_SECTION_GAP),
    )
    .style(theme::draft_panel)
    .padding(
        iced::Padding::new(theme::PANEL_PAD_V).left(theme::PANEL_PAD_H).right(theme::PANEL_PAD_H),
    )
    .width(Fill);

    // ── Per-task LLM settings ────────────────────────────────────────
    let task_section_amplify = task_settings_section(
        palette,
        t!("settings_task_amplify").to_string(),
        TaskKind::Amplify,
        settings.task_drafts.get(TaskKind::Amplify),
        &settings.provider_names,
        settings.system_prompt_hints_expanded.contains(&TaskKind::Amplify),
        settings.user_prompt_hints_expanded.contains(&TaskKind::Amplify),
    );
    let task_section_distill = task_settings_section(
        palette,
        t!("settings_task_distill").to_string(),
        TaskKind::Distill,
        settings.task_drafts.get(TaskKind::Distill),
        &settings.provider_names,
        settings.system_prompt_hints_expanded.contains(&TaskKind::Distill),
        settings.user_prompt_hints_expanded.contains(&TaskKind::Distill),
    );
    let task_section_atomize = task_settings_section(
        palette,
        t!("settings_task_atomize").to_string(),
        TaskKind::Atomize,
        settings.task_drafts.get(TaskKind::Atomize),
        &settings.provider_names,
        settings.system_prompt_hints_expanded.contains(&TaskKind::Atomize),
        settings.user_prompt_hints_expanded.contains(&TaskKind::Atomize),
    );
    let task_section_probe = task_settings_section(
        palette,
        t!("settings_task_probe").to_string(),
        TaskKind::Probe,
        settings.task_drafts.get(TaskKind::Probe),
        &settings.provider_names,
        settings.system_prompt_hints_expanded.contains(&TaskKind::Probe),
        settings.user_prompt_hints_expanded.contains(&TaskKind::Probe),
    );

    // ── Data Paths section ───────────────────────────────────────────
    let data_path = AppPaths::data_file().map(|p| p.display().to_string());
    let config_path = AppPaths::llm_config().map(|p| p.display().to_string());
    let data_path_display =
        data_path.clone().unwrap_or_else(|| t!("settings_not_available").to_string());
    let config_path_display =
        config_path.clone().unwrap_or_else(|| t!("settings_not_available").to_string());

    let paths_title = t!("settings_data_paths").to_string();
    let data_file_label = t!("settings_data_file").to_string();
    let llm_config_label = t!("settings_llm_config").to_string();
    let copy_path_label = t!("settings_copy_path").to_string();
    let paths_section = section(
        paths_title,
        column![
            path_row(
                palette,
                data_file_label,
                data_path_display,
                copy_path_label.clone(),
                data_path.map(|path| Message::Settings(SettingsMessage::CopyPath(path))),
            ),
            path_row(
                palette,
                llm_config_label,
                config_path_display,
                copy_path_label,
                config_path.map(|path| Message::Settings(SettingsMessage::CopyPath(path))),
            ),
        ]
        .spacing(theme::BLOCK_GAP),
    );

    // ── Assemble ─────────────────────────────────────────────────────
    let max_width = theme::canvas_max_width(state.ui().window_size.width);
    let content = column![
        header,
        system_settings_section,
        provider_section,
        config_section,
        task_section_amplify,
        task_section_distill,
        task_section_atomize,
        task_section_probe,
        paths_section
    ]
    .spacing(theme::PAGE_SECTION_GAP)
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
    palette: &'static theme::Palette, label: String, value: &str, placeholder: String,
    on_input: impl Fn(String) -> Message + 'static,
) -> Element<'static, Message> {
    column![
        text(label).size(theme::LABEL_TEXT_SIZE).font(theme::INTER).color(palette.accent_muted),
        text_input(placeholder.as_str(), value)
            .on_input(on_input)
            .size(theme::INPUT_TEXT_SIZE)
            .padding(theme::PANEL_PAD_V),
    ]
    .spacing(theme::INLINE_GAP)
    .into()
}

/// A labeled read-only text display (used for preset base URLs).
fn labeled_readonly(
    palette: &'static theme::Palette, label: String, value: &str,
) -> Element<'static, Message> {
    column![
        text(label).size(theme::LABEL_TEXT_SIZE).font(theme::INTER).color(palette.accent_muted),
        text(value.to_string()).size(theme::INPUT_TEXT_SIZE),
    ]
    .spacing(theme::INLINE_GAP)
    .into()
}

/// A section with a title and content.
fn section(
    title: String, content: impl Into<Element<'static, Message>>,
) -> Element<'static, Message> {
    container(
        column![text(title).size(theme::SECTION_TITLE_SIZE).font(theme::INTER), content.into(),]
            .spacing(theme::FORM_SECTION_GAP),
    )
    .style(theme::draft_panel)
    .padding(
        iced::Padding::new(theme::PANEL_PAD_V).left(theme::PANEL_PAD_H).right(theme::PANEL_PAD_H),
    )
    .width(Fill)
    .into()
}

/// Icon for a task kind in the settings UI.
/// Mirrors the icons used in the action bar for consistency.
fn task_kind_icon(kind: TaskKind) -> Element<'static, Message> {
    let icon = match kind {
        | TaskKind::Amplify => icons::icon_maximize_2(),
        | TaskKind::Distill => icons::icon_minimize_2(),
        | TaskKind::Atomize => icons::icon_maximize(),
        | TaskKind::Probe => icons::icon_message_circle(),
    };
    icon.size(theme::SECTION_TITLE_SIZE)
        .line_height(iced::widget::text::LineHeight::Relative(1.0))
        .into()
}

/// A per-task settings section with an icon next to the title.
fn task_section(
    kind: TaskKind, title: String, content: impl Into<Element<'static, Message>>,
) -> Element<'static, Message> {
    let title_row =
        row![text(title).size(theme::SECTION_TITLE_SIZE).font(theme::INTER), task_kind_icon(kind),]
            .spacing(theme::TITLE_ICON_GAP)
            .align_y(iced::Alignment::Center);
    container(column![title_row, content.into(),].spacing(theme::FORM_SECTION_GAP))
        .style(theme::draft_panel)
        .padding(
            iced::Padding::new(theme::PANEL_PAD_V)
                .left(theme::PANEL_PAD_H)
                .right(theme::PANEL_PAD_H),
        )
        .width(Fill)
        .into()
}

/// A complete per-task configuration section: provider picker, model input, and token limit.
///
/// Each section lets the user independently select a provider, model,
/// and token limit for one [`TaskKind`].
fn task_settings_section(
    palette: &'static theme::Palette, title: String, kind: TaskKind, draft: &TaskDraft,
    provider_names: &[String], system_hint_expanded: bool, user_hint_expanded: bool,
) -> Element<'static, Message> {
    let provider_label = t!("settings_task_provider").to_string();
    let model_label = t!("settings_model").to_string();
    let model_placeholder = t!("settings_model_placeholder").to_string();
    let token_label = t!("settings_task_token_limit").to_string();

    // Provider picker for this task.
    let provider_picker =
        pick_list(provider_names.to_vec(), Some(draft.provider.clone()), move |name| {
            Message::Settings(SettingsMessage::TaskProviderChanged(kind, name))
        })
        .text_size(theme::INPUT_TEXT_SIZE)
        .padding(theme::PANEL_PAD_V);

    let provider_row = row![
        text(provider_label)
            .size(theme::LABEL_TEXT_SIZE)
            .font(theme::INTER)
            .color(palette.accent_muted)
            .width(Fill),
        provider_picker.width(Length::FillPortion(3))
    ]
    .spacing(theme::FORM_SECTION_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    // Model text input for this task.
    let model_input = text_input(&model_placeholder, &draft.model)
        .on_input(move |v| Message::Settings(SettingsMessage::TaskModelChanged(kind, v)))
        .size(theme::INPUT_TEXT_SIZE)
        .padding(theme::PANEL_PAD_V);

    let model_row = row![
        text(model_label)
            .size(theme::LABEL_TEXT_SIZE)
            .font(theme::INTER)
            .color(palette.accent_muted)
            .width(Fill),
        model_input.width(Length::FillPortion(3))
    ]
    .spacing(theme::FORM_SECTION_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    // Token limit input + unlimited checkbox.
    let is_unlimited = draft.token_limit.is_unlimited();
    let input_field: Element<'static, Message> = if is_unlimited {
        text_input("", "")
            .size(theme::INPUT_TEXT_SIZE)
            .padding(theme::PANEL_PAD_V)
            .width(Length::Fixed(theme::SETTINGS_TOKEN_INPUT_WIDTH))
            .into()
    } else {
        text_input("", &draft.max_tokens_text)
            .on_input(move |v| Message::Settings(SettingsMessage::MaxTokensChanged(kind, v)))
            .size(theme::INPUT_TEXT_SIZE)
            .padding(theme::PANEL_PAD_V)
            .width(Length::Fixed(theme::SETTINGS_TOKEN_INPUT_WIDTH))
            .into()
    };

    let unlimited_label = t!("settings_unlimited").to_string();
    let unlimited_checkbox = checkbox(is_unlimited)
        .label(unlimited_label)
        .on_toggle(move |checked| {
            Message::Settings(SettingsMessage::ToggleMaxTokensUnlimited(kind, checked))
        })
        .size(theme::SECTION_TITLE_SIZE)
        .text_size(theme::LABEL_TEXT_SIZE);

    let token_row = row![
        text(token_label)
            .size(theme::LABEL_TEXT_SIZE)
            .font(theme::INTER)
            .color(palette.accent_muted)
            .width(Fill),
        row![input_field, unlimited_checkbox]
            .spacing(theme::FORM_SECTION_GAP)
            .align_y(iced::Alignment::Center)
    ]
    .spacing(theme::FORM_SECTION_GAP)
    .align_y(iced::Alignment::Center)
    .width(Fill);

    // Custom prompt inputs
    let system_prompt_label = t!("settings_custom_system_prompt").to_string();
    let system_prompt_input_hint = t!("settings_custom_prompt_hint").to_string();
    let system_prompt_input = text_input(&system_prompt_input_hint, &draft.system_prompt)
        .on_input(move |v| Message::Settings(SettingsMessage::TaskSystemPromptChanged(kind, v)))
        .size(theme::INPUT_TEXT_SIZE)
        .padding(theme::PANEL_PAD_V);

    let system_default_hint = llm::default_system_prompt_hint(kind);
    let system_hint_toggle = foldable_hint_row(
        palette,
        &t!("settings_default_label"),
        &system_default_hint,
        system_hint_expanded,
        move || Message::Settings(SettingsMessage::ToggleSystemPromptHintExpanded(kind)),
    );
    let system_prompt_row = column![
        text(system_prompt_label)
            .size(theme::LABEL_TEXT_SIZE)
            .font(theme::INTER)
            .color(palette.accent_muted),
        system_prompt_input,
        system_hint_toggle,
    ]
    .spacing(theme::INLINE_GAP);

    let user_prompt_label = t!("settings_custom_user_prompt").to_string();
    let user_prompt_input_hint = t!("settings_custom_prompt_hint").to_string();
    let user_prompt_input = text_input(&user_prompt_input_hint, &draft.user_prompt)
        .on_input(move |v| Message::Settings(SettingsMessage::TaskUserPromptChanged(kind, v)))
        .size(theme::INPUT_TEXT_SIZE)
        .padding(theme::PANEL_PAD_V);

    let user_default_hint = llm::default_user_prompt_hint(kind);
    let user_hint_toggle = foldable_hint_row(
        palette,
        &t!("settings_default_label"),
        user_default_hint,
        user_hint_expanded,
        move || Message::Settings(SettingsMessage::ToggleUserPromptHintExpanded(kind)),
    );
    let user_prompt_row = column![
        text(user_prompt_label)
            .size(theme::LABEL_TEXT_SIZE)
            .font(theme::INTER)
            .color(palette.accent_muted),
        user_prompt_input,
        user_hint_toggle,
    ]
    .spacing(theme::INLINE_GAP);

    task_section(
        kind,
        title,
        column![provider_row, model_row, token_row, system_prompt_row, user_prompt_row]
            .spacing(theme::FORM_ROW_GAP),
    )
}

/// A foldable row: clickable label with chevron, optional content when expanded.
fn foldable_hint_row<F>(
    palette: &'static theme::Palette, label: &str, content: &str, expanded: bool, on_toggle: F,
) -> Element<'static, Message>
where
    F: Fn() -> Message + 'static,
{
    let chevron: Element<'static, Message> = if expanded {
        icons::icon_chevron_down()
            .size(theme::LABEL_TEXT_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into()
    } else {
        icons::icon_chevron_right()
            .size(theme::LABEL_TEXT_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into()
    };
    let label_text = text(label.to_string())
        .size(theme::LABEL_TEXT_SIZE)
        .font(theme::INTER)
        .color(palette.accent_muted);
    let header =
        row![chevron, label_text].spacing(theme::INLINE_GAP).align_y(iced::Alignment::Center);
    let clickable = button(header).style(theme::action_button).on_press(on_toggle());
    let content_text = text(content.to_string())
        .size(theme::SMALL_TEXT_SIZE)
        .font(theme::INTER)
        .color(palette.accent_muted);
    let col = if expanded && !content.is_empty() {
        column![clickable, content_text].spacing(theme::INLINE_GAP)
    } else {
        column![clickable].spacing(theme::INLINE_GAP)
    };
    col.into()
}

/// A read-only key-value row for path display.
///
/// Optionally appends a copy action button when a concrete path exists.
/// All values are owned so the row can outlive local temporaries.
fn path_row(
    palette: &'static theme::Palette, label: String, path: String, copy_tooltip: String,
    copy_message: Option<Message>,
) -> Element<'static, Message> {
    let label_text = text(label)
        .size(theme::LABEL_TEXT_SIZE)
        .font(theme::INTER)
        .color(palette.accent_muted)
        .width(Length::Fixed(theme::PATH_LABEL_WIDTH));
    let path_text = text(path).size(theme::LABEL_TEXT_SIZE).width(Fill);

    let mut content = row![label_text, path_text]
        .spacing(theme::PANEL_BUTTON_GAP)
        .align_y(iced::Alignment::Center)
        .width(Fill);

    if let Some(message) = copy_message {
        let copy_button = IconButton::action(
            icons::icon_copy()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        )
        .on_press(message);
        let copy_with_tooltip = tooltip(
            copy_button,
            text(copy_tooltip).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP);
        content = content.push(copy_with_tooltip);
    }

    content.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, Message};

    #[test]
    fn theme_preference_roundtrips_dark_mode_override() {
        assert_eq!(ThemePreference::from_dark_mode(Some(false)), ThemePreference::Light);
        assert_eq!(ThemePreference::from_dark_mode(None), ThemePreference::System);
        assert_eq!(ThemePreference::from_dark_mode(Some(true)), ThemePreference::Dark);

        assert_eq!(ThemePreference::Light.as_dark_mode(), Some(false));
        assert_eq!(ThemePreference::System.as_dark_mode(), None);
        assert_eq!(ThemePreference::Dark.as_dark_mode(), Some(true));
    }

    #[test]
    fn theme_preference_slider_mapping_matches_three_positions() {
        assert_eq!(ThemePreference::Light.slider_value(), 0);
        assert_eq!(ThemePreference::System.slider_value(), 1);
        assert_eq!(ThemePreference::Dark.slider_value(), 2);

        assert_eq!(ThemePreference::from_slider_value(0), ThemePreference::Light);
        assert_eq!(ThemePreference::from_slider_value(1), ThemePreference::System);
        assert_eq!(ThemePreference::from_slider_value(2), ThemePreference::Dark);
    }

    #[test]
    fn first_line_enter_behavior_roundtrips_flag() {
        assert_eq!(FirstLineEnterBehavior::from_flag(true), FirstLineEnterBehavior::AddChild);
        assert_eq!(FirstLineEnterBehavior::from_flag(false), FirstLineEnterBehavior::InsertNewline);

        assert!(FirstLineEnterBehavior::AddChild.as_flag());
        assert!(!FirstLineEnterBehavior::InsertNewline.as_flag());
    }

    #[test]
    fn locale_choice_roundtrips_config_locale() {
        assert_eq!(LocaleChoice::from_config_locale(None), LocaleChoice::SystemDefault);
        assert_eq!(LocaleChoice::from_config_locale(Some("en-US")), LocaleChoice::EnUs);
        assert_eq!(LocaleChoice::from_config_locale(Some("zh-CN")), LocaleChoice::ZhCn);
        assert_eq!(LocaleChoice::from_config_locale(Some("ja")), LocaleChoice::Ja);

        assert_eq!(LocaleChoice::SystemDefault.into_config_locale(), None);
        assert_eq!(LocaleChoice::EnUs.into_config_locale().as_deref(), Some("en-US"));
        assert_eq!(LocaleChoice::ZhCn.into_config_locale().as_deref(), Some("zh-CN"));
        assert_eq!(LocaleChoice::Ja.into_config_locale().as_deref(), Some("ja"));
    }

    #[test]
    fn task_persist_revisions_track_current_revision_per_task() {
        let mut revisions = TaskPersistRevisions::default();

        let amplify_revision = revisions.bump(TaskKind::Amplify);
        let distill_revision = revisions.bump(TaskKind::Distill);
        let amplify_next_revision = revisions.bump(TaskKind::Amplify);

        assert!(revisions.is_current(TaskKind::Distill, distill_revision));
        assert!(!revisions.is_current(TaskKind::Amplify, amplify_revision));
        assert!(revisions.is_current(TaskKind::Amplify, amplify_next_revision));
    }

    #[test]
    fn blank_token_text_does_not_flip_task_into_unlimited_mode() {
        let (mut state, _) = AppState::test_state();
        let kind = TaskKind::Amplify;
        let previous_limit = MaxTokens::new(1234);
        state.config.tasks.config_mut(kind).token_limit = previous_limit;
        state.settings = SettingsState::from_providers(&state.providers, &state.config);

        let _ = AppState::update(
            &mut state,
            Message::Settings(SettingsMessage::MaxTokensChanged(kind, String::new())),
        );

        let draft = state.settings.task_drafts.get(kind);
        assert_eq!(draft.max_tokens_text, "");
        assert_eq!(draft.token_limit, previous_limit);
        assert!(!draft.token_limit.is_unlimited());
    }
}
