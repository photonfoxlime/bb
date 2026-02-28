//! Mount system: data structures and operations for external file mounts.
//!
//! A mount is a `BlockNode::Mount { path }` that references an external file.
//! When expanded at runtime, the file is deserialized into a `BlockStore`, its
//! blocks are re-keyed with fresh `BlockId`s into the main store, and the mount
//! node is swapped to `BlockNode::Children`. The [`MountTable`] remembers which
//! blocks came from which file so that edits can be saved back to the
//! originating file and collapsed back to a `Mount` node.
//!
//! # Data types
//!
//! - [`MountError`] -- error enum for mount I/O and parse failures.
//! - [`BlockOrigin`] -- per-block provenance tag (which mount loaded it).
//! - [`MountEntry`] -- metadata for a single mounted file (paths, format, ids).
//! - [`MountTable`] -- runtime-only table aggregating origins and entries.
//! - [`MountFormat`] -- the on-disk serialization format (`Json` or `Markdown`).
//!
//! # `BlockStore` methods
//!
//! Mount point lifecycle: [`set_mount_path`](BlockStore::set_mount_path) →
//! [`expand_mount`](BlockStore::expand_mount) →
//! [`collapse_mount`](BlockStore::collapse_mount).
//!
//! Mount management helpers: [`move_mount_file`](BlockStore::move_mount_file),
//! [`inline_mount`](BlockStore::inline_mount), and
//! [`inline_mount_recursive`](BlockStore::inline_mount_recursive).
//!
//! Persistence helpers: [`save_subtree_to_file`](BlockStore::save_subtree_to_file),
//! [`snapshot_for_save`](BlockStore::snapshot_for_save),
//! [`extract_mount_store`](BlockStore::extract_mount_store).

use super::drafts::{
    ExpansionDraftRecord, InquiryDraftRecord, InstructionDraftRecord, ReductionDraftRecord,
};
use super::{BlockId, BlockNode, BlockStore, FriendBlock, MountProjection, PanelBarState};
use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap, SparseSecondaryMap};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Mount data types (formerly top-level `crate::mount`)
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum MountError {
    #[error("block is not a mount node")]
    NotAMount,
    #[error("block is not mounted")]
    NotMounted,
    #[error("unknown block id")]
    UnknownBlock,
    #[error("failed to read mount file {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("failed to write mount file {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("failed to parse mount file {path}: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
    #[error("failed to parse markdown mount file {path}: {reason}")]
    MarkdownParse { path: PathBuf, reason: String },
}

/// Identifies which file owns a block.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockOrigin {
    /// Block was loaded from an external mounted file.
    Mounted {
        /// The id of the mount-point block whose `BlockNode::Mount` triggered the load.
        mount_point: BlockId,
    },
}

/// Metadata for a single mounted file.
#[derive(Debug, Clone)]
pub struct MountEntry {
    /// Canonical (absolute) path used for save-back.
    pub path: PathBuf,
    /// Original relative path as stored in the `BlockNode::Mount`.
    /// Restored on collapse to preserve the serialization form.
    pub rel_path: PathBuf,
    /// Persisted format of the mount file.
    pub format: MountFormat,
    /// Root block ids of the mounted sub-store (after re-keying).
    pub root_ids: Vec<BlockId>,
    /// All block ids belonging to this mount (roots + descendants).
    pub block_ids: Vec<BlockId>,
}

impl MountEntry {
    pub fn new(
        path: PathBuf, rel_path: PathBuf, format: MountFormat, root_ids: Vec<BlockId>,
        block_ids: Vec<BlockId>,
    ) -> Self {
        Self { path, rel_path, format, root_ids, block_ids }
    }
}

/// Runtime-only table tracking mounted files and block ownership.
///
/// Not serialized: mount state is reconstructed on load by re-expanding
/// `BlockNode::Mount` nodes.
#[derive(Debug, Clone, Default)]
pub struct MountTable {
    /// Per-block origin. Only blocks from mounted files are tracked.
    origins: SecondaryMap<BlockId, BlockOrigin>,
    /// Per-mount-point metadata, keyed by the mount-point block id.
    entries: SecondaryMap<BlockId, MountEntry>,
}

impl MountTable {
    /// Create a new empty mount table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the block origin (used when blocks are loaded from mounted files).
    pub fn set_origin(&mut self, block_id: BlockId, origin: BlockOrigin) {
        self.origins.insert(block_id, origin);
    }

    /// Register a mount entry for a mount-point block.
    pub fn insert_entry(&mut self, mount_point: BlockId, entry: MountEntry) {
        self.entries.insert(mount_point, entry);
    }

    /// Look up the mount entry for a mount-point block.
    pub fn entry(&self, mount_point: BlockId) -> Option<&MountEntry> {
        self.entries.get(mount_point)
    }

    /// Mutably look up the mount entry for a mount-point block.
    pub fn entry_mut(&mut self, mount_point: BlockId) -> Option<&mut MountEntry> {
        self.entries.get_mut(mount_point)
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

    pub fn origin(&self, block_id: BlockId) -> Option<&BlockOrigin> {
        self.origins.get(block_id)
    }

    /// Iterate over all mount entries as `(mount_point_id, entry)` pairs.
    pub fn entries(&self) -> impl Iterator<Item = (BlockId, &MountEntry)> {
        self.entries.iter()
    }
}

// ---------------------------------------------------------------------------
// Mount format enum
// ---------------------------------------------------------------------------

/// Persisted format for mount files referenced by [`BlockNode::Mount`].
///
/// `Json` remains the default for backward compatibility with existing files
/// that only stored `path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MountFormat {
    /// Canonical store JSON encoding used for full-fidelity mount round-trips.
    #[default]
    Json,
    /// Markdown Mount v1 encoding produced by [`BlockStore::render_markdown_mount_store`].
    Markdown,
}

impl std::fmt::Display for MountFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            | MountFormat::Json => write!(f, "json"),
            | MountFormat::Markdown => write!(f, "markdown"),
        }
    }
}

// ---------------------------------------------------------------------------
// BlockStore mount methods
// ---------------------------------------------------------------------------

impl BlockStore {
    /// Borrow the mount table for querying block origins.
    pub fn mount_table(&self) -> &MountTable {
        &self.mount_table
    }

    /// Convert a childless block into a mount-point node.
    ///
    /// The block must exist and have no children; otherwise returns `None`.
    /// After this call, [`expand_mount`](Self::expand_mount) can load the file.
    pub fn set_mount_path(&mut self, id: &BlockId, path: std::path::PathBuf) -> Option<()> {
        self.set_mount_path_with_format(id, path, MountFormat::Json)
    }

    /// Convert a childless block into a mount-point node with a specific format.
    pub fn set_mount_path_with_format(
        &mut self, id: &BlockId, path: std::path::PathBuf, format: MountFormat,
    ) -> Option<()> {
        let node = self.nodes.get(*id)?;
        if !node.children().is_empty() {
            return None;
        }
        if let Some(node) = self.nodes.get_mut(*id) {
            *node = if format == MountFormat::Json {
                BlockNode::with_path(path)
            } else {
                BlockNode::with_path_and_format(path, format)
            };
        }
        Some(())
    }

    /// Expand a `Mount` node: load the referenced file, re-key its blocks
    /// into this store, and swap the node to `Children`.
    ///
    /// `base_dir` is the directory against which relative mount paths are
    /// resolved (typically the directory containing the main blocks file).
    /// For nested mounts, relative paths resolve against the parent mount file
    /// directory instead of global app data dir.
    ///
    /// Cycle policy: expansion is lazy and user-driven; this function does not
    /// proactively reject recursive mount chains.
    ///
    /// Returns the re-keyed root ids of the mounted sub-store.
    pub fn expand_mount(
        &mut self, mount_point: &BlockId, base_dir: &Path,
    ) -> Result<Vec<BlockId>, MountError> {
        let node = self.nodes.get(*mount_point).ok_or(MountError::UnknownBlock)?;
        let (rel_path, format) = match node {
            | BlockNode::Mount { path, format } => (path.clone(), *format),
            | BlockNode::Children { .. } => return Err(MountError::NotAMount),
        };

        let effective_base_dir = self.effective_mount_base_dir(mount_point, base_dir);
        let resolved = Self::resolve_mount_path(&rel_path, &effective_base_dir);
        let canonical = fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());

        let contents = fs::read_to_string(&resolved)
            .map_err(|e| MountError::Read { path: resolved.clone(), source: e })?;
        let sub_store: BlockStore = match format {
            | MountFormat::Json => serde_json::from_str(&contents)
                .map_err(|e| MountError::Parse { path: resolved.clone(), source: e })?,
            | MountFormat::Markdown => Self::parse_markdown_mount_store(&contents)
                .map_err(|reason| MountError::MarkdownParse { path: resolved.clone(), reason })?,
        };

        // tracing::trace!(mount_point = ?mount_point, "expanding mount");
        // tracing::trace!(point = ?self.points.get(*mount_point), "mount point content");
        // tracing::trace!(hint = ?sub_store.hint, "hint");

        // If the mount point is empty, use the hint to fill it's content.
        if let Some(point) = self.points.get_mut(*mount_point)
            && point.is_empty()
            && let Some(hint) = &sub_store.hint
        {
            *point = hint.clone();
        }

        let (new_roots, all_new_ids) = self.rekey_sub_store(&sub_store, mount_point);

        self.mount_table.insert_entry(
            *mount_point,
            MountEntry::new(canonical, rel_path.clone(), format, new_roots.clone(), all_new_ids),
        );

        if let Some(node) = self.nodes.get_mut(*mount_point) {
            *node = BlockNode::with_children(new_roots.clone());
        }
        self.view_collapsed.remove(*mount_point);
        self.friend_blocks.remove(*mount_point);

        Ok(new_roots)
    }

    /// Unmount a previously expanded mount point: remove all re-keyed blocks
    /// and restore the node to `Mount { path }`.
    ///
    /// This also clears nested mounted runtime blocks reachable under the
    /// expanded subtree and restores the mount-point using `entry.rel_path`.
    ///
    /// Returns `None` if the mount point has no entry in the mount table.
    pub fn collapse_mount(&mut self, mount_point: &BlockId) -> Option<()> {
        let entry = self.mount_table.remove_entry(*mount_point)?;

        let mut removed_ids = Vec::new();
        for child in self.children(mount_point) {
            self.collect_subtree_ids(child, &mut removed_ids);
        }
        let mut seen = std::collections::HashSet::new();
        removed_ids.retain(|id| seen.insert(*id));

        let nested_mount_points: Vec<BlockId> = removed_ids
            .iter()
            .copied()
            .filter(|id| self.mount_table.entry(*id).is_some())
            .collect();
        for nested_mount_point in nested_mount_points {
            self.mount_table.remove_entry(nested_mount_point);
        }

        for id in &removed_ids {
            self.nodes.remove(*id);
            self.points.remove(*id);
            self.expansion_drafts.remove(*id);
            self.reduction_drafts.remove(*id);
            self.instruction_drafts.remove(*id);
            self.inquiry_drafts.remove(*id);
            self.view_collapsed.remove(*id);
            self.friend_blocks.remove(*id);
            self.panel_state.remove(*id);
            self.mount_table.remove_origin(*id);
        }
        self.remove_friend_block_references(&removed_ids);
        if let Some(node) = self.nodes.get_mut(*mount_point) {
            *node = BlockNode::with_path_and_format(entry.rel_path, entry.format);
        }
        Some(())
    }

    /// Move a mounted file to a new location and update mount metadata.
    ///
    /// Behavior depends on mount state:
    /// - expanded mount: writes current in-memory mounted content to `new_path`,
    ///   updates the mount entry paths, and removes the old file when paths differ;
    /// - unexpanded mount: moves the existing backing file and updates the node's
    ///   persisted mount path.
    ///
    /// Relative path storage is preserved against the effective mount base
    /// directory (parent mount file directory for nested mounts, otherwise
    /// the provided `base_dir`).
    pub fn move_mount_file(
        &mut self, mount_point: &BlockId, new_path: &Path, base_dir: &Path,
    ) -> Result<(), MountError> {
        let _ = self.nodes.get(*mount_point).ok_or(MountError::UnknownBlock)?;
        let effective_base_dir = self.effective_mount_base_dir(mount_point, base_dir);
        let target_path = if new_path.is_relative() {
            effective_base_dir.join(new_path)
        } else {
            new_path.to_path_buf()
        };

        if let Some(entry) = self.mount_table.entry(*mount_point).cloned() {
            let projected = self.extract_mount_store(mount_point, &entry);
            Self::write_store_with_format(&target_path, entry.format, &projected)?;

            let canonical_target =
                fs::canonicalize(&target_path).unwrap_or_else(|_| target_path.clone());
            if canonical_target != entry.path
                && let Err(source) = fs::remove_file(&entry.path)
                && source.kind() != std::io::ErrorKind::NotFound
            {
                return Err(MountError::Write { path: entry.path.clone(), source });
            }

            let rel_path = Self::relative_or_absolute_path(&target_path, &effective_base_dir);
            if let Some(entry_mut) = self.mount_table.entry_mut(*mount_point) {
                entry_mut.path = canonical_target;
                entry_mut.rel_path = rel_path;
            }
            return Ok(());
        }

        let (current_rel_path, format) = match self.nodes.get(*mount_point) {
            | Some(BlockNode::Mount { path, format }) => (path.clone(), *format),
            | Some(BlockNode::Children { .. }) => return Err(MountError::NotMounted),
            | None => return Err(MountError::UnknownBlock),
        };
        let source_path = Self::resolve_mount_path(&current_rel_path, &effective_base_dir);
        if !source_path.exists() {
            return Err(MountError::Read {
                path: source_path,
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "mount file not found"),
            });
        }

        if source_path != target_path {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|source| MountError::Write { path: target_path.clone(), source })?;
            }
            if fs::rename(&source_path, &target_path).is_err() {
                fs::copy(&source_path, &target_path)
                    .map_err(|source| MountError::Write { path: target_path.clone(), source })?;
                fs::remove_file(&source_path)
                    .map_err(|source| MountError::Write { path: source_path.clone(), source })?;
            }
        }

        let rel_path = Self::relative_or_absolute_path(&target_path, &effective_base_dir);
        if let Some(node) = self.nodes.get_mut(*mount_point) {
            *node = BlockNode::with_path_and_format(rel_path, format);
        }
        Ok(())
    }

    /// Inline one mount into the current store.
    ///
    /// If the mount is unexpanded, this expands it first. Then runtime mount
    /// tracking for `mount_point` is removed while leaving its expanded children
    /// as normal in-store nodes.
    pub fn inline_mount(
        &mut self, mount_point: &BlockId, base_dir: &Path,
    ) -> Result<(), MountError> {
        let node = self.nodes.get(*mount_point).ok_or(MountError::UnknownBlock)?;
        let has_entry = self.mount_table.entry(*mount_point).is_some();
        let is_unexpanded_mount = matches!(node, BlockNode::Mount { .. });
        if !has_entry && !is_unexpanded_mount {
            return Err(MountError::NotMounted);
        }

        if is_unexpanded_mount {
            self.expand_mount(mount_point, base_dir)?;
        }

        let Some(entry) = self.mount_table.remove_entry(*mount_point) else {
            return Err(MountError::NotMounted);
        };
        tracing::info!(
            mount_point = ?mount_point,
            inlined_blocks = entry.block_ids.len(),
            "inlined mount into current store"
        );
        Ok(())
    }

    /// Inline all mounted files reachable under `mount_point`.
    ///
    /// Traverses the subtree rooted at `mount_point`, expanding and detaching
    /// each encountered mount so the full content remains in the current file.
    /// Returns the number of inlined mount points.
    pub fn inline_mount_recursive(
        &mut self, mount_point: &BlockId, base_dir: &Path,
    ) -> Result<usize, MountError> {
        let _ = self.nodes.get(*mount_point).ok_or(MountError::UnknownBlock)?;

        let mut stack = vec![*mount_point];
        let mut inlined_mount_count = 0;
        while let Some(current) = stack.pop() {
            if self.nodes.get(current).is_none() {
                continue;
            }

            let is_expanded_mount = self.mount_table.entry(current).is_some();
            let is_unexpanded_mount =
                self.node(&current).is_some_and(|node| matches!(node, BlockNode::Mount { .. }));
            if is_expanded_mount || is_unexpanded_mount {
                self.inline_mount(&current, base_dir)?;
                inlined_mount_count += 1;
            }

            let children = self.children(&current).to_vec();
            stack.extend(children.into_iter().rev());
        }

        Ok(inlined_mount_count)
    }

    /// Extract a block's children and their subtrees into a standalone
    /// store and write it to `path`. The block is then replaced with
    /// `BlockNode::Mount { rel_path }`.
    ///
    /// `base_dir` is used to compute the relative path stored in the mount
    /// node. Expanded mounts within the subtree are collapsed back to
    /// `Mount` nodes in the saved file, preserving recursive mount
    /// structure.
    pub fn save_subtree_to_file(
        &mut self, block_id: &BlockId, path: &Path, base_dir: &Path,
    ) -> Result<(), MountError> {
        let node = self.nodes.get(*block_id).ok_or(MountError::UnknownBlock)?;
        let hint = self
            .points
            .get(*block_id)
            .cloned()
            .and_then(|p| if p.is_empty() { None } else { Some(p) });
        let children = node.children().to_vec();

        // Collect descendant IDs, stopping at expanded mount boundaries.
        let mut own_ids = Vec::new();
        let mut nested_mounts = Vec::new();
        for child in &children {
            self.collect_own_subtree_ids(child, &mut own_ids, &mut nested_mounts);
        }

        let mut mount_path_overrides: std::collections::HashMap<BlockId, MountProjection> =
            std::collections::HashMap::new();
        for &old_id in &nested_mounts {
            if let Some(entry) = self.mount_table.entry(old_id) {
                mount_path_overrides.insert(
                    old_id,
                    MountProjection { path: entry.rel_path.clone(), format: entry.format },
                );
            }
        }
        let sub_store =
            self.build_projected_store(&own_ids, hint, &children, &mount_path_overrides);

        let format = Self::format_from_path(path);

        // Write to file.
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| MountError::Read { path: path.to_path_buf(), source: e })?;
        }
        match format {
            | MountFormat::Json => {
                let json = serde_json::to_string_pretty(&sub_store)
                    .map_err(|e| MountError::Parse { path: path.to_path_buf(), source: e })?;
                fs::write(path, &json)
                    .map_err(|e| MountError::Read { path: path.to_path_buf(), source: e })?;
            }
            | MountFormat::Markdown => {
                let markdown = Self::render_markdown_mount_store(&sub_store);
                fs::write(path, markdown)
                    .map_err(|e| MountError::Read { path: path.to_path_buf(), source: e })?;
            }
        }

        // Clean up nested expanded mounts and their blocks.
        let mut removed_friend_references = Vec::new();
        for &mount_id in &nested_mounts {
            if let Some(entry) = self.mount_table.remove_entry(mount_id) {
                removed_friend_references.extend(entry.block_ids.iter().copied());
                for &id in &entry.block_ids {
                    self.nodes.remove(id);
                    self.points.remove(id);
                    self.expansion_drafts.remove(id);
                    self.reduction_drafts.remove(id);
                    self.instruction_drafts.remove(id);
                    self.inquiry_drafts.remove(id);
                    self.view_collapsed.remove(id);
                    self.friend_blocks.remove(id);
                    self.panel_state.remove(id);
                }
            }
        }

        // Remove own subtree nodes from main store (not block_id itself).
        for &id in &own_ids {
            self.nodes.remove(id);
            self.points.remove(id);
            self.expansion_drafts.remove(id);
            self.reduction_drafts.remove(id);
            self.instruction_drafts.remove(id);
            self.inquiry_drafts.remove(id);
            self.view_collapsed.remove(id);
            self.friend_blocks.remove(id);
            self.panel_state.remove(id);
            self.mount_table.remove_origin(id);
        }
        removed_friend_references.extend(own_ids.iter().copied());
        self.remove_friend_block_references(&removed_friend_references);

        // Compute relative path.
        let rel_path = path
            .strip_prefix(base_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf());

        // Replace node with mount.
        if let Some(node) = self.nodes.get_mut(*block_id) {
            *node = BlockNode::with_path_and_format(rel_path, format);
        }

        Ok(())
    }

    fn build_projected_store(
        &self, kept_ids: &[BlockId], hint: Option<String>, roots: &[BlockId],
        mount_path_overrides: &std::collections::HashMap<BlockId, MountProjection>,
    ) -> BlockStore {
        let mut sub_nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let mut sub_points: SecondaryMap<BlockId, String> = SecondaryMap::new();
        let mut sub_expansion_drafts: SparseSecondaryMap<BlockId, ExpansionDraftRecord> =
            SparseSecondaryMap::new();
        let mut sub_reduction_drafts: SparseSecondaryMap<BlockId, ReductionDraftRecord> =
            SparseSecondaryMap::new();
        let mut sub_instruction_drafts: SparseSecondaryMap<BlockId, InstructionDraftRecord> =
            SparseSecondaryMap::new();
        let mut sub_inquiry_drafts: SparseSecondaryMap<BlockId, InquiryDraftRecord> =
            SparseSecondaryMap::new();
        let mut sub_friend_blocks: SparseSecondaryMap<BlockId, Vec<FriendBlock>> =
            SparseSecondaryMap::new();
        let mut sub_panel_state: SparseSecondaryMap<BlockId, PanelBarState> =
            SparseSecondaryMap::new();
        let mut id_map: std::collections::HashMap<BlockId, BlockId> =
            std::collections::HashMap::new();

        for &old_id in kept_ids {
            let point = self.points.get(old_id).cloned().unwrap_or_default();
            let new_id = sub_nodes.insert(BlockNode::with_children(vec![]));
            sub_points.insert(new_id, point);
            id_map.insert(old_id, new_id);
        }

        for &old_id in kept_ids {
            let Some(&new_id) = id_map.get(&old_id) else {
                continue;
            };
            if let Some(mount_projection) = mount_path_overrides.get(&old_id) {
                if let Some(node) = sub_nodes.get_mut(new_id) {
                    *node = BlockNode::with_path_and_format(
                        mount_projection.path.clone(),
                        mount_projection.format,
                    );
                }
                continue;
            }

            if let Some(old_node) = self.nodes.get(old_id) {
                match old_node {
                    | BlockNode::Children { children } => {
                        let new_children: Vec<BlockId> =
                            children.iter().filter_map(|c| id_map.get(c).copied()).collect();
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_children(new_children);
                        }
                    }
                    | BlockNode::Mount { path, format } => {
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_path_and_format(path.clone(), *format);
                        }
                    }
                }
            }
        }

        let sub_roots: Vec<BlockId> = roots.iter().filter_map(|r| id_map.get(r).copied()).collect();

        for (old_id, draft) in &self.expansion_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                sub_expansion_drafts.insert(new_id, draft.clone());
            }
        }
        for (old_id, draft) in &self.reduction_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                sub_reduction_drafts.insert(new_id, draft.clone());
            }
        }
        for (old_id, draft) in &self.instruction_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                sub_instruction_drafts.insert(new_id, draft.clone());
            }
        }
        for (old_id, draft) in &self.inquiry_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                sub_inquiry_drafts.insert(new_id, draft.clone());
            }
        }
        let mut sub_view_collapsed: SparseSecondaryMap<BlockId, bool> = SparseSecondaryMap::new();
        for (old_id, _) in &self.view_collapsed {
            if let Some(&new_id) = id_map.get(&old_id) {
                sub_view_collapsed.insert(new_id, true);
            }
        }
        for (old_target_id, old_friend_ids) in &self.friend_blocks {
            let Some(&new_target_id) = id_map.get(&old_target_id) else {
                continue;
            };
            let remapped = old_friend_ids
                .iter()
                .filter_map(|friend| {
                    id_map.get(&friend.block_id).copied().map(|block_id| FriendBlock {
                        block_id,
                        perspective: friend.perspective.clone(),
                        parent_lineage_telescope: friend.parent_lineage_telescope,
                        children_telescope: friend.children_telescope,
                    })
                })
                .collect::<Vec<_>>();
            if !remapped.is_empty() {
                sub_friend_blocks.insert(new_target_id, remapped);
            }
        }
        for (old_id, state) in &self.panel_state {
            if let Some(&new_id) = id_map.get(&old_id) {
                sub_panel_state.insert(new_id, *state);
            }
        }
        BlockStore::new_with_drafts(
            sub_roots,
            sub_nodes,
            sub_points,
            sub_expansion_drafts,
            sub_reduction_drafts,
            sub_instruction_drafts,
            sub_inquiry_drafts,
            sub_view_collapsed,
            sub_friend_blocks,
            sub_panel_state,
            hint,
        )
    }

    /// Re-key all blocks from `sub_store` into this store with fresh ids.
    ///
    /// Returns `(new_root_ids, all_new_ids)`.
    ///
    /// The `mount_point` is used to track the origin of re-keyed blocks.
    /// For external file imports that are not mounts, pass one of the store's roots.
    pub fn rekey_sub_store(
        &mut self, sub_store: &BlockStore, mount_point: &BlockId,
    ) -> (Vec<BlockId>, Vec<BlockId>) {
        let mut id_map: std::collections::HashMap<BlockId, BlockId> =
            std::collections::HashMap::new();
        let mut all_new_ids = Vec::new();

        // First pass: allocate fresh ids for every block in the sub-store.
        for (old_id, _node) in &sub_store.nodes {
            let new_id = self.nodes.insert(BlockNode::with_children(vec![]));
            id_map.insert(old_id, new_id);
            all_new_ids.push(new_id);

            let point = sub_store.points.get(old_id).cloned().unwrap_or_default();
            self.points.insert(new_id, point);

            self.mount_table.set_origin(new_id, BlockOrigin::Mounted { mount_point: *mount_point });
        }

        // Second pass: rewrite children references using the id map.
        for (old_id, old_node) in &sub_store.nodes {
            let new_id = id_map[&old_id];
            let remapped_children: Vec<BlockId> =
                old_node.children().iter().filter_map(|c| id_map.get(c).copied()).collect();

            match old_node {
                | BlockNode::Children { .. } => {
                    if let Some(node) = self.nodes.get_mut(new_id) {
                        *node = BlockNode::with_children(remapped_children);
                    }
                }
                | BlockNode::Mount { path, format } => {
                    if let Some(node) = self.nodes.get_mut(new_id) {
                        *node = BlockNode::with_path_and_format(path.clone(), *format);
                    }
                }
            }
        }

        let new_roots: Vec<BlockId> =
            sub_store.roots.iter().filter_map(|r| id_map.get(r).copied()).collect();

        for (old_id, draft) in &sub_store.expansion_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                self.expansion_drafts.insert(new_id, draft.clone());
            }
        }
        for (old_id, draft) in &sub_store.reduction_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                self.reduction_drafts.insert(new_id, draft.clone());
            }
        }
        for (old_id, draft) in &sub_store.instruction_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                self.instruction_drafts.insert(new_id, draft.clone());
            }
        }
        for (old_id, draft) in &sub_store.inquiry_drafts {
            if let Some(&new_id) = id_map.get(&old_id) {
                self.inquiry_drafts.insert(new_id, draft.clone());
            }
        }

        for (old_id, _) in &sub_store.view_collapsed {
            if let Some(&new_id) = id_map.get(&old_id) {
                self.view_collapsed.insert(new_id, true);
            }
        }
        for (old_target_id, old_friend_ids) in &sub_store.friend_blocks {
            let Some(&new_target_id) = id_map.get(&old_target_id) else {
                continue;
            };
            let remapped = old_friend_ids
                .iter()
                .filter_map(|friend| {
                    id_map.get(&friend.block_id).copied().map(|block_id| FriendBlock {
                        block_id,
                        perspective: friend.perspective.clone(),
                        parent_lineage_telescope: friend.parent_lineage_telescope,
                        children_telescope: friend.children_telescope,
                    })
                })
                .collect::<Vec<_>>();
            if !remapped.is_empty() {
                self.friend_blocks.insert(new_target_id, remapped);
            }
        }
        for (old_id, state) in &sub_store.panel_state {
            if let Some(&new_id) = id_map.get(&old_id) {
                self.panel_state.insert(new_id, *state);
            }
        }
        (new_roots, all_new_ids)
    }

    /// Build a serialization-ready snapshot that restores mount nodes and
    /// excludes re-keyed blocks.
    ///
    /// Builds a fresh `BlockStore` with compacted SlotMaps so that
    /// serialization produces no vacant-slot nulls.
    pub(crate) fn snapshot_for_save(&self) -> BlockStore {
        let mut mounted_ids: std::collections::HashSet<BlockId> = std::collections::HashSet::new();
        for (mount_point, _entry) in self.mount_table.entries() {
            for child in self.children(&mount_point) {
                let mut subtree = Vec::new();
                self.collect_subtree_ids(child, &mut subtree);
                mounted_ids.extend(subtree);
            }
        }

        let mut kept_ids = Vec::new();
        for (old_id, _node) in &self.nodes {
            if !mounted_ids.contains(&old_id) {
                kept_ids.push(old_id);
            }
        }

        let mut mount_path_overrides: std::collections::HashMap<BlockId, MountProjection> =
            std::collections::HashMap::new();
        for (mount_point, entry) in self.mount_table.entries() {
            mount_path_overrides.insert(
                mount_point,
                MountProjection { path: entry.rel_path.clone(), format: entry.format },
            );
        }

        self.build_projected_store(&kept_ids, None, &self.roots, &mount_path_overrides)
    }

    /// Extract blocks belonging to a mount entry into a standalone store.
    ///
    /// Builds a fresh `BlockStore` with compacted SlotMaps so that
    /// serialization produces no vacant-slot nulls.
    pub(crate) fn extract_mount_store(
        &self, mount_point: &BlockId, entry: &MountEntry,
    ) -> BlockStore {
        let root_ids = self
            .node(mount_point)
            .map(|node| node.children().to_vec())
            .unwrap_or_else(|| entry.root_ids.clone());
        let mut own_ids = Vec::new();
        let mut mount_points = Vec::new();
        for root_id in &root_ids {
            self.collect_own_subtree_ids(root_id, &mut own_ids, &mut mount_points);
        }
        let mut seen = std::collections::HashSet::new();
        own_ids.retain(|id| seen.insert(*id));

        let mut mount_path_overrides: std::collections::HashMap<BlockId, MountProjection> =
            std::collections::HashMap::new();
        for &old_id in &own_ids {
            if let Some(nested_entry) = self.mount_table.entry(old_id) {
                mount_path_overrides.insert(
                    old_id,
                    MountProjection {
                        path: nested_entry.rel_path.clone(),
                        format: nested_entry.format,
                    },
                );
            }
        }

        let hint = self
            .points
            .get(*mount_point)
            .cloned()
            .and_then(|p| if p.is_empty() { None } else { Some(p) });

        self.build_projected_store(&own_ids, hint, &root_ids, &mount_path_overrides)
    }

    /// Derive the effective base directory for mount path resolution.
    ///
    /// For nested mounts this is the parent mount file directory; for top-level
    /// mounts this is `base_dir`.
    fn effective_mount_base_dir(&self, mount_point: &BlockId, base_dir: &Path) -> PathBuf {
        self.mount_origin_path(mount_point)
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| base_dir.to_path_buf())
    }

    /// Convert `path` to a path stored in mount metadata.
    ///
    /// Relative paths are preferred when `path` is under `base_dir`.
    fn relative_or_absolute_path(path: &Path, base_dir: &Path) -> PathBuf {
        path.strip_prefix(base_dir)
            .map(|relative| relative.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf())
    }

    /// Write a `BlockStore` to disk using the chosen mount format.
    fn write_store_with_format(
        path: &Path, format: MountFormat, store: &BlockStore,
    ) -> Result<(), MountError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|source| MountError::Write { path: path.to_path_buf(), source })?;
        }
        match format {
            | MountFormat::Json => {
                let json = serde_json::to_string_pretty(store)
                    .map_err(|source| MountError::Parse { path: path.to_path_buf(), source })?;
                fs::write(path, json)
                    .map_err(|source| MountError::Write { path: path.to_path_buf(), source })
            }
            | MountFormat::Markdown => {
                let markdown = Self::render_markdown_mount_store(store);
                fs::write(path, markdown)
                    .map_err(|source| MountError::Write { path: path.to_path_buf(), source })
            }
        }
    }

    /// Resolve a mount path against a base directory.
    ///
    /// If the path is relative, join it with `base_dir`. Otherwise use as-is.
    fn resolve_mount_path(rel_path: &Path, base_dir: &Path) -> std::path::PathBuf {
        if rel_path.is_relative() { base_dir.join(rel_path) } else { rel_path.to_path_buf() }
    }

    /// Infer mount file format from the target path extension.
    ///
    /// `.md` and `.markdown` map to [`MountFormat::Markdown`].
    /// All other extensions (or missing extension) map to [`MountFormat::Json`].
    fn format_from_path(path: &Path) -> MountFormat {
        match path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            | Some("md") | Some("markdown") => MountFormat::Markdown,
            | _ => MountFormat::Json,
        }
    }

    fn mount_origin_path(&self, block_id: &BlockId) -> Option<&Path> {
        let origin = self.mount_table.origin(*block_id)?;
        match origin {
            | BlockOrigin::Mounted { mount_point } => {
                self.mount_table.entry(*mount_point).map(|entry| entry.path.as_path())
            }
        }
    }
}
