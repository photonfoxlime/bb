//! Load and save the main block-store file.
//!
//! The main store is persisted as pretty-printed JSON to the application data
//! directory.  On save, expanded mount points are restored to `Mount` nodes and
//! mounted descendants are excluded from the snapshot.

use super::{BlockStore, MountFormat};
use crate::paths::AppPaths;
use std::path::Path;
use std::{fs, io};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreLoadError {
    #[error("application data path is unavailable")]
    PathUnavailable,
    #[error("failed to read block store file {path}: {source}")]
    Read { path: std::path::PathBuf, source: io::Error },
    #[error("failed to parse block store file {path}: {source}")]
    Parse { path: std::path::PathBuf, source: serde_json::Error },
}

impl BlockStore {
    pub fn load() -> Result<Self, StoreLoadError> {
        let Some(path) = AppPaths::data_file() else {
            return Err(StoreLoadError::PathUnavailable);
        };
        Self::load_from_path(&path)
    }

    pub(crate) fn load_from_path(path: &Path) -> Result<Self, StoreLoadError> {
        match fs::read_to_string(path) {
            | Ok(contents) => serde_json::from_str(&contents)
                .map_err(|source| StoreLoadError::Parse { path: path.to_path_buf(), source }),
            | Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            | Err(source) => Err(StoreLoadError::Read { path: path.to_path_buf(), source }),
        }
    }

    /// Persist the main store as pretty-printed JSON to the app data file.
    ///
    /// Snapshot semantics:
    /// - expanded mount points are restored to `Mount { rel_path }`,
    /// - mounted descendants are excluded from the main-file snapshot,
    /// - draft keys are remapped to the compacted key-space,
    /// - serialization is strict (`serde_json` failure aborts save).
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = AppPaths::data_file() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let clean = self.snapshot_for_save();
        let contents = serde_json::to_string_pretty(&clean)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(path, contents)
    }

    /// Save all expanded mount files back to disk.
    ///
    /// For each mount entry, this extracts the live mounted subtree into a
    /// standalone store, preserves nested mounts as `Mount { path }` links,
    /// and writes strict JSON to the mount's canonical path.
    pub fn save_mounts(&self) -> io::Result<()> {
        for (mount_point, entry) in self.mount_table.entries() {
            let sub = self.extract_mount_store(&mount_point, entry);
            if let Some(parent) = entry.path.parent() {
                fs::create_dir_all(parent)?;
            }
            match entry.format {
                | MountFormat::Json => {
                    let json = serde_json::to_string_pretty(&sub)
                        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                    fs::write(&entry.path, json)?;
                }
                | MountFormat::Markdown => {
                    let markdown = Self::render_markdown_mount_store(&sub);
                    fs::write(&entry.path, markdown)?;
                }
            }
        }
        Ok(())
    }
}
