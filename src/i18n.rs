//! UI string internationalization via [rust-i18n](https://crates.io/crates/rust-i18n).
//!
//! Translations are loaded from `locales/*.yml` at compile time. Call
//! [`set_app_locale`] at the start of each view so that `t!(...)` uses the
//! correct locale.
//!
//! # Locale resolution
//!
//! At startup the effective locale is, in order: (1) optional persisted locale
//! in `app.toml`, (2) environment (`LANG` / `LC_ALL`), (3) [`DEFAULT_LOCALE`].
//! Use [`resolved_locale`] when loading app state.

use crate::paths::AppPaths;
use crate::AppConfig;
use std::{fs, io};

/// Default UI locale when no persisted locale and no env are available.
pub const DEFAULT_LOCALE: &str = "en-US";

/// Supported UI locales. Used for language picker and validation.
pub const SUPPORTED_LOCALES: &[&str] = &["en-US", "zh-CN", "ja"];

/// Resolve a locale to a supported one, or fallback to default.
pub fn resolve_locale(locale: &str) -> &'static str {
    if let Some(&s) = SUPPORTED_LOCALES.iter().find(|s| **s == locale) {
        return s;
    }
    let lang = locale.split('-').next().unwrap_or(locale);
    for s in SUPPORTED_LOCALES {
        if s.starts_with(lang) {
            return s;
        }
    }
    DEFAULT_LOCALE
}

/// Effective locale at startup: persisted (if set) → env → [`DEFAULT_LOCALE`],
/// then normalized to a supported locale via [`resolve_locale`].
pub fn resolved_locale() -> String {
    let raw = load_persisted_locale()
        .or_else(|| Some(locale_from_env()))
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string());
    resolve_locale(&raw).to_string()
}

/// Load optional persisted locale from `app.toml`. Returns `None` if no file,
/// parse error, or locale is absent/empty.
pub fn load_persisted_locale() -> Option<String> {
    let path = AppPaths::app_config()?;
    let contents = fs::read_to_string(&path).ok()?;
    let config: AppConfig = toml::from_str(&contents).ok()?;
    config
        .locale
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
}

/// Persist optional locale to `app.toml`. Pass `None` to clear and fall back to env/default.
pub fn save_locale(locale: Option<&str>) -> Result<(), SaveLocaleError> {
    let path = match AppPaths::app_config() {
        Some(p) => p,
        None => return Err(SaveLocaleError::NoConfigPath),
    };
    let locale = locale.map(|s| s.trim()).filter(|s| !s.is_empty());
    let config = AppConfig {
        locale: locale.map(String::from),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(SaveLocaleError::CreateDir)?;
    }
    let body = toml::to_string_pretty(&config).map_err(SaveLocaleError::Serialize)?;
    fs::write(&path, body).map_err(SaveLocaleError::Write)?;
    Ok(())
}

/// Error when persisting locale to `app.toml`.
#[derive(Debug, thiserror::Error)]
pub enum SaveLocaleError {
    #[error("no config path available")]
    NoConfigPath,
    #[error("failed to create config directory: {0}")]
    CreateDir(io::Error),
    #[error("failed to serialize config: {0}")]
    Serialize(toml::ser::Error),
    #[error("failed to write config: {0}")]
    Write(io::Error),
}

/// Set the current locale for subsequent `t!(...)` calls. Call this at the
/// start of each view with `state.locale` so all lookups use the right language.
pub fn set_app_locale(locale: &str) {
    rust_i18n::set_locale(resolve_locale(locale));
}

/// Best-effort parse of locale from env (e.g. LANG=zh_CN.UTF-8).
pub fn locale_from_env() -> String {
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_else(|_| "en_US.UTF-8".into());
    raw.split('.').next().unwrap_or(&raw).replace('_', "-")
}
