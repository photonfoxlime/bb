#![doc = include_str!("../README.md")]
rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
mod i18n;
mod store;
mod llm;
mod paths;
mod theme;
mod undo;

/// Re-exports.
pub use app::AppState;
