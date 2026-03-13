//! Store-specific filesystem paths.
//!
//! This crate resolves only the paths needed for the persisted block store.
//! App and LLM configuration paths remain owned by the application crate.

use std::{path::PathBuf, sync::LazyLock};

static PROJECT_DIRS: LazyLock<Option<directories::ProjectDirs>> =
    LazyLock::new(|| directories::ProjectDirs::from("app", "miorin", "blooming-blockery"));

/// Resolved filesystem locations for the persisted block store.
///
/// # Invariants
/// - All paths are derived from a single `ProjectDirs` lookup.
/// - If `ProjectDirs` is unavailable, all methods return `None`.
pub struct StorePaths;

impl StorePaths {
    /// Path to the main store JSON file: `<data_dir>/blocks.json`.
    pub fn data_file() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|dirs| dirs.data_dir().join("blocks.json"))
    }

    /// Directory containing the main block store and the default mount base.
    pub fn data_dir() -> Option<PathBuf> {
        PROJECT_DIRS.as_ref().map(|dirs| dirs.data_dir().to_path_buf())
    }
}
