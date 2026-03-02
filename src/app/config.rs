//! Persisted application configuration.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Stores optional UI locale, appearance preference, and editor-key behavior in
//! `<config_dir>/app.toml`. Loaded at startup; changes are saved via
//! [`save`] or [`AppState::save_app_config`](crate::app::AppState::save_app_config).

use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{fs, io};

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

/// Per-task-kind token limits persisted in `app.toml`.
///
/// Each field defaults to a sensible value when absent from the file.
/// A value of `0` means unlimited (the `max_completion_tokens` field is
/// omitted from the API request).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenLimits {
    /// Max completion tokens for reduce requests.
    #[serde(default = "default_reduce_tokens")]
    pub reduce: MaxTokens,
    /// Max completion tokens for expand requests.
    #[serde(default = "default_expand_tokens")]
    pub expand: MaxTokens,
    /// Max completion tokens for inquire requests.
    #[serde(default = "default_inquire_tokens")]
    pub inquire: MaxTokens,
}

impl Default for TokenLimits {
    fn default() -> Self {
        Self {
            reduce: default_reduce_tokens(),
            expand: default_expand_tokens(),
            inquire: default_inquire_tokens(),
        }
    }
}

fn default_reduce_tokens() -> MaxTokens {
    MaxTokens(400)
}
fn default_expand_tokens() -> MaxTokens {
    MaxTokens(500)
}
fn default_inquire_tokens() -> MaxTokens {
    MaxTokens(700)
}

/// Persisted app preferences (locale override, optional appearance override,
/// and point-editor Enter behavior).
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
    /// Per-task-kind maximum completion token limits.
    ///
    /// Persisted under `[token-limits]` in `app.toml`. Each field defaults to
    /// a sensible value when absent. A value of `0` means unlimited (the
    /// `max_completion_tokens` field is omitted from the API request).
    #[serde(rename = "token-limits", default)]
    pub token_limits: TokenLimits,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: None,
            dark_mode: None,
            first_line_enter_add_child: default_first_line_enter_add_child(),
            token_limits: TokenLimits::default(),
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
    use super::{AppConfig, MaxTokens, TokenLimits};

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
    fn token_limits_defaults_are_sensible() {
        let limits = TokenLimits::default();
        assert_eq!(limits.reduce.raw(), 400);
        assert_eq!(limits.expand.raw(), 500);
        assert_eq!(limits.inquire.raw(), 700);
    }

    #[test]
    fn token_limits_round_trips_through_toml() {
        let limits = TokenLimits {
            reduce: MaxTokens::new(300),
            expand: MaxTokens::UNLIMITED,
            inquire: MaxTokens::new(1000),
        };
        let config = AppConfig { token_limits: limits.clone(), ..Default::default() };
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.token_limits.reduce.raw(), 300);
        assert_eq!(parsed.token_limits.expand.raw(), 0);
        assert_eq!(parsed.token_limits.inquire.raw(), 1000);
    }

    #[test]
    fn token_limits_missing_from_toml_uses_defaults() {
        let toml_str = r#"locale = "en-US""#;
        let config: AppConfig = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(config.token_limits.reduce.raw(), 400);
        assert_eq!(config.token_limits.expand.raw(), 500);
        assert_eq!(config.token_limits.inquire.raw(), 700);
    }
}
