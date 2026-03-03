//! Persisted application configuration.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Stores optional UI locale, appearance preference, editor-key behavior, and
//! per-task LLM settings in `<config_dir>/app.toml`. Loaded at startup; changes
//! are saved via [`save`] or
//! [`AppState::save_app_config`](crate::app::AppState::save_app_config).
//!
//! # Per-task LLM settings
//!
//! Each LLM task kind (reduce, expand, inquire) independently selects:
//! - **Provider** — name of a preset or custom provider.
//! - **Model** — model identifier sent to the API.
//! - **Token limit** — max completion tokens (0 = unlimited).
//!
//! These live in the `[tasks.*]` TOML tables. Providers themselves (URL + API
//! key) are stored separately in `llm.toml`.

use crate::llm;
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{fs, io};

/// Persisted app preferences (locale override, optional appearance override,
/// editor-key behavior, and per-task LLM settings).
///
/// Stored in `<config_dir>/app.toml`; see [`AppPaths::app_config`].
/// The effective locale is derived via [`crate::i18n::resolved_locale_from_config`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Override UI locale; if absent or empty, env then default is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Override UI dark mode preference.
    ///
    /// - `None`: follow current system appearance and live system theme changes.
    /// - `Some(true)`: force dark appearance.
    /// - `Some(false)`: force light appearance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark_mode: Option<bool>,
    /// Whether plain `Enter` at the end of a one-line point inserts an empty
    /// first child instead of a newline.
    ///
    /// `Cmd/Ctrl+Enter` always inserts a first child independent of this
    /// setting. This preference only affects plain `Enter` on one-line points.
    ///
    /// Persisted key uses kebab-case (`first-line-enter-add-child`).
    /// Snake-case is still accepted as a read alias for backward compatibility.
    #[serde(
        rename = "first-line-enter-add-child",
        alias = "first_line_enter_add_child",
        default = "default_first_line_enter_add_child",
        skip_serializing_if = "is_true"
    )]
    pub first_line_enter_add_child: bool,
    /// Per-task-kind LLM settings (provider, model, token limit).
    ///
    /// Persisted under `[tasks]` in `app.toml`. Each task defaults to
    /// a sensible value when absent.
    #[serde(default)]
    pub tasks: TaskSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: None,
            dark_mode: None,
            first_line_enter_add_child: default_first_line_enter_add_child(),
            tasks: TaskSettings::default(),
        }
    }
}

fn default_first_line_enter_add_child() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

impl AppConfig {
    /// Resolve the effective dark mode for this session.
    ///
    /// Uses the persisted override when present; otherwise falls back to
    /// the caller-provided system appearance value.
    pub fn resolved_dark_mode(&self, system_is_dark: bool) -> bool {
        self.dark_mode.unwrap_or(system_is_dark)
    }
}

/// Load app config from `<config_dir>/app.toml`. Returns default on missing or parse error.
pub fn load() -> AppConfig {
    let path = match AppPaths::app_config() {
        | Some(p) => p,
        | None => return AppConfig::default(),
    };
    let contents = match fs::read_to_string(&path) {
        | Ok(c) => c,
        | Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                tracing::warn!(path = %path.display(), error = %e, "failed to read app config");
            }
            return AppConfig::default();
        }
    };
    toml::from_str(&contents).unwrap_or_else(|e| {
        tracing::warn!(path = %path.display(), error = %e, "failed to parse app config");
        AppConfig::default()
    })
}

/// Persist app config to `<config_dir>/app.toml`. Call when config changes (e.g. locale from settings).
pub fn save(config: &AppConfig) -> Result<(), SaveError> {
    let path = AppPaths::app_config().ok_or(SaveError::NoConfigPath)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(SaveError::CreateDir)?;
    }
    let body = toml::to_string_pretty(config).map_err(SaveError::Serialize)?;
    fs::write(&path, body).map_err(SaveError::Write)?;
    Ok(())
}

/// Maximum completion tokens for a single LLM request.
///
/// Wraps a `u32` where `0` means unlimited (omit `max_completion_tokens` from
/// the API request) and any positive value caps the response length.
///
/// Serialized transparently as a plain integer in TOML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaxTokens(u32);

impl MaxTokens {
    /// Sentinel value: do not send `max_completion_tokens` to the API.
    pub const UNLIMITED: Self = Self(0);

    /// Reasonable non-zero fallback used when the user toggles *off* unlimited
    /// mode and no prior numeric value exists.
    pub const FALLBACK_LIMIT: Self = Self(4096);

    /// Create a new token limit from a raw `u32`.
    ///
    /// `0` is interpreted as unlimited.
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    /// Convert to `Option<u32>` suitable for the API request field.
    ///
    /// Returns `None` when unlimited, `Some(n)` otherwise.
    pub fn as_api_param(self) -> Option<u32> {
        if self.0 == 0 { None } else { Some(self.0) }
    }

    /// Whether this limit is unlimited (zero).
    pub fn is_unlimited(self) -> bool {
        self.0 == 0
    }

    /// Raw numeric value (`0` = unlimited).
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for MaxTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Per-task-kind LLM settings persisted in `app.toml`.
///
/// Each task independently selects a provider, model, and token limit.
/// Uses `[tasks.reduce]`, `[tasks.expand]`, `[tasks.inquire]` TOML tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSettings {
    /// Settings for the reduce task.
    #[serde(default = "TaskConfig::default_reduce")]
    pub reduce: TaskConfig,
    /// Settings for the expand task.
    #[serde(default = "TaskConfig::default_expand")]
    pub expand: TaskConfig,
    /// Settings for the inquire task.
    #[serde(default = "TaskConfig::default_inquire")]
    pub inquire: TaskConfig,
}

impl Default for TaskSettings {
    fn default() -> Self {
        Self {
            reduce: TaskConfig::default_reduce(),
            expand: TaskConfig::default_expand(),
            inquire: TaskConfig::default_inquire(),
        }
    }
}

/// Configuration for a single LLM task (reduce, expand, or inquire).
///
/// Selects which provider to use, which model to request, the
/// maximum number of completion tokens, and optional custom prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskConfig {
    /// Name of the provider (preset or custom) to use for this task.
    #[serde(default = "default_provider_name")]
    pub provider: String,
    /// Model identifier sent to the API.
    #[serde(default = "default_model_name")]
    pub model: String,
    /// Max completion tokens. `0` means unlimited (omit from API request).
    #[serde(rename = "token-limit", default = "default_reduce_tokens")]
    pub token_limit: MaxTokens,
    /// Custom system prompt. If empty, uses the built-in default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub system_prompt: String,
    /// Custom user prompt template. If empty, uses the built-in default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_prompt: String,
}

impl TaskConfig {
    /// Default config for the reduce task.
    pub fn default_reduce() -> Self {
        Self {
            provider: default_provider_name(),
            model: default_model_name(),
            token_limit: default_reduce_tokens(),
            system_prompt: String::new(),
            user_prompt: String::new(),
        }
    }

    /// Default config for the expand task.
    pub fn default_expand() -> Self {
        Self {
            provider: default_provider_name(),
            model: default_model_name(),
            token_limit: default_expand_tokens(),
            system_prompt: String::new(),
            user_prompt: String::new(),
        }
    }

    /// Default config for the inquire task.
    pub fn default_inquire() -> Self {
        Self {
            provider: default_provider_name(),
            model: default_model_name(),
            token_limit: default_inquire_tokens(),
            system_prompt: String::new(),
            user_prompt: String::new(),
        }
    }
}

fn default_provider_name() -> String {
    llm::DEFAULT_PROVIDER.to_string()
}

fn default_model_name() -> String {
    llm::PresetProvider::OpenAI.default_model().to_string()
}

fn default_reduce_tokens() -> MaxTokens {
    MaxTokens::UNLIMITED
}
fn default_expand_tokens() -> MaxTokens {
    MaxTokens::UNLIMITED
}
fn default_inquire_tokens() -> MaxTokens {
    MaxTokens::UNLIMITED
}

/// Error when persisting app config.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("no config path available")]
    NoConfigPath,
    #[error("failed to create config directory: {0}")]
    CreateDir(io::Error),
    #[error("failed to serialize config: {0}")]
    Serialize(toml::ser::Error),
    #[error("failed to write config: {0}")]
    Write(io::Error),
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, MaxTokens, TaskConfig, TaskSettings};

    #[test]
    fn resolved_dark_mode_prefers_override() {
        let config = AppConfig { dark_mode: Some(true), ..Default::default() };
        assert!(config.resolved_dark_mode(false));

        let config = AppConfig { dark_mode: Some(false), ..Default::default() };
        assert!(!config.resolved_dark_mode(true));
    }

    #[test]
    fn resolved_dark_mode_falls_back_to_system_when_unset() {
        let config = AppConfig::default();
        assert!(config.resolved_dark_mode(true));
        assert!(!config.resolved_dark_mode(false));
    }

    #[test]
    fn toml_omits_dark_mode_when_unset() {
        let config = AppConfig { locale: Some("en-US".to_string()), ..Default::default() };
        let toml = toml::to_string(&config).expect("serialize app config");
        assert!(!toml.contains("dark_mode"));
        assert!(!toml.contains("first-line-enter-add-child"));
        assert!(toml.contains("locale"));
    }

    #[test]
    fn toml_serializes_first_line_enter_add_child_when_disabled() {
        let config = AppConfig { first_line_enter_add_child: false, ..Default::default() };
        let toml = toml::to_string(&config).expect("serialize app config");
        assert!(toml.contains("first-line-enter-add-child"));
    }

    #[test]
    fn max_tokens_unlimited_omits_api_param() {
        assert_eq!(MaxTokens::UNLIMITED.as_api_param(), None);
        assert!(MaxTokens::UNLIMITED.is_unlimited());
    }

    #[test]
    fn max_tokens_positive_maps_to_some() {
        let mt = MaxTokens::new(400);
        assert_eq!(mt.as_api_param(), Some(400));
        assert!(!mt.is_unlimited());
        assert_eq!(mt.raw(), 400);
    }

    #[test]
    fn task_settings_defaults_are_sensible() {
        let tasks = TaskSettings::default();
        assert!(tasks.reduce.token_limit.is_unlimited());
        assert!(tasks.expand.token_limit.is_unlimited());
        assert!(tasks.inquire.token_limit.is_unlimited());
        assert_eq!(tasks.reduce.provider, "openai");
        assert_eq!(tasks.reduce.model, "gpt-4o");
    }

    #[test]
    fn task_settings_round_trips_through_toml() {
        let tasks = TaskSettings {
            reduce: TaskConfig {
                provider: "deepseek".to_string(),
                model: "deepseek-chat".to_string(),
                token_limit: MaxTokens::new(300),
                system_prompt: String::new(),
                user_prompt: String::new(),
            },
            expand: TaskConfig {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                token_limit: MaxTokens::UNLIMITED,
                system_prompt: String::new(),
                user_prompt: String::new(),
            },
            inquire: TaskConfig::default_inquire(),
        };
        let config = AppConfig { tasks: tasks.clone(), ..Default::default() };
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.tasks.reduce.provider, "deepseek");
        assert_eq!(parsed.tasks.reduce.model, "deepseek-chat");
        assert_eq!(parsed.tasks.reduce.token_limit.raw(), 300);
        assert_eq!(parsed.tasks.expand.token_limit.raw(), 0);
        assert_eq!(parsed.tasks.inquire.token_limit.raw(), 0);
    }

    #[test]
    fn task_settings_missing_from_toml_uses_defaults() {
        let toml_str = r#"locale = "en-US""#;
        let config: AppConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(config.tasks.reduce.token_limit.is_unlimited());
        assert!(config.tasks.expand.token_limit.is_unlimited());
        assert!(config.tasks.inquire.token_limit.is_unlimited());
        assert_eq!(config.tasks.reduce.provider, "openai");
    }
}
