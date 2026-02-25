//! UI string internationalization via [rust-i18n](https://crates.io/crates/rust-i18n).
//!
//! # Design
//!
//! - **Resolution**: Effective locale is, in order: (1) optional locale in
//!   [`AppConfig`], (2) environment (`LANG` / `LC_ALL`), (3) [`DEFAULT_LOCALE`].
//!   The result is normalized via [`resolve_locale`]. Use [`resolved_locale_from_config`]
//!   with the app's config to get the locale for the session.
//! - **Persistence**: [`AppConfig`] is loaded and saved by the app layer (see
//!   `AppState::load` / app config save). This module does not perform I/O.
//! - **View contract**: Call [`set_app_locale`] once at the start of each view
//!   with the effective locale (e.g. from `state.effective_locale()`) so all
//!   `t!(...)` lookups use the same language.
//!
//! Translations are loaded from `locales/*.yml` at compile time. The `i18n!`
//! macro is in `lib.rs` so the whole crate shares one loader and `t!(...)`
//! resolves in all UI modules.

use crate::config::AppConfig;

/// Default UI locale when no config locale and no env are available.
pub const DEFAULT_LOCALE: &str = "en-US";

/// Supported UI locales. Used for language picker and validation.
pub const SUPPORTED_LOCALES: &[&str] = &["en-US", "zh-CN", "ja"];

/// Resolve a locale to a supported one. Exact match first, then language prefix
/// (e.g. `zh` → `zh-CN`), otherwise [`DEFAULT_LOCALE`].
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

/// Effective locale from config: config.locale (if set) → env → [`DEFAULT_LOCALE`],
/// then normalized via [`resolve_locale`]. Use this with the app's loaded config.
pub fn resolved_locale_from_config(config: &AppConfig) -> String {
    let raw = config
        .locale
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .or_else(|| Some(locale_from_env()))
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string());
    resolve_locale(&raw).to_string()
}

/// Set the current locale for subsequent `t!(...)` calls. Call once at the
/// start of each view with the effective locale so all lookups use the right language.
pub fn set_app_locale(locale: &str) {
    rust_i18n::set_locale(resolve_locale(locale));
}

/// Best-effort locale from env. Reads `LANG` or `LC_ALL` (e.g. `zh_CN.UTF-8`),
/// strips encoding and normalizes `_` to `-`.
pub fn locale_from_env() -> String {
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_else(|_| "en_US.UTF-8".into());
    raw.split('.').next().unwrap_or(&raw).replace('_', "-")
}
