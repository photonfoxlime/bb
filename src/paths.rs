//! Shared application directory paths.
//!
//! Uses the `directories` crate to resolve platform-appropriate data and config
//! directories for the `bb` application.

use std::{path::PathBuf, sync::LazyLock};

static PROJECT_DIRS: LazyLock<Option<directories::ProjectDirs>> =
    LazyLock::new(|| directories::ProjectDirs::from("app", "miorin", "bb"));

/// Resolved application paths for data and configuration storage.
///
/// Invariant: all paths are derived from a single `ProjectDirs` instance.
/// If `ProjectDirs` cannot be resolved (e.g. no home directory), all
/// path methods return `None`.
pub struct AppPaths;

impl AppPaths {
    /// Path to the block graph JSON file: `<data_dir>/blocks.json`.
    pub fn data_file() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|p| p.data_dir().join("blocks.json"))
    }

    /// Path to the LLM configuration TOML: `<config_dir>/llm.toml`.
    pub fn llm_config() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|p| p.config_dir().join("llm.toml"))
    }
}
