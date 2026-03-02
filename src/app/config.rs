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
use std::{fs, io};

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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: None,
            dark_mode: None,
            first_line_enter_add_child: default_first_line_enter_add_child(),
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
    use super::AppConfig;

    #[test]
    fn resolved_dark_mode_prefers_override() {
        let config =
            AppConfig { locale: None, dark_mode: Some(true), first_line_enter_add_child: true };
        assert!(config.resolved_dark_mode(false));

        let config =
            AppConfig { locale: None, dark_mode: Some(false), first_line_enter_add_child: true };
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
        let config = AppConfig {
            locale: Some("en-US".to_string()),
            dark_mode: None,
            first_line_enter_add_child: true,
        };
        let toml = toml::to_string(&config).expect("serialize app config");
        assert!(!toml.contains("dark_mode"));
        assert!(!toml.contains("first-line-enter-add-child"));
        assert!(toml.contains("locale"));
    }

    #[test]
    fn toml_serializes_first_line_enter_add_child_when_disabled() {
        let config = AppConfig { locale: None, dark_mode: None, first_line_enter_add_child: false };
        let toml = toml::to_string(&config).expect("serialize app config");
        assert!(toml.contains("first-line-enter-add-child"));
    }
}
