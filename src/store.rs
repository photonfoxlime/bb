//! Block store: the core document data model.
//!
//! A document is a forest of blocks. Each block has a slotmap identity, a text
//! point, and ordered children. Persistence and mount invariants are documented
//! on the owning save/expand/collapse functions below.
//!
//! # Adding per-block persistent data
//!
//! The store uses [`SparseSecondaryMap<BlockId, T>`] for optional per-block
//! metadata that must survive save/load cycles. Existing examples:
//! `expansion_drafts`, `reduction_drafts`, `view_collapsed`, and
//! `friend_blocks`.
//!
//! Checklist for a new field:
//!
//! 1. Declare the field on [`BlockStore`] with `#[serde(default)]`.
//!    Use `bool` rather than `()` as the value type for set-like maps;
//!    `SparseSecondaryMap<_, ()>` fails to round-trip through serde.
//! 2. Thread through [`BlockStore::new_with_drafts`] (the internal
//!    constructor used by `new` and all projection paths).
//! 3. Accessor methods, at minimum a read accessor and a mutating
//!    method. See `is_collapsed` / `toggle_collapsed` for the set-like
//!    pattern.
//! 4. Remap in [`BlockStore::build_projected_store`] which iterate the old
//!    map, translate keys through `id_map`, insert into the new sub-map,
//!    and pass it to `new_with_drafts`.
//! 5. Import in [`BlockStore::rekey_sub_store`] with same key translation
//!    but inserting into `self` rather than a fresh store.
//! 6. Clean up on removal. Add `.remove(id)` calls in:
//!    - [`BlockStore::remove_block_subtree`]
//!    - [`BlockStore::collapse_mount`] (two sites: own ids and nested mount ids)
//!    - [`BlockStore::save_subtree_to_file`] (two sites: nested mount cleanup
//!      and own-ids cleanup)
//! 7. Update [`PartialEq`] by adding a length + element comparison clause.
//! 8. Tests, at minimum: serde round-trip, backward-compat (missing key
//!    defaults to empty), and cleanup-on-removal.

use crate::llm;
use crate::mount::{BlockOrigin, MountEntry, MountError, MountTable};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap, SparseSecondaryMap};
use std::path::Path;
use std::{fs, io};
use thiserror::Error;

#[derive(Debug, Clone)]
struct MountProjection {
    path: std::path::PathBuf,
    format: MountFormat,
}

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
///
/// When `redundant_children` is non-empty, the reduction draft suggests that
/// those children are captured by the condensed text and can be deleted.
/// The [`BlockId`]s are resolved at response time from the LLM's returned
/// indices into the children snapshot that was sent with the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReductionDraftRecord {
    pub reduction: String,
    /// Children whose information is captured by the reduction.
    ///
    /// May contain stale ids if children were modified between response
    /// arrival and apply time; consumers must filter at render and apply.
    #[serde(default)]
    pub redundant_children: Vec<BlockId>,
}

/// Persisted instruction draft text keyed by target [`BlockId`].
///
/// Stores per-block instruction-editor input so drafts survive reloads and
/// round-trips through mount projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionDraftRecord {
    pub instruction: String,
}

/// Persisted inquiry draft payload keyed by target [`BlockId`].
///
/// This captures the latest inquiry response for a target block until the user
/// applies or dismisses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InquiryDraftRecord {
    pub response: String,
}

/// Persisted friend relation from a source block to a target block.
///
/// Friend blocks are user-selected related context for a block: they are not
/// children but extra blocks whose text (and optional perspective) is included
/// when building LLM context for reduce/expand. The block that "has" the
/// friends is the *source* (key in `BlockStore::friend_blocks`); each
/// [`FriendBlock`] points to another block in the graph and an optional
/// framing string (perspective) for how the source should interpret that friend.
///
/// `block_id` points to the friend block in the main store graph.
/// `perspective` is optional source-authored framing text that describes how
/// the source block should interpret that friend block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendBlock {
    /// Target friend block id.
    pub block_id: BlockId,
    /// Optional source-authored framing for this friend relation.
    #[serde(default)]
    pub perspective: Option<String>,
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

/// Persisted format for mount files referenced by [`BlockNode::Mount`].
///
/// `Json` remains the default for backward compatibility with existing files
/// that only stored `path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MountFormat {
    /// Canonical store JSON encoding used for full-fidelity mount round-trips.
    Json,
    /// Markdown Mount v1 encoding produced by [`BlockStore::render_markdown_mount_store`].
    Markdown,
}

impl Default for MountFormat {
    fn default() -> Self {
        Self::Json
    }
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
    Mount {
        path: std::path::PathBuf,
        #[serde(default)]
        format: MountFormat,
    },
}

impl BlockNode {
    /// Create an inline-children node with the given child ids.
    pub fn with_children(children: Vec<BlockId>) -> Self {
        Self::Children { children }
    }

    /// Create a mount-point node referencing an external file.
    pub fn with_path(path: std::path::PathBuf) -> Self {
        Self::with_path_and_format(path, MountFormat::Json)
    }

    /// Create a mount-point node with an explicit persisted file format.
    pub fn with_path_and_format(path: std::path::PathBuf, format: MountFormat) -> Self {
        Self::Mount { path, format }
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
            | Self::Mount { path, .. } => Some(path),
        }
    }

    /// Return the persisted mount format if this is a mount node.
    pub fn mount_format(&self) -> Option<MountFormat> {
        match self {
            | Self::Children { .. } => None,
            | Self::Mount { format, .. } => Some(*format),
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
    /// Sparse by design: only blocks with pending expansion drafts are stored.
    #[serde(default)]
    expansion_drafts: SparseSecondaryMap<BlockId, ExpansionDraftRecord>,
    /// Persisted per-block reduction drafts.
    ///
    /// Invariant: keys should reference existing blocks in `nodes`.
    /// Sparse by design: only blocks with pending reduction drafts are stored.
    #[serde(default)]
    reduction_drafts: SparseSecondaryMap<BlockId, ReductionDraftRecord>,
    /// Persisted per-block instruction drafts.
    #[serde(default)]
    instruction_drafts: SparseSecondaryMap<BlockId, InstructionDraftRecord>,
    /// Persisted per-block inquiry drafts.
    #[serde(default)]
    inquiry_drafts: SparseSecondaryMap<BlockId, InquiryDraftRecord>,
    /// Persisted per-block fold (collapse) state.
    ///
    /// Presence of a key means the block's children are hidden in the UI.
    /// Stored in the authoritative graph so fold state survives reloads,
    /// participates in undo/redo snapshots, and follows save/load id remapping.
    #[serde(default)]
    view_collapsed: SparseSecondaryMap<BlockId, bool>,
    #[serde(default)]
    friend_blocks: SparseSecondaryMap<BlockId, Vec<FriendBlock>>,
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
        Self::new_with_drafts(
            roots,
            nodes,
            points,
            SparseSecondaryMap::new(),
            SparseSecondaryMap::new(),
            SparseSecondaryMap::new(),
            SparseSecondaryMap::new(),
            SparseSecondaryMap::new(),
            SparseSecondaryMap::new(),
        )
    }

    fn new_with_drafts(
        roots: Vec<BlockId>, nodes: SlotMap<BlockId, BlockNode>,
        points: SecondaryMap<BlockId, String>,
        expansion_drafts: SparseSecondaryMap<BlockId, ExpansionDraftRecord>,
        reduction_drafts: SparseSecondaryMap<BlockId, ReductionDraftRecord>,
        instruction_drafts: SparseSecondaryMap<BlockId, InstructionDraftRecord>,
        inquiry_drafts: SparseSecondaryMap<BlockId, InquiryDraftRecord>,
        view_collapsed: SparseSecondaryMap<BlockId, bool>,
        friend_blocks: SparseSecondaryMap<BlockId, Vec<FriendBlock>>,
    ) -> Self {
        Self {
            roots,
            nodes,
            points,
            mount_table: MountTable::new(),
            expansion_drafts,
            reduction_drafts,
            instruction_drafts,
            inquiry_drafts,
            view_collapsed,
            friend_blocks,
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

    pub fn instruction_draft(&self, id: &BlockId) -> Option<&InstructionDraftRecord> {
        self.instruction_drafts.get(*id)
    }

    pub fn set_instruction_draft(&mut self, id: BlockId, instruction: String) {
        if instruction.is_empty() {
            self.instruction_drafts.remove(id);
        } else {
            self.instruction_drafts.insert(id, InstructionDraftRecord { instruction });
        }
    }

    pub fn remove_instruction_draft(&mut self, id: &BlockId) -> Option<InstructionDraftRecord> {
        self.instruction_drafts.remove(*id)
    }

    pub fn inquiry_draft(&self, id: &BlockId) -> Option<&InquiryDraftRecord> {
        self.inquiry_drafts.get(*id)
    }

    pub fn set_inquiry_draft(&mut self, id: BlockId, response: String) {
        let trimmed = response.trim();
        if trimmed.is_empty() {
            self.inquiry_drafts.remove(id);
        } else {
            self.inquiry_drafts.insert(id, InquiryDraftRecord { response: trimmed.to_string() });
        }
    }

    pub fn remove_inquiry_draft(&mut self, id: &BlockId) -> Option<InquiryDraftRecord> {
        self.inquiry_drafts.remove(*id)
    }

    /// Whether the given block's children are folded (hidden) in the UI.
    pub fn is_collapsed(&self, id: &BlockId) -> bool {
        self.view_collapsed.contains_key(*id)
    }

    /// Toggle the fold state of a block. Returns the new state (`true` = collapsed).
    pub fn toggle_collapsed(&mut self, id: &BlockId) -> bool {
        if self.view_collapsed.remove(*id).is_some() {
            false
        } else {
            self.view_collapsed.insert(*id, true);
            true
        }
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

        self.build_projected_store(&kept_ids, &self.roots, &mount_path_overrides)
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

        self.build_projected_store(&own_ids, &root_ids, &mount_path_overrides)
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
        if let Some(parent) = self.nodes.get_mut(*parent_id)
            && let Some(children) = parent.children_mut()
        {
            children.push(child_id);
        }
        Some(child_id)
    }

    /// Wrap a block with a new parent inserted at the block's current position.
    ///
    /// Preserves sibling/root ordering by replacing the original slot with the
    /// new parent and attaching the target block as its first child.
    pub fn insert_parent(&mut self, block_id: &BlockId, point: String) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;

        let parent_block_id = self.nodes.insert(BlockNode::with_children(vec![*block_id]));
        self.points.insert(parent_block_id, point);

        if let Some(mount_point) = self.inherited_mount_point_for_anchor(block_id) {
            self.mount_table.set_origin(parent_block_id, BlockOrigin::Mounted { mount_point });
        }

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(parent_id)?;
            if let Some(children) = parent.children_mut() {
                children[index] = parent_block_id;
            }
        } else {
            self.roots[index] = parent_block_id;
        }

        Some(parent_block_id)
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
            if let Some(parent) = self.nodes.get_mut(parent_id)
                && let Some(children) = parent.children_mut()
            {
                children.remove(index);
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
            self.instruction_drafts.remove(*id);
            self.inquiry_drafts.remove(*id);
            self.view_collapsed.remove(*id);
            self.friend_blocks.remove(*id);
            self.mount_table.remove_origin(*id);
        }
        self.remove_friend_block_references(&removed_ids);

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

    /// Build a [`llm::BlockContext`] for the given block from all visible context.
    ///
    /// Visibility model:
    /// - target point (as the final lineage item),
    /// - parent chain (earlier lineage items),
    /// - direct children point texts,
    /// - user-selected friend blocks.
    ///
    /// Used by inquire/reduce/expand handlers so all three operations read the
    /// same context envelope.
    pub fn block_context_for_id(&self, target: &BlockId) -> llm::BlockContext {
        let friend_ids = self.friend_blocks.get(*target).cloned().unwrap_or_default();
        self.block_context_for_id_with_friend_blocks(target, &friend_ids)
    }

    /// Build a [`llm::BlockContext`] with user-selected friend blocks.
    ///
    /// Friend blocks are extra readable blocks outside the target's direct
    /// children and may include an optional per-friend perspective.
    pub fn block_context_for_id_with_friend_blocks(
        &self, target: &BlockId, friend_block_ids: &[FriendBlock],
    ) -> llm::BlockContext {
        let lineage = self.lineage_points_for_id(target);
        let existing_children = self
            .children(target)
            .iter()
            .filter_map(|child_id| self.point(child_id))
            .collect::<Vec<_>>();
        let friend_blocks = friend_block_ids
            .iter()
            .filter_map(|friend| {
                self.point(&friend.block_id)
                    .map(|point| llm::FriendContext::new(point, friend.perspective.clone()))
            })
            .collect::<Vec<_>>();
        llm::BlockContext::new(lineage, existing_children, friend_blocks)
    }

    pub fn friend_blocks_for(&self, target: &BlockId) -> &[FriendBlock] {
        self.friend_blocks.get(*target).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn set_friend_blocks_for(&mut self, target: &BlockId, friend_block_ids: Vec<FriendBlock>) {
        if friend_block_ids.is_empty() {
            self.friend_blocks.remove(*target);
        } else {
            self.friend_blocks.insert(*target, friend_block_ids);
        }
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

        let effective_base_dir = self
            .mount_origin_path(mount_point)
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| base_dir.to_path_buf());
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
            self.mount_table.remove_origin(*id);
        }
        self.remove_friend_block_references(&removed_ids);
        if let Some(node) = self.nodes.get_mut(*mount_point) {
            *node = BlockNode::with_path_and_format(entry.rel_path, entry.format);
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
        let sub_store = self.build_projected_store(&own_ids, &children, &mount_path_overrides);

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
        &self, kept_ids: &[BlockId], roots: &[BlockId],
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
                    })
                })
                .collect::<Vec<_>>();
            if !remapped.is_empty() {
                sub_friend_blocks.insert(new_target_id, remapped);
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
        )
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
                    })
                })
                .collect::<Vec<_>>();
            if !remapped.is_empty() {
                self.friend_blocks.insert(new_target_id, remapped);
            }
        }
        (new_roots, all_new_ids)
    }

    fn remove_friend_block_references(&mut self, removed_ids: &[BlockId]) {
        if removed_ids.is_empty() || self.friend_blocks.is_empty() {
            return;
        }
        let removed = removed_ids.iter().copied().collect::<std::collections::HashSet<_>>();
        let target_ids = self.friend_blocks.iter().map(|(id, _)| id).collect::<Vec<_>>();
        let mut empty_targets = Vec::new();
        for target_id in target_ids {
            if let Some(friend_ids) = self.friend_blocks.get_mut(target_id) {
                friend_ids.retain(|friend| !removed.contains(&friend.block_id));
                if friend_ids.is_empty() {
                    empty_targets.push(target_id);
                }
            }
        }
        for target_id in empty_targets {
            self.friend_blocks.remove(target_id);
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

    /// Render a projected mount store into Markdown Mount v1.
    ///
    /// Mapping rules (block graph -> markdown):
    ///
    /// 1. Emit a required preamble line:
    ///    `<!-- bb-mount format=markdown v1 -->`.
    /// 2. Emit each root block as a top-level list item in root order.
    /// 3. Emit each child block as a nested list item in child order.
    /// 4. Indent nested list items by two spaces per depth level.
    /// 5. Serialize each block point as a double-quoted scalar:
    ///    `- "<escaped-point>"`.
    /// 6. Escape point text with [`Self::escape_markdown_point`].
    ///
    /// Notes:
    /// - This projection intentionally writes only structural hierarchy and
    ///   point text for parser-friendly, deterministic output.
    /// - Runtime-only metadata (drafts, fold state, mount table) is excluded.
    fn render_markdown_mount_store(store: &BlockStore) -> String {
        let mut output = String::from("<!-- bb-mount format=markdown v1 -->\n");
        for &root in store.roots() {
            Self::render_markdown_node(store, root, 0, &mut output);
        }
        output
    }

    /// Parse Markdown Mount v1 into a projected mount store.
    ///
    /// The parser accepts exactly the markdown structure emitted by
    /// [`Self::render_markdown_mount_store`]: preamble line + two-space nested
    /// bullet list with quoted and escaped point text.
    fn parse_markdown_mount_store(markdown: &str) -> Result<BlockStore, String> {
        let mut nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let mut points: SecondaryMap<BlockId, String> = SecondaryMap::new();
        let mut roots: Vec<BlockId> = Vec::new();
        let mut path_by_depth: Vec<BlockId> = Vec::new();

        let mut saw_preamble = false;
        let mut saw_item = false;

        for (line_index, raw_line) in markdown.lines().enumerate() {
            let line_no = line_index + 1;
            let line = raw_line.trim_end();
            if line.trim().is_empty() {
                continue;
            }
            if !saw_preamble {
                if line == "<!-- bb-mount format=markdown v1 -->" {
                    saw_preamble = true;
                    continue;
                }
                return Err(format!(
                    "line {}: missing markdown mount preamble '<!-- bb-mount format=markdown v1 -->'",
                    line_no
                ));
            }

            let depth_spaces = raw_line.chars().take_while(|ch| *ch == ' ').count();
            if depth_spaces % 2 != 0 {
                return Err(format!(
                    "line {}: indentation must be multiples of two spaces",
                    line_no
                ));
            }
            let depth = depth_spaces / 2;
            let trimmed = &raw_line[depth_spaces..];

            if !trimmed.starts_with("- \"") || !trimmed.ends_with('"') {
                return Err(format!("line {}: expected '- \"...\"' markdown list item", line_no));
            }

            let quoted_content = &trimmed[3..trimmed.len() - 1];
            let point = Self::unescape_markdown_point(quoted_content)
                .map_err(|reason| format!("line {}: {}", line_no, reason))?;

            if depth > path_by_depth.len() {
                return Err(format!(
                    "line {}: indentation depth jumps more than one level",
                    line_no
                ));
            }
            path_by_depth.truncate(depth);

            let id = nodes.insert(BlockNode::with_children(vec![]));
            points.insert(id, point);
            saw_item = true;

            if depth == 0 {
                roots.push(id);
            } else {
                let Some(parent_id) = path_by_depth.get(depth - 1).copied() else {
                    return Err(format!(
                        "line {}: missing parent block at depth {}",
                        line_no,
                        depth - 1
                    ));
                };
                let Some(parent) = nodes.get_mut(parent_id) else {
                    return Err(format!("line {}: parent block does not exist", line_no));
                };
                let Some(children) = parent.children_mut() else {
                    return Err(format!("line {}: parent block is not a children node", line_no));
                };
                children.push(id);
            }

            path_by_depth.push(id);
        }

        if !saw_preamble {
            return Err("missing markdown mount preamble '<!-- bb-mount format=markdown v1 -->'"
                .to_string());
        }
        if !saw_item {
            return Err("markdown mount file contains no block items".to_string());
        }

        Ok(BlockStore::new(roots, nodes, points))
    }

    /// Emit one block as a markdown list item, then recurse into children.
    fn render_markdown_node(store: &BlockStore, id: BlockId, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        let point = store.point(&id).unwrap_or_default();
        let escaped = Self::escape_markdown_point(&point);
        out.push_str(&indent);
        out.push_str("- \"");
        out.push_str(&escaped);
        out.push_str("\"\n");
        for child in store.children(&id) {
            Self::render_markdown_node(store, *child, depth + 1, out);
        }
    }

    /// Escape point text used in markdown quoted scalars.
    ///
    /// Escapes: `\\`, `"`, `\n`, `\r`, and `\t`.
    fn escape_markdown_point(point: &str) -> String {
        let mut escaped = String::with_capacity(point.len());
        for ch in point.chars() {
            match ch {
                | '\\' => escaped.push_str("\\\\"),
                | '"' => escaped.push_str("\\\""),
                | '\n' => escaped.push_str("\\n"),
                | '\r' => escaped.push_str("\\r"),
                | '\t' => escaped.push_str("\\t"),
                | _ => escaped.push(ch),
            }
        }
        escaped
    }

    /// Unescape point text parsed from markdown quoted scalars.
    ///
    /// Supports the exact escapes emitted by [`Self::escape_markdown_point`].
    fn unescape_markdown_point(point: &str) -> Result<String, String> {
        let mut chars = point.chars();
        let mut out = String::with_capacity(point.len());

        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            let Some(next) = chars.next() else {
                return Err("trailing backslash in escaped point".to_string());
            };
            match next {
                | '\\' => out.push('\\'),
                | '"' => out.push('"'),
                | 'n' => out.push('\n'),
                | 'r' => out.push('\r'),
                | 't' => out.push('\t'),
                | other => {
                    return Err(format!("unsupported escape sequence \\{}", other));
                }
            }
        }

        Ok(out)
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
    /// Uses [`Self::view_collapsed`] to determine which blocks are folded.
    /// Returns `None` when `current` is the last visible block.
    pub fn next_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId> {
        // If current has visible children, descend into the first child.
        if !self.view_collapsed.contains_key(*current) {
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
    /// Uses [`Self::view_collapsed`] to determine which blocks are folded.
    /// Returns `None` when `current` is the first visible block.
    pub fn prev_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId> {
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
            if self.view_collapsed.contains_key(target) {
                return Some(target);
            }
            let children = self.children(&target);
            if children.is_empty() {
                return Some(target);
            }
            if let Some(&last) = children.last() {
                target = last;
            } else {
                return Some(target);
            }
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

    /// Construct a minimal one-root workspace for startup recovery mode.
    ///
    /// Used when persisted data cannot be loaded safely. This intentionally
    /// avoids the sample default document so the UI clearly indicates recovery
    /// state instead of looking like a normal first-run dataset.
    pub(crate) fn recovery_store() -> Self {
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();

        let root_id = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(root_id, String::new());

        BlockStore::new(vec![root_id], nodes, points)
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
            && self.instruction_drafts.len() == other.instruction_drafts.len()
            && self
                .instruction_drafts
                .iter()
                .all(|(id, draft)| other.instruction_drafts.get(id) == Some(draft))
            && self.inquiry_drafts.len() == other.inquiry_drafts.len()
            && self
                .inquiry_drafts
                .iter()
                .all(|(id, draft)| other.inquiry_drafts.get(id) == Some(draft))
            && self.view_collapsed.len() == other.view_collapsed.len()
            && self.view_collapsed.iter().all(|(id, _)| other.view_collapsed.contains_key(id))
            && self.friend_blocks.len() == other.friend_blocks.len()
            && self
                .friend_blocks
                .iter()
                .all(|(id, blocks)| other.friend_blocks.get(id) == Some(blocks))
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

    #[test]
    fn insert_parent_wraps_non_root_block() {
        let (mut store, root, child_a, child_b) = simple_store();

        let inserted = store.insert_parent(&child_a, "new_parent".to_string()).unwrap();

        assert_eq!(store.point(&inserted), Some("new_parent".to_string()));
        let root_node = store.node(&root).unwrap();
        assert_eq!(root_node.children(), &[inserted, child_b]);
        let inserted_node = store.node(&inserted).unwrap();
        assert_eq!(inserted_node.children(), &[child_a]);
    }

    #[test]
    fn insert_parent_wraps_root_block() {
        let (mut store, root, _child_a, _child_b) = simple_store();

        let inserted = store.insert_parent(&root, "new_root_parent".to_string()).unwrap();

        assert_eq!(store.roots(), &[inserted]);
        let inserted_node = store.node(&inserted).unwrap();
        assert_eq!(inserted_node.children(), &[root]);
    }

    #[test]
    fn insert_parent_returns_none_for_unknown_block() {
        let (mut store, _, _, _) = simple_store();
        let unknown = BlockId::default();
        assert_eq!(store.insert_parent(&unknown, "x".to_string()), None);
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

    #[test]
    fn block_context_with_friend_blocks_skips_unknown_ids() {
        let (store, root, child_a, _) = simple_store();
        let unknown = BlockId::default();
        let context = store.block_context_for_id_with_friend_blocks(
            &root,
            &[
                FriendBlock { block_id: unknown, perspective: None },
                FriendBlock { block_id: child_a, perspective: Some("supporting lens".to_string()) },
            ],
        );
        let friend_blocks = context.friend_blocks();
        assert_eq!(friend_blocks.len(), 1);
        assert_eq!(friend_blocks[0].point(), "child_a");
        assert_eq!(friend_blocks[0].perspective(), Some("supporting lens"));
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
        store.reduction_drafts.insert(
            child_a,
            ReductionDraftRecord { reduction: "reduction".to_string(), redundant_children: vec![] },
        );
        store.set_instruction_draft(root, "instruction".to_string());
        store.set_inquiry_draft(child_a, "inquiry".to_string());

        let json = serde_json::to_string(&store).unwrap();
        let restored: BlockStore = serde_json::from_str(&json).unwrap();

        assert_eq!(store, restored);
        assert!(restored.expansion_draft(&root).is_some());
        assert!(restored.reduction_draft(&child_a).is_some());
        assert_eq!(
            restored.instruction_draft(&root).map(|draft| draft.instruction.as_str()),
            Some("instruction")
        );
        assert_eq!(
            restored.inquiry_draft(&child_a).map(|draft| draft.response.as_str()),
            Some("inquiry")
        );
    }

    #[test]
    fn remove_subtree_cleans_persisted_drafts() {
        let (mut store, _root, child_a, child_b) = simple_store();
        store.expansion_drafts.insert(
            child_a,
            ExpansionDraftRecord { rewrite: None, children: vec!["draft".to_string()] },
        );
        store.reduction_drafts.insert(
            child_b,
            ReductionDraftRecord { reduction: "draft".to_string(), redundant_children: vec![] },
        );
        store.set_instruction_draft(child_a, "instruction draft".to_string());
        store.set_inquiry_draft(child_b, "inquiry draft".to_string());

        store.remove_block_subtree(&child_a).unwrap();
        store.remove_block_subtree(&child_b).unwrap();

        assert!(store.expansion_draft(&child_a).is_none());
        assert!(store.reduction_draft(&child_b).is_none());
        assert!(store.instruction_draft(&child_a).is_none());
        assert!(store.inquiry_draft(&child_b).is_none());
    }

    #[test]
    fn backward_compat_missing_draft_fields_defaults_empty() {
        let (store, _, _, _) = simple_store();
        let mut value = serde_json::to_value(&store).unwrap();
        value.as_object_mut().unwrap().remove("expansion_drafts");
        value.as_object_mut().unwrap().remove("reduction_drafts");
        value.as_object_mut().unwrap().remove("instruction_drafts");
        value.as_object_mut().unwrap().remove("inquiry_drafts");

        let restored: BlockStore = serde_json::from_value(value).unwrap();
        assert_eq!(restored.expansion_drafts.len(), 0);
        assert_eq!(restored.reduction_drafts.len(), 0);
        assert_eq!(restored.instruction_drafts.len(), 0);
        assert_eq!(restored.inquiry_drafts.len(), 0);
    }

    #[test]
    fn backward_compat_mount_without_format_defaults_to_json() {
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path(std::path::PathBuf::from("legacy.json")));
        points.insert(mount_id, "legacy mount".to_string());
        let store = BlockStore::new(vec![mount_id], nodes, points);

        let mut value = serde_json::to_value(&store).unwrap();
        if let Some(nodes_obj) = value["nodes"].as_object_mut() {
            for node in nodes_obj.values_mut() {
                if node.get("path").is_some() {
                    node.as_object_mut().expect("mount node object").remove("format");
                }
            }
        } else if let Some(nodes_arr) = value["nodes"].as_array_mut() {
            for node in nodes_arr {
                if node.get("path").is_some() {
                    node.as_object_mut().expect("mount node object").remove("format");
                }
            }
        } else {
            panic!("unexpected nodes serialization shape");
        }

        let restored: BlockStore = serde_json::from_value(value).unwrap();
        assert_eq!(
            restored.node(&restored.roots()[0]).and_then(|node| node.mount_format()),
            Some(MountFormat::Json)
        );
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

    fn write_markdown_sub_store(
        dir: &std::path::Path, filename: &str,
    ) -> (std::path::PathBuf, BlockStore) {
        let sub = simple_store().0;
        let path = dir.join(filename);
        let markdown = BlockStore::render_markdown_mount_store(&sub);
        fs::write(&path, markdown).unwrap();
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
    fn expand_markdown_mount_loads_and_rekeys() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, sub) = write_markdown_sub_store(tmp.path(), "sub.md");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path_and_format(
            std::path::PathBuf::from("sub.md"),
            MountFormat::Markdown,
        ));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        assert_eq!(new_roots.len(), sub.roots().len());
        assert_eq!(store.point(&new_roots[0]), Some("root".to_string()));
    }

    #[test]
    fn expand_markdown_mount_clears_collapsed_state_for_mount_point() {
        let tmp = tempfile::tempdir().unwrap();
        write_markdown_sub_store(tmp.path(), "sub.md");

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path_and_format(
            std::path::PathBuf::from("sub.md"),
            MountFormat::Markdown,
        ));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);
        store.view_collapsed.insert(mount_id, true);

        let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
        assert!(!new_roots.is_empty());
        assert!(!store.is_collapsed(&mount_id));
        assert_eq!(store.children(&mount_id), new_roots.as_slice());
    }

    #[test]
    fn expand_markdown_mount_errors_on_invalid_text() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("sub.md"), "- \"missing preamble\"\n").unwrap();

        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let mount_id = nodes.insert(BlockNode::with_path_and_format(
            std::path::PathBuf::from("sub.md"),
            MountFormat::Markdown,
        ));
        points.insert(mount_id, String::new());
        let mut store = BlockStore::new(vec![mount_id], nodes, points);

        let result = store.expand_mount(&mount_id, tmp.path());
        assert!(matches!(result, Err(MountError::MarkdownParse { .. })));
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
    fn save_subtree_to_markdown_sets_mount_format_and_writes_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut store, root, _child_a, _child_b) = simple_store();
        let path = tmp.path().join("subtree.md");

        store.save_subtree_to_file(&root, &path, tmp.path()).unwrap();

        let mount_node = store.node(&root).unwrap();
        assert_eq!(mount_node.mount_path(), Some(std::path::Path::new("subtree.md")));
        assert_eq!(mount_node.mount_format(), Some(MountFormat::Markdown));

        let markdown = fs::read_to_string(&path).unwrap();
        assert!(markdown.starts_with("<!-- bb-mount format=markdown v1 -->\n"));
        assert!(markdown.contains("- \"child_a\"\n"));
        assert!(markdown.contains("- \"child_b\"\n"));
    }

    #[test]
    fn save_subtree_to_markdown_escapes_special_characters() {
        let tmp = tempfile::tempdir().unwrap();
        let mut nodes = SlotMap::with_key();
        let mut points = SecondaryMap::new();
        let child = nodes.insert(BlockNode::with_children(vec![]));
        points.insert(child, "line1\n\"quoted\"\\tail".to_string());
        let root = nodes.insert(BlockNode::with_children(vec![child]));
        points.insert(root, "root".to_string());
        let mut store = BlockStore::new(vec![root], nodes, points);

        let path = tmp.path().join("escaped.md");
        store.save_subtree_to_file(&root, &path, tmp.path()).unwrap();

        let markdown = fs::read_to_string(&path).unwrap();
        assert!(markdown.contains("- \"line1\\n\\\"quoted\\\"\\\\tail\"\n"));
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
        assert!(store.node(&mount_id).unwrap().mount_path().is_none());
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
            |(_, node)| matches!(node, BlockNode::Mount { path, .. } if path == std::path::Path::new("sub.json")),
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

    // -- view_collapsed persistence --

    #[test]
    fn serde_round_trip_preserves_view_collapsed() {
        let (mut store, _root, child_a, _child_b) = simple_store();
        store.view_collapsed.insert(child_a, true);

        let json = serde_json::to_string(&store).unwrap();
        let restored: BlockStore = serde_json::from_str(&json).unwrap();

        assert_eq!(store, restored);
        assert!(restored.view_collapsed.contains_key(child_a));
    }

    #[test]
    fn backward_compat_missing_view_collapsed_defaults_empty() {
        let (store, _, _, _) = simple_store();
        let mut value = serde_json::to_value(&store).unwrap();
        value.as_object_mut().unwrap().remove("view_collapsed");

        let restored: BlockStore = serde_json::from_value(value).unwrap();
        assert_eq!(restored.view_collapsed.len(), 0);
    }

    #[test]
    fn remove_subtree_cleans_view_collapsed() {
        let (mut store, _root, child_a, _child_b) = simple_store();
        store.view_collapsed.insert(child_a, true);

        store.remove_block_subtree(&child_a).unwrap();
        assert!(!store.view_collapsed.contains_key(child_a));
    }

    #[test]
    fn block_context_with_friend_blocks_preserves_order_and_perspective() {
        let (store, root, child_a, child_b) = simple_store();
        let context = store.block_context_for_id_with_friend_blocks(
            &root,
            &[
                FriendBlock { block_id: child_b, perspective: Some("contrast".to_string()) },
                FriendBlock { block_id: child_a, perspective: None },
            ],
        );
        let friend_blocks = context.friend_blocks();
        assert_eq!(friend_blocks.len(), 2);
        assert_eq!(friend_blocks[0].point(), "child_b");
        assert_eq!(friend_blocks[0].perspective(), Some("contrast"));
        assert_eq!(friend_blocks[1].point(), "child_a");
        assert_eq!(friend_blocks[1].perspective(), None);
    }

    #[test]
    fn block_context_uses_persisted_friend_blocks_for_target() {
        let (mut store, root, child_a, child_b) = simple_store();
        store.set_friend_blocks_for(
            &root,
            vec![
                FriendBlock {
                    block_id: child_a,
                    perspective: Some("historical precedent".to_string()),
                },
                FriendBlock { block_id: child_b, perspective: None },
            ],
        );
        let context = store.block_context_for_id(&root);
        let friend_blocks = context.friend_blocks();
        assert_eq!(friend_blocks.len(), 2);
        assert_eq!(friend_blocks[0].point(), "child_a");
        assert_eq!(friend_blocks[0].perspective(), Some("historical precedent"));
        assert_eq!(friend_blocks[1].point(), "child_b");
        assert_eq!(friend_blocks[1].perspective(), None);
    }

    #[test]
    fn serde_round_trip_preserves_friend_blocks() {
        let (mut store, root, child_a, child_b) = simple_store();
        store.set_friend_blocks_for(
            &root,
            vec![
                FriendBlock { block_id: child_a, perspective: None },
                FriendBlock { block_id: child_b, perspective: Some("counter-example".to_string()) },
            ],
        );

        let json = serde_json::to_string(&store).unwrap();
        let restored: BlockStore = serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.friend_blocks_for(&root),
            &[
                FriendBlock { block_id: child_a, perspective: None },
                FriendBlock { block_id: child_b, perspective: Some("counter-example".to_string()) },
            ]
        );
    }

    #[test]
    fn backward_compat_missing_friend_blocks_defaults_empty() {
        let (store, _, _, _) = simple_store();
        let mut value = serde_json::to_value(&store).unwrap();
        value.as_object_mut().unwrap().remove("friend_blocks");

        let restored: BlockStore = serde_json::from_value(value).unwrap();
        assert_eq!(restored.friend_blocks.len(), 0);
    }

    #[test]
    fn remove_subtree_cleans_friend_blocks_keys_and_values() {
        let (mut store, root, child_a, child_b) = simple_store();
        store.set_friend_blocks_for(
            &root,
            vec![
                FriendBlock { block_id: child_a, perspective: None },
                FriendBlock { block_id: child_b, perspective: None },
            ],
        );
        store.set_friend_blocks_for(
            &child_a,
            vec![FriendBlock { block_id: root, perspective: Some("parent framing".to_string()) }],
        );

        store.remove_block_subtree(&child_a).unwrap();

        assert_eq!(
            store.friend_blocks_for(&root),
            &[FriendBlock { block_id: child_b, perspective: None }]
        );
        assert!(store.friend_blocks_for(&child_a).is_empty());
    }
}
