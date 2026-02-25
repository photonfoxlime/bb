#![doc = include_str!("../README.md")]

mod app;
mod i18n;
mod store;
mod llm;
mod paths;
mod theme;
mod undo;

use serde::{Deserialize, Serialize};

rust_i18n::i18n!("locales", fallback = "en-US");

pub use app::AppState;

/// Persisted app preferences (e.g. optional locale).
///
/// Stored in `<config_dir>/app.toml`; see [`paths::AppPaths::app_config`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    /// Override UI locale; if absent or empty, env then default is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) locale: Option<String>,
}
