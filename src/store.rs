//! Block store: the core document data model.
//!
//! A document is a forest of blocks. Each block has a slotmap identity, a text
//! point, and ordered children. Persistence and mount invariants are documented
//! on the owning save/expand/collapse functions below.
//!
//! # Module layout
//!
//! Start with this file (`mod.rs`) for the core types: [`BlockId`], [`BlockNode`],
//! and [`BlockStore`] with its fundamental accessors. Then read the submodules in
//! dependency order:
//!
//! 1. [`drafts`] -- per-block draft records (expand, reduce, instruct,
//!    inquire) and friend-block relations. These are the "optional metadata"
//!    that ride along with each block.
//! 2. [`tree`] -- structural mutations (append child/sibling, insert parent,
//!    duplicate, remove) and tree-traversal helpers (subtree collection, lineage).
//! 3. [`navigate`] -- DFS navigation respecting collapsed state, plus LLM
//!    context builders that assemble lineage + children + friends.
//! 4. [`mount`] -- mount data structures (table, entries, origins, errors)
//!    and operations: expand/collapse external files, save subtrees to disk,
//!    projected-store construction, and re-keying.
//! 5. [`markdown`] -- Markdown Mount v1 render/parse for the `.md` mount format.
//! 6. [`persist`] -- load/save the main store file; snapshot logic for
//!    excluding mounted blocks from the main-file serialization.
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

mod drafts;
mod markdown;
mod mount;
mod navigate;
mod persist;
mod tree;

pub use drafts::{
    ExpansionDraftRecord, InquiryDraftRecord, InstructionDraftRecord,
    ReductionDraftRecord,
};
pub use mount::MountFormat;
pub use persist::StoreLoadError;

use mount::MountTable;
use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap, SparseSecondaryMap};

/// Internal projection used during snapshot/extract to override mount paths.
#[derive(Debug, Clone)]
pub(crate) struct MountProjection {
    pub(crate) path: std::path::PathBuf,
    pub(crate) format: MountFormat,
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

slotmap::new_key_type! {
    pub struct BlockId;
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
    pub(crate) roots: Vec<BlockId>,
    pub(crate) nodes: SlotMap<BlockId, BlockNode>,
    /// Text content for each block, keyed by the same `BlockId`.
    pub(crate) points: SecondaryMap<BlockId, String>,
    /// Runtime-only mount tracking. Not serialized; reconstructed by
    /// re-expanding `BlockNode::Mount` nodes after deserialization.
    #[serde(skip)]
    pub(crate) mount_table: MountTable,
    /// Persisted per-block expansion drafts (rewrite + suggested children).
    ///
    /// Invariant: keys should reference existing blocks in `nodes`.
    /// Sparse by design: only blocks with pending expansion drafts are stored.
    #[serde(default)]
    pub(crate) expansion_drafts: SparseSecondaryMap<BlockId, ExpansionDraftRecord>,
    /// Persisted per-block reduction drafts.
    ///
    /// Invariant: keys should reference existing blocks in `nodes`.
    /// Sparse by design: only blocks with pending reduction drafts are stored.
    #[serde(default)]
    pub(crate) reduction_drafts: SparseSecondaryMap<BlockId, ReductionDraftRecord>,
    /// Persisted per-block instruction drafts.
    #[serde(default)]
    pub(crate) instruction_drafts: SparseSecondaryMap<BlockId, InstructionDraftRecord>,
    /// Persisted per-block inquiry drafts.
    #[serde(default)]
    pub(crate) inquiry_drafts: SparseSecondaryMap<BlockId, InquiryDraftRecord>,
    /// Persisted per-block fold (collapse) state.
    ///
    /// Presence of a key means the block's children are hidden in the UI.
    /// Stored in the authoritative graph so fold state survives reloads,
    /// participates in undo/redo snapshots, and follows save/load id remapping.
    #[serde(default)]
    pub(crate) view_collapsed: SparseSecondaryMap<BlockId, bool>,
    #[serde(default)]
    pub(crate) friend_blocks: SparseSecondaryMap<BlockId, Vec<FriendBlock>>,
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

    pub(crate) fn new_with_drafts(
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
mod tests;
