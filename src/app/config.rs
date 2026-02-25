//! Persisted application configuration.
//!
//! Stores optional UI locale and other app-level preferences in
//! `<config_dir>/app.toml`. Loaded at startup; changes are saved via
//! [`save`] or [`AppState::save_app_config`](crate::app::AppState::save_app_config).

use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::{fs, io};

/// Persisted app preferences (e.g. optional locale).
///
/// Stored in `<config_dir>/app.toml`; see [`AppPaths::app_config`].
/// The effective locale is derived via [`crate::i18n::resolved_locale_from_config`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Override UI locale; if absent or empty, env then default is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
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
