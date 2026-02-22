//! Block store: the core document data model.
//!
//! A document is a forest of blocks. Each block has a slotmap identity, a text
//! point, and ordered children. The store is serialized as JSON for persistence.

use crate::llm;
use crate::mount::{BlockOrigin, MountEntry, MountError, MountTable};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap};
use std::path::Path;
use std::{fs, io};
use thiserror::Error;

slotmap::new_key_type! {
pub struct BlockId;
}

/// Persisted expansion draft payload keyed by [`BlockId`].
///
/// Stored in [`BlockStore`] so in-progress rewrite/child suggestions survive
/// reloads and mount save/load round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpansionDraftRecord {
    pub rewrite: Option<String>,
    pub children: Vec<String>,
}

/// Persisted reduction draft payload keyed by [`BlockId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReductionDraftRecord {
    pub reduction: String,
}

#[derive(Debug, Error)]
pub enum StoreLoadError {
    #[error("application data path is unavailable")]
    PathUnavailable,
    #[error("failed to read block store file {path}: {source}")]
    Read { path: std::path::PathBuf, source: io::Error },
    #[error("failed to parse block store file {path}: {source}")]
    Parse { path: std::path::PathBuf, source: serde_json::Error },
}

/// One node in the block tree.
///
/// A node is either an inline list of child ids, or a mount point
/// referencing an external file whose contents are loaded lazily.
/// Text content (the "point") is stored separately in
/// [`BlockStore::points`] so that structure and content can be
/// queried and mutated independently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BlockNode {
    /// Inline children: the default structural variant.
    Children { children: Vec<BlockId> },
    /// Mount point: a path to an external block-store file.
    /// The path is stored relative to the parent store's file when possible.
    /// At runtime, the referenced file is loaded and its blocks are re-keyed
    /// into the main store; the node is then swapped to `Children` in memory.
    Mount { path: std::path::PathBuf },
}

impl BlockNode {
    /// Create an inline-children node with the given child ids.
    pub fn with_children(children: Vec<BlockId>) -> Self {
        Self::Children { children }
    }

    /// Create a mount-point node referencing an external file.
    pub fn with_path(path: std::path::PathBuf) -> Self {
        Self::Mount { path }
    }

    /// Return the inline children slice, or an empty slice for mount nodes.
    pub fn children(&self) -> &[BlockId] {
        match self {
            | Self::Children { children } => children,
            | Self::Mount { .. } => &[],
        }
    }

    /// Return a mutable reference to the inline children vec.
    ///
    /// Returns `None` for mount nodes.
    pub fn children_mut(&mut self) -> Option<&mut Vec<BlockId>> {
        match self {
            | Self::Children { children } => Some(children),
            | Self::Mount { .. } => None,
        }
    }

    /// Return the mount path if this is a mount node.
    pub fn mount_path(&self) -> Option<&std::path::Path> {
        match self {
            | Self::Children { .. } => None,
            | Self::Mount { path } => Some(path),
        }
    }
}

/// Forest of blocks: root ids, a structural map, and a content map.
///
/// Invariant: every id in `roots` and in any node's `children` must exist as
/// a key in `nodes` **and** in `points`. The store always has at least one root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockStore {
    roots: Vec<BlockId>,
    nodes: SlotMap<BlockId, BlockNode>,
    /// Text content for each block, keyed by the same `BlockId`.
    points: SecondaryMap<BlockId, String>,
    /// Runtime-only mount tracking. Not serialized; reconstructed by
    /// re-expanding `BlockNode::Mount` nodes after deserialization.
    #[serde(skip)]
    mount_table: MountTable,
    /// Persisted per-block expansion drafts (rewrite + suggested children).
    ///
    /// Invariant: keys should reference existing blocks in `nodes`.
    #[serde(default)]
    expansion_drafts: SecondaryMap<BlockId, ExpansionDraftRecord>,
    /// Persisted per-block reduction drafts.
    ///
    /// Invariant: keys should reference existing blocks in `nodes`.
    #[serde(default)]
    reduction_drafts: SecondaryMap<BlockId, ReductionDraftRecord>,
}

impl BlockStore {
    /// Construct a store from pre-built roots, nodes, and points.
    ///
    /// Caller must ensure every id in `roots` and in each node's `children`
    /// exists as a key in both `nodes` and `points`.
    pub fn new(
        roots: Vec<BlockId>, nodes: SlotMap<BlockId, BlockNode>,
        points: SecondaryMap<BlockId, String>,
    ) -> Self {
        Self::new_with_drafts(roots, nodes, points, SecondaryMap::new(), SecondaryMap::new())
    }

    fn new_with_drafts(
        roots: Vec<BlockId>, nodes: SlotMap<BlockId, BlockNode>,
        points: SecondaryMap<BlockId, String>,
        expansion_drafts: SecondaryMap<BlockId, ExpansionDraftRecord>,
        reduction_drafts: SecondaryMap<BlockId, ReductionDraftRecord>,
    ) -> Self {
        Self {
            roots,
            nodes,
            points,
            mount_table: MountTable::new(),
            expansion_drafts,
            reduction_drafts,
        }
    }

    /// The ordered root block ids.
    pub fn roots(&self) -> &[BlockId] {
        &self.roots
    }

    pub fn load() -> Result<Self, StoreLoadError> {
        let Some(path) = AppPaths::data_file() else {
            return Err(StoreLoadError::PathUnavailable);
        };
        Self::load_from_path(&path)
    }

    fn load_from_path(path: &Path) -> Result<Self, StoreLoadError> {
        match fs::read_to_string(&path) {
            | Ok(contents) => serde_json::from_str(&contents)
                .map_err(|source| StoreLoadError::Parse { path: path.to_path_buf(), source }),
            | Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            | Err(source) => Err(StoreLoadError::Read { path: path.to_path_buf(), source }),
        }
    }

    /// Persist the main store as pretty-printed JSON to the app data file.
    ///
    /// Expanded mount nodes are restored to `Mount { path }` before
    /// serialization and their re-keyed blocks are excluded.
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
    /// Each mount entry's blocks are extracted into a standalone `BlockStore`
    /// and written to the entry's path.
    pub fn save_mounts(&self) -> io::Result<()> {
        for (mount_point, entry) in self.mount_table.entries() {
            let sub = self.extract_mount_store(&mount_point, entry);
            if let Some(parent) = entry.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string_pretty(&sub)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            fs::write(&entry.path, json)?;
        }
        Ok(())
    }

    pub fn expansion_draft(&self, id: &BlockId) -> Option<&ExpansionDraftRecord> {
        self.expansion_drafts.get(*id)
    }

    pub fn expansion_draft_mut(&mut self, id: &BlockId) -> Option<&mut ExpansionDraftRecord> {
        self.expansion_drafts.get_mut(*id)
    }

    pub fn insert_expansion_draft(&mut self, id: BlockId, draft: ExpansionDraftRecord) {
        self.expansion_drafts.insert(id, draft);
    }

    pub fn remove_expansion_draft(&mut self, id: &BlockId) -> Option<ExpansionDraftRecord> {
        self.expansion_drafts.remove(*id)
    }

    pub fn reduction_draft(&self, id: &BlockId) -> Option<&ReductionDraftRecord> {
        self.reduction_drafts.get(*id)
    }

    pub fn insert_reduction_draft(&mut self, id: BlockId, draft: ReductionDraftRecord) {
        self.reduction_drafts.insert(id, draft);
    }

    pub fn remove_reduction_draft(&mut self, id: &BlockId) -> Option<ReductionDraftRecord> {
        self.reduction_drafts.remove(*id)
    }

    /// Build a serialization-ready snapshot that restores mount nodes and
    /// excludes re-keyed blocks.
    ///
    /// Builds a fresh `BlockStore` with compacted SlotMaps so that
    /// serialization produces no vacant-slot nulls.
    fn snapshot_for_save(&self) -> BlockStore {
        let mut mounted_ids: std::collections::HashSet<BlockId> = std::collections::HashSet::new();
        for (mount_point, _entry) in self.mount_table.entries() {
            for child in self.children(&mount_point) {
                let mut subtree = Vec::new();
                self.collect_subtree_ids(child, &mut subtree);
                mounted_ids.extend(subtree);
            }
        }

        let mut sub_nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let mut sub_points: SecondaryMap<BlockId, String> = SecondaryMap::new();
        let mut sub_expansion_drafts: SecondaryMap<BlockId, ExpansionDraftRecord> =
            SecondaryMap::new();
        let mut sub_reduction_drafts: SecondaryMap<BlockId, ReductionDraftRecord> =
            SecondaryMap::new();
        let mut id_map: std::collections::HashMap<BlockId, BlockId> =
            std::collections::HashMap::new();

        // First pass: allocate fresh ids for every non-mounted block.
        for (old_id, _node) in &self.nodes {
            if mounted_ids.contains(&old_id) {
                continue;
            }
            let point = self.points.get(old_id).cloned().unwrap_or_default();
            let new_id = sub_nodes.insert(BlockNode::with_children(vec![]));
            sub_points.insert(new_id, point);
            id_map.insert(old_id, new_id);
        }

        // Second pass: rewrite node contents with remapped ids.
        // Mount-point nodes are restored to Mount { path }.
        for (old_id, old_node) in &self.nodes {
            let Some(&new_id) = id_map.get(&old_id) else {
                continue;
            };
            if let Some(entry) = self.mount_table.entry(old_id) {
                // This is an expanded mount point: restore as Mount node.
                if let Some(node) = sub_nodes.get_mut(new_id) {
                    *node = BlockNode::with_path(entry.rel_path.clone());
                }
            } else {
                match old_node {
                    | BlockNode::Children { children } => {
                        let new_children: Vec<BlockId> =
                            children.iter().filter_map(|c| id_map.get(c).copied()).collect();
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_children(new_children);
                        }
                    }
                    | BlockNode::Mount { path } => {
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_path(path.clone());
                        }
                    }
                }
            }
        }

        let sub_roots: Vec<BlockId> =
            self.roots.iter().filter_map(|r| id_map.get(r).copied()).collect();

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

        BlockStore::new_with_drafts(
            sub_roots,
            sub_nodes,
            sub_points,
            sub_expansion_drafts,
            sub_reduction_drafts,
        )
    }

    /// Extract blocks belonging to a mount entry into a standalone store.
    ///
    /// Builds a fresh `BlockStore` with compacted SlotMaps so that
    /// serialization produces no vacant-slot nulls.
    fn extract_mount_store(&self, mount_point: &BlockId, entry: &MountEntry) -> BlockStore {
        let root_ids = self
            .node(mount_point)
            .map(|node| node.children().to_vec())
            .unwrap_or_else(|| entry.root_ids.clone());
        let mut own_ids = Vec::new();
        let mut _mount_points = Vec::new();
        for root_id in &root_ids {
            self.collect_own_subtree_ids(root_id, &mut own_ids, &mut _mount_points);
        }
        let mut seen = std::collections::HashSet::new();
        own_ids.retain(|id| seen.insert(*id));

        let mut sub_nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let mut sub_points: SecondaryMap<BlockId, String> = SecondaryMap::new();
        let mut sub_expansion_drafts: SecondaryMap<BlockId, ExpansionDraftRecord> =
            SecondaryMap::new();
        let mut sub_reduction_drafts: SecondaryMap<BlockId, ReductionDraftRecord> =
            SecondaryMap::new();
        let mut id_map: std::collections::HashMap<BlockId, BlockId> =
            std::collections::HashMap::new();

        // First pass: allocate fresh ids for every kept block.
        for &old_id in &own_ids {
            let point = self.points.get(old_id).cloned().unwrap_or_default();
            let new_id = sub_nodes.insert(BlockNode::with_children(vec![]));
            sub_points.insert(new_id, point);
            id_map.insert(old_id, new_id);
        }

        // Second pass: rewrite node contents with remapped ids.
        for &old_id in &own_ids {
            let new_id = id_map[&old_id];
            if let Some(nested_entry) = self.mount_table.entry(old_id) {
                if let Some(node) = sub_nodes.get_mut(new_id) {
                    *node = BlockNode::with_path(nested_entry.rel_path.clone());
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
                    | BlockNode::Mount { path } => {
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_path(path.clone());
                        }
                    }
                }
            }
        }

        let sub_roots: Vec<BlockId> =
            root_ids.iter().filter_map(|r| id_map.get(r).copied()).collect();

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

        BlockStore::new_with_drafts(
            sub_roots,
            sub_nodes,
            sub_points,
            sub_expansion_drafts,
            sub_reduction_drafts,
        )
    }

    /// Look up a node by id.
    pub fn node(&self, id: &BlockId) -> Option<&BlockNode> {
        self.nodes.get(*id)
    }

    /// Return the children of a block, or an empty slice if unknown or a mount.
    pub fn children(&self, id: &BlockId) -> &[BlockId] {
        self.nodes.get(*id).map(|n| n.children()).unwrap_or(&[])
    }

    /// Return the text point of a block, or `None` if the id is unknown.
    pub fn point(&self, id: &BlockId) -> Option<String> {
        self.points.get(*id).cloned()
    }

    /// Overwrite the text point of an existing block. No-op if `id` is unknown.
    pub fn update_point(&mut self, id: &BlockId, value: String) {
        if self.nodes.contains_key(*id) {
            self.points.insert(*id, value);
        }
    }

    /// Add one child block under the parent and return the new child id.
    pub fn append_child(&mut self, parent_id: &BlockId, point: String) -> Option<BlockId> {
        if !self.nodes.contains_key(*parent_id) {
            return None;
        }

        let child_id = self.nodes.insert(BlockNode::with_children(vec![]));
        self.points.insert(child_id, point);
        if let Some(mount_point) = self.inherited_mount_point_for_anchor(parent_id) {
            self.mount_table.set_origin(child_id, BlockOrigin::Mounted { mount_point });
        }
        if let Some(parent) = self.nodes.get_mut(*parent_id) {
            if let Some(children) = parent.children_mut() {
                children.push(child_id);
            }
        }
        Some(child_id)
    }

    /// Insert a sibling block immediately after `block_id` in its parent's
    /// child list (or in roots if `block_id` is a root). Returns the new id.
    pub fn append_sibling(&mut self, block_id: &BlockId, point: String) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let sibling_id = self.nodes.insert(BlockNode::with_children(vec![]));
        self.points.insert(sibling_id, point);
        if let Some(mount_point) = self.inherited_mount_point_for_anchor(block_id) {
            self.mount_table.set_origin(sibling_id, BlockOrigin::Mounted { mount_point });
        }

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(parent_id)?;
            if let Some(children) = parent.children_mut() {
                children.insert(index + 1, sibling_id);
            }
        } else {
            self.roots.insert(index + 1, sibling_id);
        }
        Some(sibling_id)
    }

    /// Deep-clone a block and its entire subtree with fresh ids, inserting the
    /// copy immediately after the original. Returns the cloned root id.
    pub fn duplicate_subtree_after(&mut self, block_id: &BlockId) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let duplicate_id = self.clone_subtree_with_new_ids(block_id)?;

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(parent_id)?;
            if let Some(children) = parent.children_mut() {
                children.insert(index + 1, duplicate_id);
            }
        } else {
            self.roots.insert(index + 1, duplicate_id);
        }
        Some(duplicate_id)
    }

    /// Remove a block and its entire subtree. Returns the removed ids.
    ///
    /// If removal empties the root list, a fresh empty root is inserted.
    pub fn remove_block_subtree(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(parent_id) {
                if let Some(children) = parent.children_mut() {
                    children.remove(index);
                }
            }
        } else {
            self.roots.remove(index);
        }

        let mut removed_ids = Vec::new();
        self.collect_subtree_ids(block_id, &mut removed_ids);
        for id in &removed_ids {
            self.nodes.remove(*id);
            self.points.remove(*id);
            self.expansion_drafts.remove(*id);
            self.reduction_drafts.remove(*id);
            self.mount_table.remove_origin(*id);
        }

        if self.roots.is_empty() {
            let root_id = self.nodes.insert(BlockNode::with_children(vec![]));
            self.points.insert(root_id, String::new());
            self.roots.push(root_id);
        }

        Some(removed_ids)
    }

    /// Return lineage points from one root to the target id (DFS).
    pub fn lineage_points_for_id(&self, target: &BlockId) -> llm::Lineage {
        for root in &self.roots {
            let mut collected = Vec::new();
            if self.collect_lineage_points(root, target, &mut collected) {
                return llm::Lineage::from_points(collected);
            }
        }
        llm::Lineage::from_points(vec![])
    }

    /// Borrow the mount table for querying block origins.
    pub fn mount_table(&self) -> &MountTable {
        &self.mount_table
    }

    /// Convert a childless block into a mount-point node.
    ///
    /// The block must exist and have no children; otherwise returns `None`.
    /// After this call, [`expand_mount`](Self::expand_mount) can load the file.
    pub fn set_mount_path(&mut self, id: &BlockId, path: std::path::PathBuf) -> Option<()> {
        let node = self.nodes.get(*id)?;
        if !node.children().is_empty() {
            return None;
        }
        if let Some(node) = self.nodes.get_mut(*id) {
            *node = BlockNode::with_path(path);
        }
        Some(())
    }

    /// Expand a `Mount` node: load the referenced file, re-key its blocks
    /// into this store, and swap the node to `Children`.
    ///
    /// `base_dir` is the directory against which relative mount paths are
    /// resolved (typically the directory containing the main blocks file).
    ///
    /// Returns the re-keyed root ids of the mounted sub-store.
    pub fn expand_mount(
        &mut self, mount_point: &BlockId, base_dir: &Path,
    ) -> Result<Vec<BlockId>, MountError> {
        let node = self.nodes.get(*mount_point).ok_or(MountError::UnknownBlock)?;
        let rel_path = match node {
            | BlockNode::Mount { path } => path.clone(),
            | BlockNode::Children { .. } => return Err(MountError::NotAMount),
        };

        let effective_base_dir = self
            .mount_origin_path(mount_point)
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| base_dir.to_path_buf());
        let resolved = Self::resolve_mount_path(&rel_path, &effective_base_dir);
        let canonical = fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());

        let contents = fs::read_to_string(&resolved)
            .map_err(|e| MountError::Read { path: resolved.clone(), source: e })?;
        let sub_store: BlockStore = serde_json::from_str(&contents)
            .map_err(|e| MountError::Parse { path: resolved.clone(), source: e })?;

        let (new_roots, all_new_ids) = self.rekey_sub_store(&sub_store, mount_point);

        self.mount_table.insert_entry(
            *mount_point,
            MountEntry::new(canonical, rel_path.clone(), new_roots.clone(), all_new_ids),
        );

        if let Some(node) = self.nodes.get_mut(*mount_point) {
            *node = BlockNode::with_children(new_roots.clone());
        }

        Ok(new_roots)
    }

    /// Unmount a previously expanded mount point: remove all re-keyed blocks
    /// and restore the node to `Mount { path }`.
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

        for id in removed_ids {
            self.nodes.remove(id);
            self.points.remove(id);
            self.expansion_drafts.remove(id);
            self.reduction_drafts.remove(id);
            self.mount_table.remove_origin(id);
        }
        if let Some(node) = self.nodes.get_mut(*mount_point) {
            *node = BlockNode::with_path(entry.rel_path);
        }
        Some(())
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
        let children = node.children().to_vec();

        // Collect descendant IDs, stopping at expanded mount boundaries.
        let mut own_ids = Vec::new();
        let mut nested_mounts = Vec::new();
        for child in &children {
            self.collect_own_subtree_ids(child, &mut own_ids, &mut nested_mounts);
        }

        // Build a standalone sub-store.
        let mut sub_nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let mut sub_points: SecondaryMap<BlockId, String> = SecondaryMap::new();
        let mut sub_expansion_drafts: SecondaryMap<BlockId, ExpansionDraftRecord> =
            SecondaryMap::new();
        let mut sub_reduction_drafts: SecondaryMap<BlockId, ReductionDraftRecord> =
            SecondaryMap::new();
        let mut id_map: std::collections::HashMap<BlockId, BlockId> =
            std::collections::HashMap::new();

        // First pass: allocate fresh ids.
        for &old_id in &own_ids {
            let new_id = sub_nodes.insert(BlockNode::with_children(vec![]));
            let point = self.points.get(old_id).cloned().unwrap_or_default();
            sub_points.insert(new_id, point);
            id_map.insert(old_id, new_id);
        }

        // Second pass: rewrite node contents.
        for &old_id in &own_ids {
            let new_id = id_map[&old_id];
            if nested_mounts.contains(&old_id) {
                // Expanded mount point: restore as a Mount node.
                if let Some(entry) = self.mount_table.entry(old_id) {
                    if let Some(node) = sub_nodes.get_mut(new_id) {
                        *node = BlockNode::with_path(entry.rel_path.clone());
                    }
                }
            } else if let Some(old_node) = self.nodes.get(old_id) {
                match old_node {
                    | BlockNode::Children { children } => {
                        let new_children: Vec<BlockId> =
                            children.iter().filter_map(|c| id_map.get(c).copied()).collect();
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_children(new_children);
                        }
                    }
                    | BlockNode::Mount { path } => {
                        if let Some(node) = sub_nodes.get_mut(new_id) {
                            *node = BlockNode::with_path(path.clone());
                        }
                    }
                }
            }
        }

        // Sub-store roots = re-mapped children of block_id.
        let sub_roots: Vec<BlockId> =
            children.iter().filter_map(|c| id_map.get(c).copied()).collect();
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

        let sub_store = BlockStore::new_with_drafts(
            sub_roots,
            sub_nodes,
            sub_points,
            sub_expansion_drafts,
            sub_reduction_drafts,
        );

        // Write to file.
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| MountError::Read { path: path.to_path_buf(), source: e })?;
        }
        let json = serde_json::to_string_pretty(&sub_store)
            .map_err(|e| MountError::Parse { path: path.to_path_buf(), source: e })?;
        fs::write(path, &json)
            .map_err(|e| MountError::Read { path: path.to_path_buf(), source: e })?;

        // Clean up nested expanded mounts and their blocks.
        for &mount_id in &nested_mounts {
            if let Some(entry) = self.mount_table.remove_entry(mount_id) {
                for &id in &entry.block_ids {
                    self.nodes.remove(id);
                    self.points.remove(id);
                    self.expansion_drafts.remove(id);
                    self.reduction_drafts.remove(id);
                }
            }
        }

        // Remove own subtree nodes from main store (not block_id itself).
        for &id in &own_ids {
            self.nodes.remove(id);
            self.points.remove(id);
            self.expansion_drafts.remove(id);
            self.reduction_drafts.remove(id);
            self.mount_table.remove_origin(id);
        }

        // Compute relative path.
        let rel_path = path
            .strip_prefix(base_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf());

        // Replace node with mount.
        if let Some(node) = self.nodes.get_mut(*block_id) {
            *node = BlockNode::with_path(rel_path);
        }

        Ok(())
    }

    /// Re-key all blocks from `sub_store` into this store with fresh ids.
    ///
    /// Returns `(new_root_ids, all_new_ids)`.
    fn rekey_sub_store(
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
                | BlockNode::Mount { path } => {
                    if let Some(node) = self.nodes.get_mut(new_id) {
                        *node = BlockNode::with_path(path.clone());
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

        (new_roots, all_new_ids)
    }

    /// Resolve a mount path against a base directory.
    ///
    /// If the path is relative, join it with `base_dir`. Otherwise use as-is.
    fn resolve_mount_path(rel_path: &Path, base_dir: &Path) -> std::path::PathBuf {
        if rel_path.is_relative() { base_dir.join(rel_path) } else { rel_path.to_path_buf() }
    }

    fn mount_origin_path(&self, block_id: &BlockId) -> Option<&Path> {
        let origin = self.mount_table.origin(*block_id)?;
        match origin {
            | BlockOrigin::Mounted { mount_point } => {
                self.mount_table.entry(*mount_point).map(|entry| entry.path.as_path())
            }
        }
    }

    fn parent_and_index_of(&self, target: &BlockId) -> Option<(Option<BlockId>, usize)> {
        if let Some(index) = self.roots.iter().position(|id| id == target) {
            return Some((None, index));
        }

        for (parent_id, node) in &self.nodes {
            if let Some(index) = node.children().iter().position(|id| id == target) {
                return Some((Some(parent_id), index));
            }
        }
        None
    }

    /// Return the next block in visible DFS order, skipping collapsed subtrees.
    ///
    /// `collapsed` is the set of block ids whose children are hidden.
    /// Returns `None` when `current` is the last visible block.
    pub fn next_visible_in_dfs(
        &self, current: &BlockId, collapsed: &std::collections::HashSet<BlockId>,
    ) -> Option<BlockId> {
        // If current has visible children, descend into the first child.
        if !collapsed.contains(current) {
            let children = self.children(current);
            if let Some(&first) = children.first() {
                return Some(first);
            }
        }
        // Otherwise walk up ancestors looking for a next sibling.
        let mut target = *current;
        loop {
            let (parent, index) = self.parent_and_index_of(&target)?;
            let siblings = match parent {
                | Some(pid) => self.children(&pid),
                | None => self.roots(),
            };
            if index + 1 < siblings.len() {
                return Some(siblings[index + 1]);
            }
            // No next sibling: move up to parent and retry.
            match parent {
                | Some(pid) => target = pid,
                | None => return None,
            }
        }
    }

    /// Return the previous block in visible DFS order, skipping collapsed subtrees.
    ///
    /// `collapsed` is the set of block ids whose children are hidden.
    /// Returns `None` when `current` is the first visible block.
    pub fn prev_visible_in_dfs(
        &self, current: &BlockId, collapsed: &std::collections::HashSet<BlockId>,
    ) -> Option<BlockId> {
        let (parent, index) = self.parent_and_index_of(current)?;
        if index == 0 {
            // No previous sibling; go to parent (None for root-0 means we are first).
            return parent;
        }
        let siblings = match parent {
            | Some(pid) => self.children(&pid),
            | None => self.roots(),
        };
        // Previous sibling's deepest visible descendant.
        let mut target = siblings[index - 1];
        loop {
            if collapsed.contains(&target) {
                return Some(target);
            }
            let children = self.children(&target);
            if children.is_empty() {
                return Some(target);
            }
            target = *children.last().unwrap();
        }
    }

    fn clone_subtree_with_new_ids(&mut self, source_id: &BlockId) -> Option<BlockId> {
        let source_node = self.node(source_id)?.clone();
        let source_point = self.point(source_id).unwrap_or_default();
        let source_children: Vec<BlockId> = source_node.children().to_vec();
        let mut child_ids = Vec::with_capacity(source_children.len());
        for child in &source_children {
            child_ids.push(self.clone_subtree_with_new_ids(child)?);
        }

        let next_id = self.nodes.insert(BlockNode::with_children(child_ids));
        self.points.insert(next_id, source_point);
        if let Some(mount_point) = self.inherited_mount_point_for_anchor(source_id) {
            self.mount_table.set_origin(next_id, BlockOrigin::Mounted { mount_point });
        }
        Some(next_id)
    }

    fn inherited_mount_point_for_anchor(&self, anchor_id: &BlockId) -> Option<BlockId> {
        if self.mount_table.entry(*anchor_id).is_some() {
            return Some(*anchor_id);
        }

        match self.mount_table.origin(*anchor_id) {
            | Some(BlockOrigin::Mounted { mount_point }) => Some(*mount_point),
            | None => None,
        }
    }

    fn collect_subtree_ids(&self, current: &BlockId, out: &mut Vec<BlockId>) {
        let Some(node) = self.node(current) else {
            return;
        };
        out.push(*current);
        for child in node.children() {
            self.collect_subtree_ids(child, out);
        }
    }

    /// Collect subtree IDs owned by this store, stopping at expanded mount
    /// boundaries.
    ///
    /// `own_ids` receives every block id in the subtree that is not from a
    /// nested mounted file. `mount_points` receives ids of expanded mount
    /// points encountered during traversal (they are also included in
    /// `own_ids` since the mount-point node itself belongs to this store).
    fn collect_own_subtree_ids(
        &self, current: &BlockId, own_ids: &mut Vec<BlockId>, mount_points: &mut Vec<BlockId>,
    ) {
        let Some(node) = self.node(current) else {
            return;
        };
        own_ids.push(*current);

        // If this node is an expanded mount, its children belong to the
        // mounted file. Record it and do not recurse.
        if self.mount_table.entry(*current).is_some() {
            mount_points.push(*current);
            return;
        }

        for child in node.children() {
            self.collect_own_subtree_ids(child, own_ids, mount_points);
        }
    }

    fn collect_lineage_points(
        &self, current: &BlockId, target: &BlockId, out: &mut Vec<String>,
    ) -> bool {
        if !self.nodes.contains_key(*current) {
            return false;
        }

        let point = self.points.get(*current).cloned().unwrap_or_default();
        out.push(point);
        if current == target {
            return true;
        }

        let children = self.node(current).map(|n| n.children().to_vec()).unwrap_or_default();
        for child in &children {
            if self.collect_lineage_points(child, target, out) {
                return true;
            }
        }

        out.pop();
        false
    }

    fn default_store() -> Self {
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();

        let c1 = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(c1, "马克思：《资本论》".to_string());
        let c2 = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(c2, "马克思·韦伯：《新教伦理与资本主义精神》".to_string());
        let c3 = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(c3, "Ivan Zhao: Steam, Steel, and Invisible Minds".to_string());

        let root_id = nodes.insert(BlockNode::with_children(vec![c1, c2, c3]));
        points.insert(root_id, "Notes on liberating productivity".to_string());

        BlockStore::new(vec![root_id], nodes, points)
    }
}

impl Default for BlockStore {
    fn default() -> Self {
        Self::default_store()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a simple store: one root with two children.
    ///
    /// ```text
    /// root("root")
    /// ├── child_a("child_a")
    /// └── child_b("child_b")
    /// ```
    fn simple_store() -> (BlockStore, BlockId, BlockId, BlockId) {
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();

        let child_a = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(child_a, "child_a".to_string());
        let child_b = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(child_b, "child_b".to_string());
        let root = nodes.insert(BlockNode::with_children(vec![child_a, child_b]));
        points.insert(root, "root".to_string());

        let store = BlockStore::new(vec![root], nodes, points);
        (store, root, child_a, child_b)
    }

    // -- BlockId --

    #[test]
    fn block_id_new_produces_distinct_ids() {
        let mut nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let a = nodes.insert(BlockNode::with_children(vec![]));
        let b = nodes.insert(BlockNode::with_children(vec![]));
        assert_ne!(a, b);
    }

    // -- BlockNode --

    #[test]
    fn block_node_stores_children() {
        let child = BlockId::default();
        let node = BlockNode::with_children(vec![child]);
        assert_eq!(node.children(), &[child]);
    }

    // -- Store accessors --

    #[test]
    fn node_returns_some_for_existing_id() {
        let (store, root, _, _) = simple_store();
        assert!(store.node(&root).is_some());
    }

    #[test]
    fn node_returns_none_for_unknown_id() {
        let (store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        assert!(store.node(&unknown).is_none());
    }

    #[test]
    fn point_returns_text_for_known_id() {
        let (store, root, _, _) = simple_store();
        assert_eq!(store.point(&root), Some("root".to_string()));
    }

    #[test]
    fn roots_returns_root_list() {
        let (store, root, _, _) = simple_store();
        assert_eq!(store.roots(), &[root]);
    }

    // -- update_point --

    #[test]
    fn update_point_changes_existing_node() {
        let (mut store, root, _, _) = simple_store();
        store.update_point(&root, "updated".to_string());
        assert_eq!(store.point(&root), Some("updated".to_string()));
    }

    #[test]
    fn update_point_noop_for_unknown_id() {
        let (mut store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        store.update_point(&unknown, "nope".to_string());
    }

    // -- append_child --

    #[test]
    fn append_child_returns_new_id() {
        let (mut store, root, _, _) = simple_store();
        let child_id = store.append_child(&root, "new_child".to_string());
        assert!(child_id.is_some());
    }

    #[test]
    fn append_child_node_exists_with_point() {
        let (mut store, root, _, _) = simple_store();
        let child_id = store.append_child(&root, "new_child".to_string()).unwrap();
        assert_eq!(store.point(&child_id), Some("new_child".to_string()));
    }

    #[test]
    fn append_child_appears_in_parent_children() {
        let (mut store, root, child_a, child_b) = simple_store();
        let child_id = store.append_child(&root, "new_child".to_string()).unwrap();
        let parent = store.node(&root).unwrap();
        assert_eq!(parent.children(), &[child_a, child_b, child_id]);
    }

    #[test]
    fn append_child_returns_none_for_unknown_parent() {
        let (mut store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        assert_eq!(store.append_child(&unknown, "x".to_string()), None);
    }

    // -- append_sibling --

    #[test]
    fn append_sibling_after_root() {
        let (mut store, root, _, _) = simple_store();
        let sibling = store.append_sibling(&root, "sibling".to_string()).unwrap();
        assert_eq!(store.roots(), &[root, sibling]);
    }

    #[test]
    fn append_sibling_after_non_root() {
        let (mut store, root, child_a, child_b) = simple_store();
        let sibling = store.append_sibling(&child_a, "mid".to_string()).unwrap();
        let parent = store.node(&root).unwrap();
        assert_eq!(parent.children(), &[child_a, sibling, child_b]);
    }

    #[test]
    fn append_sibling_returns_none_for_unknown() {
        let (mut store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        assert_eq!(store.append_sibling(&unknown, "x".to_string()), None);
    }

    // -- duplicate_subtree_after --

    #[test]
    fn duplicate_leaf_appears_after_original() {
        let (mut store, root, child_a, child_b) = simple_store();
        let dup = store.duplicate_subtree_after(&child_a).unwrap();
        let parent = store.node(&root).unwrap();
        assert_eq!(parent.children(), &[child_a, dup, child_b]);
        assert_eq!(store.point(&dup), Some("child_a".to_string()));
    }

    #[test]
    fn duplicate_subtree_clones_descendants() {
        let (mut store, _root, child_a, _) = simple_store();
        let grandchild = store.append_child(&child_a, "grandchild".to_string()).unwrap();

        let dup = store.duplicate_subtree_after(&child_a).unwrap();
        let dup_node = store.node(&dup).unwrap();
        assert_eq!(dup_node.children().len(), 1);
        let cloned_grandchild = &dup_node.children()[0];
        assert_ne!(cloned_grandchild, &grandchild);
        assert_eq!(store.point(cloned_grandchild), Some("grandchild".to_string()));

        let orig = store.node(&child_a).unwrap();
        assert_eq!(orig.children(), &[grandchild]);
    }

    #[test]
    fn duplicate_returns_none_for_unknown() {
        let (mut store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        assert_eq!(store.duplicate_subtree_after(&unknown), None);
    }

    // -- remove_block_subtree --

    #[test]
    fn remove_leaf_child_shrinks_parent() {
        let (mut store, root, child_a, child_b) = simple_store();
        let removed = store.remove_block_subtree(&child_a).unwrap();
        assert_eq!(removed, vec![child_a]);
        let parent = store.node(&root).unwrap();
        assert_eq!(parent.children(), &[child_b]);
    }

    #[test]
    fn remove_subtree_removes_all_descendants() {
        let (mut store, _, child_a, _) = simple_store();
        let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();
        let removed = store.remove_block_subtree(&child_a).unwrap();
        assert!(removed.contains(&child_a));
        assert!(removed.contains(&grandchild));
        assert!(store.node(&child_a).is_none());
        assert!(store.node(&grandchild).is_none());
    }

    #[test]
    fn remove_last_root_inserts_fresh_root() {
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let id = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(id, "only".to_string());
        let mut store = BlockStore::new(vec![id], nodes, points);

        store.remove_block_subtree(&id).unwrap();
        assert_eq!(store.roots().len(), 1);
        let new_root = store.roots()[0];
        assert_ne!(new_root, id);
        assert_eq!(store.point(&new_root), Some(String::new()));
    }

    #[test]
    fn remove_returns_none_for_unknown() {
        let (mut store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        assert_eq!(store.remove_block_subtree(&unknown), None);
    }

    // -- lineage_points_for_id --

    #[test]
    fn lineage_root_to_deep_child() {
        let (mut store, _, child_a, _) = simple_store();
        let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();
        let lineage = store.lineage_points_for_id(&grandchild);
        let expected = llm::Lineage::from_points(vec![
            "root".to_string(),
            "child_a".to_string(),
            "gc".to_string(),
        ]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_for_root_is_single_element() {
        let (store, root, _, _) = simple_store();
        let lineage = store.lineage_points_for_id(&root);
        let expected = llm::Lineage::from_points(vec!["root".to_string()]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_for_unknown_is_empty() {
        let (store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        let lineage = store.lineage_points_for_id(&unknown);
        let expected = llm::Lineage::from_points(vec![]);
        assert_eq!(lineage, expected);
    }

    // -- Serialization round-trip --

    #[test]
    fn serde_round_trip_preserves_store() {
        let (store, _, _, _) = simple_store();
        let json = serde_json::to_string(&store).unwrap();
        let restored: BlockStore = serde_json::from_str(&json).unwrap();
        assert_eq!(store, restored);
    }

    #[test]
    fn serde_round_trip_preserves_persisted_drafts() {
        let (mut store, root, child_a, _) = simple_store();
        store.expansion_drafts.insert(
            root,
            ExpansionDraftRecord {
                rewrite: Some("rewrite".to_string()),
                children: vec!["child suggestion".to_string()],
            },
        );
        store
            .reduction_drafts
            .insert(child_a, ReductionDraftRecord { reduction: "reduction".to_string() });

        let json = serde_json::to_string(&store).unwrap();
        let restored: BlockStore = serde_json::from_str(&json).unwrap();

        assert_eq!(store, restored);
        assert!(restored.expansion_draft(&root).is_some());
        assert!(restored.reduction_draft(&child_a).is_some());
    }

    #[test]
    fn remove_subtree_cleans_persisted_drafts() {
        let (mut store, _root, child_a, child_b) = simple_store();
        store.expansion_drafts.insert(
            child_a,
            ExpansionDraftRecord { rewrite: None, children: vec!["draft".to_string()] },
        );
        store
            .reduction_drafts
            .insert(child_b, ReductionDraftRecord { reduction: "draft".to_string() });

        store.remove_block_subtree(&child_a).unwrap();
        store.remove_block_subtree(&child_b).unwrap();

        assert!(store.expansion_draft(&child_a).is_none());
        assert!(store.reduction_draft(&child_b).is_none());
    }

    #[test]
    fn backward_compat_missing_draft_fields_defaults_empty() {
        let (store, _, _, _) = simple_store();
        let mut value = serde_json::to_value(&store).unwrap();
        value.as_object_mut().unwrap().remove("expansion_drafts");
        value.as_object_mut().unwrap().remove("reduction_drafts");

        let restored: BlockStore = serde_json::from_value(value).unwrap();
        assert_eq!(restored.expansion_drafts.len(), 0);
        assert_eq!(restored.reduction_drafts.len(), 0);
    }

    #[test]
    fn load_from_path_returns_parse_error_on_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("broken.json");
        fs::write(&path, "{ not valid json").unwrap();

        let err = BlockStore::load_from_path(&path).unwrap_err();
        assert!(matches!(err, StoreLoadError::Parse { .. }));
    }

    #[test]
    fn load_from_path_with_dangling_child_is_operable_and_normalized_on_save_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("invalid-graph.json");

        let mut nodes = SlotMap::with_key();
        let dangling_child = BlockId::default();
        let root = nodes.insert(BlockNode::with_children(vec![dangling_child]));
        let mut points = SecondaryMap::new();
        points.insert(root, "root".to_string());
        let invalid_store = BlockStore::new(vec![root], nodes, points);
        fs::write(&path, serde_json::to_string_pretty(&invalid_store).unwrap()).unwrap();

        let loaded = BlockStore::load_from_path(&path).unwrap();
        assert!(loaded.node(&root).is_some());
        assert!(loaded.node(&dangling_child).is_none());
        let lineage = loaded.lineage_points_for_id(&root);
        assert_eq!(lineage.points().last(), Some("root"));

        let normalized = loaded.snapshot_for_save();
        let normalized_root = normalized.roots()[0];
        assert_eq!(normalized.node(&normalized_root).unwrap().children().len(), 0);
    }

    // -- expand_mount / collapse_mount --

    fn write_sub_store(dir: &std::path::Path, filename: &str) -> (std::path::PathBuf, BlockStore) {
        let sub = simple_store().0;
        let path = dir.join(filename);
        let json = serde_json::to_string_pretty(&sub).unwrap();
        fs::write(&path, json).unwrap();
        (path, sub)
    }

    #[test]
    fn expand_mount_loads_and_rekeys() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, sub) = write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();

        assert_eq!(new_roots.len(), sub.roots().len());
        assert!(store.node(&mount_id).unwrap().children().len() == new_roots.len());

        for &r in &new_roots {
            assert!(store.node(&r).is_some());
        }
        let entry = store.mount_table().entry(mount_id).unwrap();
        for &r in &new_roots {
            assert!(entry.block_ids.contains(&r));
        }
    }

    #[test]
    fn expand_mount_preserves_points() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let root_point = store.point(&new_roots[0]);
        assert_eq!(root_point, Some("root".to_string()));
    }

    #[test]
    fn expand_mount_errors_on_children_node() {
        let (mut store, root, _, _) = simple_store();
        let result = store.expand_mount(&root, std::path::Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn expand_mount_errors_on_missing_file() {
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id =
            nodes.insert(BlockNode::with_path(std::path::PathBuf::from("nonexistent.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let result = store.expand_mount(&mount_id, std::path::Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn collapse_mount_restores_mount_node() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        assert!(!new_roots.is_empty());

        store.collapse_mount(&mount_id).unwrap();

        assert!(store.node(&mount_id).unwrap().mount_path().is_some());
        for &r in &new_roots {
            assert!(store.node(&r).is_none());
        }
    }

    #[test]
    fn collapse_mount_returns_none_for_unmounted() {
        let (mut store, root, _, _) = simple_store();
        assert!(store.collapse_mount(&root).is_none());
    }

    // -- save-back --

    #[test]
    fn snapshot_excludes_mounted_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        store.expand_mount(&mount_id, tmp.path()).unwrap();

        let snap = store.snapshot_for_save();
        assert_eq!(snap.roots().len(), 1);
        let node = snap.node(&mount_id).unwrap();
        assert!(node.mount_path().is_some());
        assert_eq!(snap.nodes.len(), 1);
    }

    #[test]
    fn save_mounts_writes_updated_points() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        store.update_point(&new_roots[0], "modified root".to_string());
        store.save_mounts().unwrap();

        let saved_json = fs::read_to_string(tmp.path().join("sub.json")).unwrap();
        let saved: BlockStore = serde_json::from_str(&saved_json).unwrap();
        let saved_root_point = saved.point(&saved.roots()[0]);
        assert_eq!(saved_root_point, Some("modified root".to_string()));
    }

    #[test]
    fn expand_mount_allows_duplicate_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_a = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_a, String::new());
        let mount_b = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_b, String::new());
        let mut store = BlockStore::new(vec![mount_a, mount_b], nodes, points);

        store.expand_mount(&mount_a, tmp.path()).unwrap();
        let second = store.expand_mount(&mount_b, tmp.path()).unwrap();
        assert!(!second.is_empty());
        assert!(!store.children(&mount_b).is_empty());
    }

    #[test]
    fn expand_mount_allows_after_collapse() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_a = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_a, String::new());
        let mount_b = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_b, String::new());
        let mut store = BlockStore::new(vec![mount_a, mount_b], nodes, points);

        store.expand_mount(&mount_a, tmp.path()).unwrap();
        store.collapse_mount(&mount_a).unwrap();
        store.expand_mount(&mount_b, tmp.path()).unwrap();
        assert!(!store.children(&mount_b).is_empty());
    }

    #[test]
    fn collapse_mount_restores_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        store.expand_mount(&mount_id, tmp.path()).unwrap();
        store.collapse_mount(&mount_id).unwrap();

        let path = store.node(&mount_id).unwrap().mount_path().unwrap();
        assert_eq!(path, std::path::Path::new("sub.json"));
    }

    #[test]
    fn clone_preserves_mount_table_for_undo() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let snapshot = store.clone();
        assert!(snapshot.node(&mount_id).unwrap().mount_path().is_some());

        store.expand_mount(&mount_id, tmp.path()).unwrap();
        assert!(!store.node(&mount_id).unwrap().mount_path().is_some());
        assert!(!store.children(&mount_id).is_empty());

        // Restoring the snapshot should give back the unexpanded mount.
        let restored = snapshot;
        assert!(restored.node(&mount_id).unwrap().mount_path().is_some());
        assert!(restored.children(&mount_id).is_empty());
        assert!(restored.mount_table().entry(mount_id).is_none());
    }

    // -- integration: nested mounts --

    fn write_store(dir: &std::path::Path, filename: &str, store: &BlockStore) {
        let path = dir.join(filename);
        let json = serde_json::to_string_pretty(store).unwrap();
        fs::write(&path, json).unwrap();
    }

    #[test]
    fn nested_mount_expands_recursively() {
        let tmp = tempfile::tempdir().unwrap();

        let (inner_store, _, _, _) = simple_store();
        write_store(tmp.path(), "inner.json", &inner_store);

        let mut outer_nodes = SlotMap::with_key();
        let mut outer_points = SecondaryMap::new();
        let inner_mount =
            outer_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("inner.json")));
        outer_points.insert(inner_mount, String::new());
        let outer_root = outer_nodes.insert(BlockNode::with_children(vec![inner_mount]));
        outer_points.insert(outer_root, "outer root".to_string());
        let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
        write_store(tmp.path(), "outer.json", &outer_store);

        let mut main_nodes = SlotMap::with_key();
        let mut main_points = SecondaryMap::new();
        let outer_mount =
            main_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("outer.json")));
        main_points.insert(outer_mount, String::new());
        let mut store = BlockStore::new(vec![outer_mount], main_nodes, main_points);

        let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
        assert_eq!(outer_children.len(), 1);

        let rekeyed_outer_root = outer_children[0];
        let nested_mount_candidates: Vec<BlockId> = store
            .children(&rekeyed_outer_root)
            .iter()
            .filter(|id| store.node(id).unwrap().mount_path().is_some())
            .copied()
            .collect();
        assert_eq!(nested_mount_candidates.len(), 1);

        let nested_mount_id = nested_mount_candidates[0];
        let inner_children = store.expand_mount(&nested_mount_id, tmp.path()).unwrap();
        assert_eq!(inner_children.len(), 1);
        assert_eq!(store.point(&inner_children[0]), Some("root".to_string()));
    }

    #[test]
    fn nested_mount_path_resolves_relative_to_parent_mount_file() {
        let tmp = tempfile::tempdir().unwrap();
        let nested_dir = tmp.path().join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        let (inner_store, _, _, _) = simple_store();
        write_store(&nested_dir, "inner.json", &inner_store);

        let mut outer_nodes = SlotMap::with_key();
        let mut outer_points = SecondaryMap::new();
        let inner_mount =
            outer_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("inner.json")));
        outer_points.insert(inner_mount, String::new());
        let outer_root = outer_nodes.insert(BlockNode::with_children(vec![inner_mount]));
        outer_points.insert(outer_root, "outer root".to_string());
        let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
        write_store(&nested_dir, "outer.json", &outer_store);

        let mut main_nodes = SlotMap::with_key();
        let mut main_points = SecondaryMap::new();
        let outer_mount =
            main_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("nested/outer.json")));
        main_points.insert(outer_mount, String::new());
        let mut store = BlockStore::new(vec![outer_mount], main_nodes, main_points);

        let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
        let rekeyed_outer_root = outer_children[0];
        let nested_mount = *store
            .children(&rekeyed_outer_root)
            .iter()
            .find(|id| store.node(id).unwrap().mount_path().is_some())
            .unwrap();

        let inner_children = store.expand_mount(&nested_mount, tmp.path()).unwrap();
        assert_eq!(inner_children.len(), 1);
        assert_eq!(store.point(&inner_children[0]), Some("root".to_string()));
    }

    #[test]
    fn save_mounts_preserves_nested_mount_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        let nested_dir = tmp.path().join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        let (inner_store, _, _, _) = simple_store();
        write_store(&nested_dir, "inner.json", &inner_store);

        let mut outer_nodes = SlotMap::with_key();
        let mut outer_points = SecondaryMap::new();
        let inner_mount =
            outer_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("inner.json")));
        outer_points.insert(inner_mount, String::new());
        let outer_root = outer_nodes.insert(BlockNode::with_children(vec![inner_mount]));
        outer_points.insert(outer_root, "outer root".to_string());
        let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
        write_store(&nested_dir, "outer.json", &outer_store);

        let mut main_nodes = SlotMap::with_key();
        let mut main_points = SecondaryMap::new();
        let outer_mount =
            main_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("nested/outer.json")));
        main_points.insert(outer_mount, String::new());
        let mut store = BlockStore::new(vec![outer_mount], main_nodes, main_points);

        let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
        let rekeyed_outer_root = outer_children[0];
        let nested_mount = *store
            .children(&rekeyed_outer_root)
            .iter()
            .find(|id| store.node(id).unwrap().mount_path().is_some())
            .unwrap();
        let inner_children = store.expand_mount(&nested_mount, tmp.path()).unwrap();
        store.update_point(&inner_children[0], "edited nested root".to_string());

        store.save_mounts().unwrap();

        let outer_json = fs::read_to_string(nested_dir.join("outer.json")).unwrap();
        let saved_outer: BlockStore = serde_json::from_str(&outer_json).unwrap();
        let saved_outer_root = saved_outer.roots()[0];
        let saved_nested_mount = saved_outer.children(&saved_outer_root)[0];
        let saved_nested_path = saved_outer.node(&saved_nested_mount).unwrap().mount_path();
        assert_eq!(saved_nested_path, Some(std::path::Path::new("inner.json")));

        let inner_json = fs::read_to_string(nested_dir.join("inner.json")).unwrap();
        let saved_inner: BlockStore = serde_json::from_str(&inner_json).unwrap();
        assert_eq!(
            saved_inner.point(&saved_inner.roots()[0]),
            Some("edited nested root".to_string())
        );
    }

    // -- integration: round-trip persistence --

    #[test]
    fn mount_edit_save_collapse_remount_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let roots_1 = store.expand_mount(&mount_id, tmp.path()).unwrap();
        store.update_point(&roots_1[0], "edited root".to_string());
        store.save_mounts().unwrap();

        store.collapse_mount(&mount_id).unwrap();
        assert!(store.node(&mount_id).unwrap().mount_path().is_some());

        let roots_2 = store.expand_mount(&mount_id, tmp.path()).unwrap();
        assert_eq!(store.point(&roots_2[0]), Some("edited root".to_string()));
    }

    #[test]
    fn mount_save_persists_new_deep_non_mounted_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let roots_1 = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let root = roots_1[0];
        let child_a = store.children(&root)[0];
        let deep_child = store.append_child(&child_a, "deep child".to_string()).unwrap();
        store.append_child(&deep_child, "deep grandchild".to_string()).unwrap();

        store.save_mounts().unwrap();
        store.collapse_mount(&mount_id).unwrap();

        let roots_2 = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let reloaded_root = roots_2[0];
        let reloaded_child_a = store.children(&reloaded_root)[0];
        let reloaded_deep_child = *store
            .children(&reloaded_child_a)
            .iter()
            .find(|id| store.point(id) == Some("deep child".to_string()))
            .unwrap();
        let reloaded_deep_grandchild = store.children(&reloaded_deep_child)[0];
        assert_eq!(store.point(&reloaded_deep_grandchild), Some("deep grandchild".to_string()));
    }

    #[test]
    fn mount_save_persists_new_sibling_under_mounted_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let root = roots[0];
        let first_child = store.children(&root)[0];
        store.append_sibling(&first_child, "sibling created in mounted file".to_string()).unwrap();

        store.save_mounts().unwrap();
        store.collapse_mount(&mount_id).unwrap();
        let reloaded_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let reloaded_root = reloaded_roots[0];
        let has_new_sibling = store
            .children(&reloaded_root)
            .iter()
            .any(|id| store.point(id) == Some("sibling created in mounted file".to_string()));
        assert!(has_new_sibling);
    }

    #[test]
    fn mount_save_persists_duplicated_subtree_under_mounted_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let root = roots[0];
        let first_child = store.children(&root)[0];
        let duplicated = store.duplicate_subtree_after(&first_child).unwrap();
        store.update_point(&duplicated, "duplicated mounted node".to_string());

        store.save_mounts().unwrap();
        store.collapse_mount(&mount_id).unwrap();
        let reloaded_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let reloaded_root = reloaded_roots[0];
        let has_duplicate = store
            .children(&reloaded_root)
            .iter()
            .any(|id| store.point(id) == Some("duplicated mounted node".to_string()));
        assert!(has_duplicate);
    }

    #[test]
    fn collapse_mount_discards_unsaved_new_descendants() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let root = roots[0];
        let transient = store.append_child(&root, "transient unsaved child".to_string()).unwrap();

        store.collapse_mount(&mount_id).unwrap();
        assert!(store.node(&transient).is_none());
        assert!(store.mount_table.origin(transient).is_none());

        let reloaded_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let reloaded_root = reloaded_roots[0];
        let still_has_transient = store
            .children(&reloaded_root)
            .iter()
            .any(|id| store.point(id) == Some("transient unsaved child".to_string()));
        assert!(!still_has_transient);
    }

    #[test]
    fn nested_mount_save_persists_new_descendants_in_inner_file() {
        let tmp = tempfile::tempdir().unwrap();
        let nested_dir = tmp.path().join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        let (inner_store, _, _, _) = simple_store();
        write_store(&nested_dir, "inner.json", &inner_store);

        let mut outer_nodes = SlotMap::with_key();
        let mut outer_points = SecondaryMap::new();
        let inner_mount =
            outer_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("inner.json")));
        outer_points.insert(inner_mount, String::new());
        let outer_root = outer_nodes.insert(BlockNode::with_children(vec![inner_mount]));
        outer_points.insert(outer_root, "outer root".to_string());
        let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
        write_store(&nested_dir, "outer.json", &outer_store);

        let mut main_nodes = SlotMap::with_key();
        let mut main_points = SecondaryMap::new();
        let outer_mount =
            main_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("nested/outer.json")));
        main_points.insert(outer_mount, String::new());
        let mut store = BlockStore::new(vec![outer_mount], main_nodes, main_points);

        let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
        let rekeyed_outer_root = outer_children[0];
        let nested_mount = *store
            .children(&rekeyed_outer_root)
            .iter()
            .find(|id| store.node(id).unwrap().mount_path().is_some())
            .unwrap();
        let inner_children = store.expand_mount(&nested_mount, tmp.path()).unwrap();
        let inner_root = inner_children[0];
        let added = store.append_child(&inner_root, "new inner child".to_string()).unwrap();
        store.append_child(&added, "new inner grandchild".to_string()).unwrap();

        store.save_mounts().unwrap();
        store.collapse_mount(&outer_mount).unwrap();

        let reloaded_outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
        let reloaded_outer_root = reloaded_outer_children[0];
        let reloaded_nested_mount = *store
            .children(&reloaded_outer_root)
            .iter()
            .find(|id| store.node(id).unwrap().mount_path().is_some())
            .unwrap();
        let reloaded_inner_children =
            store.expand_mount(&reloaded_nested_mount, tmp.path()).unwrap();
        let reloaded_inner_root = reloaded_inner_children[0];
        let reloaded_added = *store
            .children(&reloaded_inner_root)
            .iter()
            .find(|id| store.point(id) == Some("new inner child".to_string()))
            .unwrap();
        let reloaded_grandchild = store.children(&reloaded_added)[0];
        assert_eq!(store.point(&reloaded_grandchild), Some("new inner grandchild".to_string()));
    }

    #[test]
    fn snapshot_excludes_new_nodes_under_expanded_mount() {
        let tmp = tempfile::tempdir().unwrap();
        write_sub_store(tmp.path(), "sub.json");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("sub.json")));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        let root = roots[0];
        store.append_child(&root, "unsaved-in-main".to_string()).unwrap();

        let snapshot = store.snapshot_for_save();
        let has_mount = snapshot.nodes.iter().any(
            |(_, node)| matches!(node, BlockNode::Mount { path } if path == std::path::Path::new("sub.json")),
        );
        assert!(has_mount);
        let leaks_new_mounted_node =
            snapshot.points.iter().any(|(_, point)| point == "unsaved-in-main");
        assert!(!leaks_new_mounted_node);
    }

    #[test]
    fn nested_self_reference_can_expand_lazily() {
        let tmp = tempfile::tempdir().unwrap();

        let mut self_nodes = SlotMap::with_key();
        let mut self_points = SecondaryMap::new();
        let inner_mount =
            self_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("self.json")));
        self_points.insert(inner_mount, String::new());
        let self_root = self_nodes.insert(BlockNode::with_children(vec![inner_mount]));
        self_points.insert(self_root, "self-ref root".to_string());
        let self_store = BlockStore::new(vec![self_root], self_nodes, self_points);
        write_store(tmp.path(), "self.json", &self_store);

        let mut main_nodes = SlotMap::with_key();
        let mut main_points = SecondaryMap::new();
        let main_mount =
            main_nodes.insert(BlockNode::with_path(std::path::PathBuf::from("self.json")));
        main_points.insert(main_mount, String::new());
        let mut store = BlockStore::new(vec![main_mount], main_nodes, main_points);

        let roots = store.expand_mount(&main_mount, tmp.path()).unwrap();
        let rekeyed_root = roots[0];
        let nested: Vec<BlockId> = store
            .children(&rekeyed_root)
            .iter()
            .filter(|id| store.node(id).unwrap().mount_path().is_some())
            .copied()
            .collect();
        assert_eq!(nested.len(), 1);

        let inner_roots = store.expand_mount(&nested[0], tmp.path()).unwrap();
        assert_eq!(inner_roots.len(), 1);
        assert_eq!(store.point(&inner_roots[0]), Some("self-ref root".to_string()));
    }
}

impl PartialEq for BlockStore {
    fn eq(&self, other: &Self) -> bool {
        self.roots == other.roots
            && self.nodes.len() == other.nodes.len()
            && self.nodes.iter().all(|(id, node)| other.nodes.get(id) == Some(node))
            && self.points.len() == other.points.len()
            && self.points.iter().all(|(id, pt)| other.points.get(id) == Some(pt))
            && self.expansion_drafts.len() == other.expansion_drafts.len()
            && self
                .expansion_drafts
                .iter()
                .all(|(id, draft)| other.expansion_drafts.get(id) == Some(draft))
            && self.reduction_drafts.len() == other.reduction_drafts.len()
            && self
                .reduction_drafts
                .iter()
                .all(|(id, draft)| other.reduction_drafts.get(id) == Some(draft))
    }
}
