//! Shared application directory paths.
//!
//! Uses the `directories` crate to resolve platform-appropriate config
//! directories. Block-store data paths are delegated to
//! [`blooming_blockery_store::StorePaths`] so the persistence crate remains the
//! canonical owner of `blocks.json` path resolution.

use blooming_blockery_store::StorePaths;
use std::{path::PathBuf, sync::LazyLock};

static PROJECT_DIRS: LazyLock<Option<directories::ProjectDirs>> =
    LazyLock::new(|| directories::ProjectDirs::from("app", "miorin", "blooming-blockery"));

/// Resolved application paths for data and configuration storage.
///
/// # Invariants
/// - All paths are derived from a single `ProjectDirs` instance.
/// - If `ProjectDirs` cannot be resolved (e.g. no home directory), all
///   path methods return `None`.
pub struct AppPaths;

impl AppPaths {
    /// Path to the block store JSON file: `<data_dir>/blocks.json`.
    pub fn data_file() -> Option<PathBuf> {
        StorePaths::data_file()
    }

    /// Directory containing the main block store, used as the base for
    /// resolving relative mount paths.
    pub fn data_dir() -> Option<PathBuf> {
        StorePaths::data_dir()
    }

    /// Path to the LLM configuration TOML: `<config_dir>/llm.toml`.
    pub fn llm_config() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|p| p.config_dir().join("llm.toml"))
    }

    /// Path to the app preferences TOML: `<config_dir>/app.toml`.
    /// Used for optional persisted locale and other app-level preferences.
    pub fn app_config() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|p| p.config_dir().join("app.toml"))
    }
}
