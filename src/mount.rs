//! Mount table: tracks blocks loaded from external files.
//!
//! When a `BlockNode::Mount { path }` is expanded at runtime, the referenced
//! file is deserialized into a `BlockStore`, its blocks are re-keyed into the
//! main store with fresh `BlockId`s, and the mount point is swapped to
//! `BlockNode::Children`. The `MountTable` remembers which blocks came from
//! which file so that edits can be saved back to the originating file.

use crate::store::BlockId;
use slotmap::SecondaryMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MountError {
    #[error("block is not a mount node")]
    NotAMount,
    #[error("unknown block id")]
    UnknownBlock,
    #[error("mount cycle detected: {path} is already mounted")]
    CycleDetected { path: PathBuf },
    #[error("failed to read mount file {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("failed to parse mount file {path}: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
}

/// Identifies which file owns a block.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockOrigin {
    /// Block belongs to the main document file.
    Main,
    /// Block was loaded from an external mounted file.
    Mounted {
        /// The id of the mount-point block whose `BlockNode::Mount` triggered the load.
        mount_point: BlockId,
    },
}

/// Metadata for a single mounted file.
#[derive(Debug, Clone)]
pub struct MountEntry {
    /// Canonical (absolute) path used for cycle detection and save-back.
    pub path: PathBuf,
    /// Original relative path as stored in the `BlockNode::Mount`.
    /// Restored on collapse to preserve the serialization form.
    pub rel_path: PathBuf,
    /// Root block ids of the mounted sub-store (after re-keying).
    pub root_ids: Vec<BlockId>,
    /// All block ids belonging to this mount (roots + descendants).
    pub block_ids: Vec<BlockId>,
}

impl MountEntry {
    pub fn new(
        path: PathBuf, rel_path: PathBuf, root_ids: Vec<BlockId>, block_ids: Vec<BlockId>,
    ) -> Self {
        Self { path, rel_path, root_ids, block_ids }
    }
}

/// Runtime-only table tracking mounted files and block ownership.
///
/// Not serialized: mount state is reconstructed on load by re-expanding
/// `BlockNode::Mount` nodes.
#[derive(Debug, Clone, Default)]
pub struct MountTable {
    /// Per-block origin. Blocks not present are implicitly `BlockOrigin::Main`.
    origins: SecondaryMap<BlockId, BlockOrigin>,
    /// Per-mount-point metadata, keyed by the mount-point block id.
    entries: SecondaryMap<BlockId, MountEntry>,
}

impl MountTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_origin(&mut self, block_id: BlockId, origin: BlockOrigin) {
        self.origins.insert(block_id, origin);
    }

    /// Look up where a block came from. Returns `Main` for unknown ids.
    pub fn origin_of(&self, block_id: BlockId) -> &BlockOrigin {
        self.origins.get(block_id).unwrap_or(&BlockOrigin::Main)
    }

    /// Register a mount entry for a mount-point block.
    pub fn insert_entry(&mut self, mount_point: BlockId, entry: MountEntry) {
        self.entries.insert(mount_point, entry);
    }

    /// Look up the mount entry for a mount-point block.
    pub fn entry(&self, mount_point: BlockId) -> Option<&MountEntry> {
        self.entries.get(mount_point)
    }

    /// Remove the mount entry and all associated origin records.
    ///
    /// Returns the removed entry, or `None` if `mount_point` had no entry.
    pub fn remove_entry(&mut self, mount_point: BlockId) -> Option<MountEntry> {
        let entry = self.entries.remove(mount_point)?;
        for &id in &entry.block_ids {
            self.origins.remove(id);
        }
        Some(entry)
    }

    /// Remove a single block's origin record (e.g. when the block is deleted).
    pub fn remove_origin(&mut self, block_id: BlockId) {
        self.origins.remove(block_id);
    }

    /// Iterate over all mount entries as `(mount_point_id, entry)` pairs.
    pub fn entries(&self) -> impl Iterator<Item = (BlockId, &MountEntry)> {
        self.entries.iter()
    }

    /// Return `true` if `block_id` belongs to a mounted file.
    pub fn is_mounted(&self, block_id: BlockId) -> bool {
        matches!(self.origins.get(block_id), Some(BlockOrigin::Mounted { .. }))
    }

    /// Return `true` if the given canonical path is already mounted.
    ///
    /// Used for cycle detection: prevents mounting a file that is already
    /// expanded somewhere in the tree.
    pub fn is_path_mounted(&self, canonical: &std::path::Path) -> bool {
        self.entries.values().any(|entry| entry.path == canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn make_ids(count: usize) -> Vec<BlockId> {
        let mut sm: SlotMap<BlockId, ()> = SlotMap::with_key();
        (0..count).map(|_| sm.insert(())).collect()
    }

    #[test]
    fn default_origin_is_main() {
        let table = MountTable::new();
        let ids = make_ids(1);
        assert_eq!(table.origin_of(ids[0]), &BlockOrigin::Main);
    }

    #[test]
    fn set_and_query_origin() {
        let mut table = MountTable::new();
        let ids = make_ids(2);
        let origin = BlockOrigin::Mounted { mount_point: ids[0] };
        table.set_origin(ids[1], origin.clone());
        assert_eq!(table.origin_of(ids[1]), &origin);
    }

    #[test]
    fn insert_and_query_entry() {
        let mut table = MountTable::new();
        let ids = make_ids(3);
        let entry = MountEntry::new(
            PathBuf::from("sub.json"),
            PathBuf::from("sub.json"),
            vec![ids[1]],
            vec![ids[1], ids[2]],
        );
        table.insert_entry(ids[0], entry);
        let got = table.entry(ids[0]).unwrap();
        assert_eq!(got.path, PathBuf::from("sub.json"));
        assert_eq!(got.root_ids, vec![ids[1]]);
        assert_eq!(got.block_ids, vec![ids[1], ids[2]]);
    }

    #[test]
    fn remove_entry_clears_origins() {
        let mut table = MountTable::new();
        let ids = make_ids(3);

        let origin = BlockOrigin::Mounted { mount_point: ids[0] };
        table.set_origin(ids[1], origin.clone());
        table.set_origin(ids[2], origin);
        table.insert_entry(
            ids[0],
            MountEntry::new(
                PathBuf::from("x.json"),
                PathBuf::from("x.json"),
                vec![ids[1]],
                vec![ids[1], ids[2]],
            ),
        );

        let removed = table.remove_entry(ids[0]).unwrap();
        assert_eq!(removed.block_ids.len(), 2);
        assert_eq!(table.origin_of(ids[1]), &BlockOrigin::Main);
        assert_eq!(table.origin_of(ids[2]), &BlockOrigin::Main);
        assert!(table.entry(ids[0]).is_none());
    }

    #[test]
    fn is_mounted_reflects_origin() {
        let mut table = MountTable::new();
        let ids = make_ids(2);
        assert!(!table.is_mounted(ids[0]));

        table.set_origin(ids[0], BlockOrigin::Mounted { mount_point: ids[1] });
        assert!(table.is_mounted(ids[0]));
    }

    #[test]
    fn remove_origin_single_block() {
        let mut table = MountTable::new();
        let ids = make_ids(2);
        table.set_origin(ids[0], BlockOrigin::Mounted { mount_point: ids[1] });
        assert!(table.is_mounted(ids[0]));

        table.remove_origin(ids[0]);
        assert!(!table.is_mounted(ids[0]));
    }

    #[test]
    fn is_path_mounted_detects_existing() {
        let mut table = MountTable::new();
        let ids = make_ids(3);
        let path = PathBuf::from("/abs/sub.json");
        table.insert_entry(
            ids[0],
            MountEntry::new(
                path.clone(),
                PathBuf::from("sub.json"),
                vec![ids[1]],
                vec![ids[1], ids[2]],
            ),
        );
        assert!(table.is_path_mounted(&path));
        assert!(!table.is_path_mounted(std::path::Path::new("/other.json")));
    }

    #[test]
    fn is_path_mounted_false_after_remove() {
        let mut table = MountTable::new();
        let ids = make_ids(3);
        let path = PathBuf::from("/abs/sub.json");
        table.insert_entry(
            ids[0],
            MountEntry::new(
                path.clone(),
                PathBuf::from("sub.json"),
                vec![ids[1]],
                vec![ids[1], ids[2]],
            ),
        );
        table.remove_entry(ids[0]);
        assert!(!table.is_path_mounted(&path));
    }
}
