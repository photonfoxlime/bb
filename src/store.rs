//! Block store: the core document data model.
//!
//! A document is a forest of blocks. Each block has a UUID identity, a text
//! point, and ordered children. Persistence and mount invariants are documented
//! on the owning save/expand/collapse functions below.
//!
//! # Module layout
//!
//! Start with this file (`mod.rs`) for [`BlockStore`] and its fundamental
//! accessors. Core identity and structure types ([`BlockId`], [`BlockNode`],
//! etc.) live in [`block`]. Then read the submodules in dependency order:
//!
//! 1. [`block`] -- [`BlockId`], [`BlockNode`], [`FriendBlock`], [`Direction`],
//!    [`BlockPanelBarState`]. Structural skeleton of the block tree.
//! 2. [`drafts`] -- per-block draft records (amplify, distill, instruct,
//!    probe) and friend-block relations. These are the "optional metadata"
//!    that ride along with each block.
//! 3. [`tree`] -- structural mutations (append child/sibling, insert parent,
//!    duplicate, remove) and tree-traversal helpers (subtree collection, lineage).
//! 4. [`navigate`] -- DFS navigation respecting collapsed state, plus LLM
//!    context builders that assemble lineage + children + friends.
//! 5. [`mount`] -- mount data structures (table, entries, origins, errors)
//!    and operations: expand/collapse external files, save subtrees to disk,
//!    projected-store construction, and re-keying.
//! 6. [`markdown`] -- Markdown Mount v1 render/parse for the `.md` mount format.
//! 7. [`persist`] -- load/save the main store file; snapshot logic for
//!    excluding mounted blocks from the main-file serialization.
//!
//! # Adding per-block persistent data
//!
//! The store uses [`FxHashMap<BlockId, T>`] for optional per-block
//! metadata that must survive save/load cycles. Two categories exist:
//!
//! 1. Per-block data (user-authored content): `amplification_drafts`,
//!    `distillation_drafts`, `view_collapsed`, `friend_blocks`, `instruction_drafts`.
//!
//! 2. Per-block UI state (ephemeral but worth persisting): `block_panel_state`.
//!    This is not user-authored content but persists because it's useful to
//!    remember which panel was open for each block.
//!
//! Checklist for a new field:
//!
//! 1. Declare the field on [`BlockStore`] with `#[serde(default)]`.
//!    Use `bool` rather than `()` as the value type for set-like maps;
//!    `FxHashMap<_, ()>` is valid but less explicit than `bool` for set-like state.
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
//! 6. Clean up on removal via [`BlockStore::remove_block_metadata`], the
//!    single helper that removes an id from every per-block map. All tree
//!    and mount cleanup sites call this helper instead of inlining the
//!    removal loop.
//! 7. Update [`PartialEq`] by adding a length + element comparison clause.
//! 8. Tests, at minimum: serde round-trip, backward-compat (missing key
//!    defaults to empty), and cleanup-on-removal.

mod block;
mod drafts;
mod markdown;
mod mount;
mod navigate;
mod persist;
mod point;
mod tree;

pub use block::{BlockId, BlockNode, BlockPanelBarState, Direction, FriendBlock, MountProjection};
pub use drafts::{
    AmplificationDraftRecord, AtomizationDraftRecord, DistillationDraftRecord,
    InstructionDraftRecord, ProbeDraftRecord,
};
pub use mount::MountFormat;
pub use persist::StoreLoadError;
pub use point::{LinkKind, PointContent, PointLink};

use mount::MountTable;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// Forest of blocks: root ids, an archive list, a structural map, and a content map.
///
/// Invariant: every id in `roots`, `archive`, and in any node's `children` must exist as
/// a key in `nodes` **and** in `points`. The store always has at least one root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockStore {
    /// Persisted hint for empty mount points.
    ///
    /// When a mount point is not empty, this stores the first child's text content.
    /// When the mount is later expanded to an empty file, this hint is used to
    /// provide initial content.
    #[serde(default)]
    pub hint: Option<String>,
    /// The ordered root block ids.
    pub roots: Vec<BlockId>,
    /// The ordered archived block ids (roots of archived subtrees).
    ///
    /// Each id here is the topmost block of a subtree that has been removed
    /// from the main document tree. The block and its entire subtree remain
    /// in `nodes` and `points`; only the detached root is tracked here.
    #[serde(default)]
    pub archive: Vec<BlockId>,
    /// The structural map of blocks, keyed by [`BlockId`].
    pub nodes: FxHashMap<BlockId, BlockNode>,
    /// Typed content for each block, keyed by [`BlockId`].
    ///
    /// Each entry holds a [`PointContent`] with editable text and zero or more
    /// [`PointLink`]s. Backward-compatible serde handles legacy bare strings
    /// and old link objects.
    pub points: FxHashMap<BlockId, PointContent>,
    /// Runtime-only mount tracking. Not serialized; reconstructed by
    /// re-expanding [`BlockNode::Mount`] nodes after deserialization.
    #[serde(skip)]
    pub mount_table: MountTable,
    /// Cached child-to-parent mapping for O(1) parent lookups.
    ///
    /// Rebuilt after every structural mutation via [`Self::rebuild_parent_index`].
    /// Not serialized; derived from `roots` and `nodes`.
    #[serde(skip)]
    parent_index: FxHashMap<BlockId, BlockId>,
    /// Persisted per-block amplification drafts (rewrite + suggested children).
    ///
    /// Invariant: keys should reference existing blocks in [`Self::nodes`].
    /// Sparse by design: only blocks with pending amplification drafts are stored.
    #[serde(default, rename = "expansion_drafts")]
    pub amplification_drafts: FxHashMap<BlockId, AmplificationDraftRecord>,
    /// Persisted per-block atomization drafts.
    #[serde(default)]
    pub atomization_drafts: FxHashMap<BlockId, AtomizationDraftRecord>,
    /// Persisted per-block distillation drafts.
    ///
    /// Invariant: keys should reference existing blocks in [`Self::nodes`].
    /// Sparse by design: only blocks with pending distillation drafts are stored.
    #[serde(default, rename = "reduction_drafts")]
    pub distillation_drafts: FxHashMap<BlockId, DistillationDraftRecord>,
    /// Persisted per-block instruction drafts.
    #[serde(default)]
    pub instruction_drafts: FxHashMap<BlockId, InstructionDraftRecord>,
    /// Persisted per-block probe drafts.
    #[serde(default, rename = "inquiry_drafts")]
    pub probe_drafts: FxHashMap<BlockId, ProbeDraftRecord>,
    /// Persisted per-block fold (collapse) state.
    ///
    /// Presence of a key means the block's children are hidden in the UI.
    /// Stored in the authoritative graph so fold state survives reloads,
    /// participates in undo/redo snapshots, and follows save/load id remapping.
    #[serde(default)]
    pub view_collapsed: FxHashMap<BlockId, bool>,
    /// Per-block friend (linked) blocks used for cross-referencing.
    #[serde(default)]
    pub friend_blocks: FxHashMap<BlockId, Vec<FriendBlock>>,
    /// Persisted per-block block panel bar state (which panel is open).
    ///
    /// Stores whether the References or Instruction panel is open for each block.
    /// This survives reloads so the UI remembers which panel was last open.
    #[serde(default)]
    pub block_panel_state: FxHashMap<BlockId, BlockPanelBarState>,
}

impl BlockStore {
    /// Construct a store from pre-built roots, nodes, and plain-text points.
    ///
    /// Accepts `FxHashMap<BlockId, String>` for backward compatibility with
    /// existing call sites and tests. Strings are wrapped in
    /// [`PointContent`] internally.
    ///
    /// # Requires
    /// - Every id in `roots` must exist as a key in both `nodes` and `points`.
    /// - Every id in each node's `children` must exist as a key in both `nodes` and `points`.
    ///
    /// # Ensures
    /// - The store has at least one root.
    pub fn new(
        roots: Vec<BlockId>, nodes: FxHashMap<BlockId, BlockNode>,
        points: FxHashMap<BlockId, String>,
    ) -> Self {
        let typed_points = Self::convert_string_points(&nodes, points);
        Self::new_with_drafts(BlockStoreBuilder::new(roots, nodes, typed_points))
    }

    /// Internal constructor accepting fully-typed [`PointContent`] points.
    ///
    /// Prefer [`BlockStoreBuilder`] for constructing stores with optional draft
    /// and metadata fields — it makes each field's role explicit and lets
    /// omitted fields default to empty.
    pub(crate) fn new_with_drafts(builder: BlockStoreBuilder) -> Self {
        let mut store = Self {
            roots: builder.roots,
            archive: builder.archive,
            nodes: builder.nodes,
            points: builder.points,
            mount_table: MountTable::new(),
            parent_index: FxHashMap::default(),
            amplification_drafts: builder.amplification_drafts,
            atomization_drafts: builder.atomization_drafts,
            distillation_drafts: builder.distillation_drafts,
            instruction_drafts: builder.instruction_drafts,
            probe_drafts: builder.probe_drafts,
            view_collapsed: builder.view_collapsed,
            friend_blocks: builder.friend_blocks,
            block_panel_state: builder.block_panel_state,
            hint: builder.hint,
        };
        store.rebuild_parent_index();
        store
    }

    /// Allocate a fresh block id that is not present in `nodes`.
    ///
    /// Note: UUID v7 collisions are negligible in practice; the loop keeps the
    /// uniqueness invariant explicit and testable.
    fn fresh_block_id(nodes: &FxHashMap<BlockId, BlockNode>) -> BlockId {
        loop {
            let id = BlockId::new_v7();
            if !nodes.contains_key(&id) {
                return id;
            }
        }
    }

    /// Insert one structural node and return its newly allocated block id.
    pub(crate) fn insert_node(
        nodes: &mut FxHashMap<BlockId, BlockNode>, node: BlockNode,
    ) -> BlockId {
        let id = Self::fresh_block_id(nodes);
        nodes.insert(id, node);
        id
    }

    /// Remove all per-block metadata for `id` from every map in the store.
    ///
    /// This is the single site that must be updated when a new per-block
    /// field is added to [`BlockStore`]. Covers: `nodes`, `points`, all
    /// draft maps, `view_collapsed`, `friend_blocks`, `block_panel_state`,
    /// and the mount-table origin index.
    ///
    /// Note: Does **not** call [`Self::remove_friend_block_references`];
    /// callers that remove multiple blocks should batch that call separately.
    pub(crate) fn remove_block_metadata(&mut self, id: &BlockId) {
        self.nodes.remove(id);
        self.points.remove(id);
        self.amplification_drafts.remove(id);
        self.atomization_drafts.remove(id);
        self.distillation_drafts.remove(id);
        self.instruction_drafts.remove(id);
        self.probe_drafts.remove(id);
        self.view_collapsed.remove(id);
        self.friend_blocks.remove(id);
        self.block_panel_state.remove(id);
        self.mount_table.remove_origin(*id);
    }

    /// Convert a `FxHashMap<BlockId, String>` to `FxHashMap<BlockId, PointContent>`.
    ///
    /// Used by [`Self::new`] to bridge the public `String`-based API to the
    /// internal `PointContent` representation.
    fn convert_string_points(
        nodes: &FxHashMap<BlockId, BlockNode>, mut string_points: FxHashMap<BlockId, String>,
    ) -> FxHashMap<BlockId, PointContent> {
        let mut typed = FxHashMap::default();
        for (id, _) in nodes.iter() {
            if let Some(s) = string_points.remove(id) {
                typed.insert(*id, PointContent::from(s));
            }
        }
        typed
    }
    /// The ordered root block ids.
    ///
    /// # Ensures
    /// - The returned slice is non-empty.
    pub fn roots(&self) -> &[BlockId] {
        &self.roots
    }

    /// The ordered archived block ids (roots of archived subtrees).
    pub fn archive(&self) -> &[BlockId] {
        &self.archive
    }

    /// Look up a node by id.
    ///
    /// # Returns
    /// - `Some(&BlockNode)` if the id exists in the store.
    /// - `None` if the id is unknown.
    pub fn node(&self, id: &BlockId) -> Option<&BlockNode> {
        self.nodes.get(id)
    }

    /// Return the parent of a block, if any.
    ///
    /// Uses the cached `parent_index` for O(1) lookup.
    /// Root blocks have no parent and return `None`.
    pub fn parent(&self, child: &BlockId) -> Option<BlockId> {
        self.parent_index.get(child).copied()
    }

    /// Rebuild the child-to-parent index from scratch.
    ///
    /// Must be called after any operation that changes the parent-child
    /// topology (node insertions, removals, moves, mount expand/collapse).
    /// Cost: O(N) where N is the total number of nodes.
    pub(crate) fn rebuild_parent_index(&mut self) {
        self.parent_index.clear();
        for (parent_id, node) in &self.nodes {
            for child_id in node.children() {
                self.parent_index.insert(*child_id, *parent_id);
            }
        }
    }

    /// Return the children of a block, or an empty slice if unknown or a mount.
    ///
    /// # Returns
    /// - The children block ids if the block exists and is an inline children node.
    /// - An empty slice if the block is unknown or a mount node.
    pub fn children(&self, id: &BlockId) -> &[BlockId] {
        self.nodes.get(id).map(|n| n.children()).unwrap_or(&[])
    }

    /// Return the text of a block's point, or `None` if unknown.
    ///
    /// Most call sites use this; only UI rendering needs [`Self::point_content`].
    pub fn point(&self, id: &BlockId) -> Option<String> {
        self.points.get(id).map(|pc| pc.text.clone())
    }

    /// Return a reference to the full [`PointContent`] for UI rendering.
    ///
    /// Use this when the caller needs access to the block's links as well
    /// as its text (e.g., rendering the reference panel).
    pub fn point_content(&self, id: &BlockId) -> Option<&PointContent> {
        self.points.get(id)
    }

    /// Overwrite the text of an existing block's point.
    ///
    /// Links are preserved unchanged. No-op if the block does not exist.
    pub fn update_point(&mut self, id: &BlockId, value: String) {
        if !self.nodes.contains_key(id) {
            return;
        }
        match self.points.get_mut(id) {
            | Some(content) => content.text = value,
            | None => {
                self.points.insert(*id, PointContent::from(value));
            }
        }
    }

    /// Append a link to a block's point.
    ///
    /// No-op if the block does not exist.
    pub fn add_link_to_point(&mut self, id: &BlockId, link: PointLink) {
        if !self.nodes.contains_key(id) {
            return;
        }
        match self.points.get_mut(id) {
            | Some(content) => content.add_link(link),
            | None => {
                let mut content = PointContent::default();
                content.add_link(link);
                self.points.insert(*id, content);
            }
        }
    }

    /// Remove the link at `index` from a block's point.
    ///
    /// No-op if the block does not exist or `index` is out of bounds.
    pub fn remove_link_from_point(&mut self, id: &BlockId, index: usize) {
        if let Some(content) = self.points.get_mut(id) {
            content.remove_link(index);
        }
    }

    /// Set the optional perspective for one link attached to a block's point.
    ///
    /// Returns `true` when the target link exists and was updated.
    pub fn set_link_perspective(
        &mut self, id: &BlockId, index: usize, perspective: Option<String>,
    ) -> bool {
        let Some(content) = self.points.get_mut(id) else {
            return false;
        };
        let Some(link) = content.links.get_mut(index) else {
            return false;
        };
        link.perspective = perspective;
        true
    }

    /// Construct a minimal one-root workspace for startup recovery mode.
    ///
    /// Used when persisted data cannot be loaded safely. This intentionally
    /// avoids the sample default document so the UI clearly indicates recovery
    /// state instead of looking like a normal first-run dataset.
    pub(crate) fn recovery_store() -> Self {
        let mut nodes = FxHashMap::default();
        let mut points = FxHashMap::default();

        let root_id = Self::insert_node(&mut nodes, BlockNode::with_children(vec![]));
        points.insert(root_id, String::new());

        BlockStore::new(vec![root_id], nodes, points)
    }

    fn default_store() -> Self {
        let mut nodes = FxHashMap::default();
        let mut points = FxHashMap::default();

        let root_id = Self::insert_node(&mut nodes, BlockNode::with_children(vec![]));
        points.insert(
            root_id,
            "Tree of Thoughts: A Notebook for Designers and Developers".to_string(),
        );

        BlockStore::new(vec![root_id], nodes, points)
    }
}

/// Builder for constructing a [`BlockStore`] with optional draft and metadata
/// fields.
///
/// Required fields (`roots`, `nodes`, `points`) are provided via [`Self::new`].
/// Optional fields default to empty and can be set via `with_*` chainable
/// methods. Finalize with [`Self::build`].
pub(crate) struct BlockStoreBuilder {
    roots: Vec<BlockId>,
    archive: Vec<BlockId>,
    nodes: FxHashMap<BlockId, BlockNode>,
    points: FxHashMap<BlockId, PointContent>,
    amplification_drafts: FxHashMap<BlockId, AmplificationDraftRecord>,
    atomization_drafts: FxHashMap<BlockId, AtomizationDraftRecord>,
    distillation_drafts: FxHashMap<BlockId, DistillationDraftRecord>,
    instruction_drafts: FxHashMap<BlockId, InstructionDraftRecord>,
    probe_drafts: FxHashMap<BlockId, ProbeDraftRecord>,
    view_collapsed: FxHashMap<BlockId, bool>,
    friend_blocks: FxHashMap<BlockId, Vec<FriendBlock>>,
    block_panel_state: FxHashMap<BlockId, BlockPanelBarState>,
    hint: Option<String>,
}

impl BlockStoreBuilder {
    /// Create a builder with the three required fields.
    pub fn new(
        roots: Vec<BlockId>, nodes: FxHashMap<BlockId, BlockNode>,
        points: FxHashMap<BlockId, PointContent>,
    ) -> Self {
        Self {
            roots,
            nodes,
            points,
            archive: Vec::new(),
            amplification_drafts: FxHashMap::default(),
            atomization_drafts: FxHashMap::default(),
            distillation_drafts: FxHashMap::default(),
            instruction_drafts: FxHashMap::default(),
            probe_drafts: FxHashMap::default(),
            view_collapsed: FxHashMap::default(),
            friend_blocks: FxHashMap::default(),
            block_panel_state: FxHashMap::default(),
            hint: None,
        }
    }

    /// Finalize the builder, producing a [`BlockStore`].
    pub fn build(self) -> BlockStore {
        BlockStore::new_with_drafts(self)
    }

    pub fn with_archive(mut self, archive: Vec<BlockId>) -> Self {
        self.archive = archive;
        self
    }

    pub fn with_amplification_drafts(
        mut self, drafts: FxHashMap<BlockId, AmplificationDraftRecord>,
    ) -> Self {
        self.amplification_drafts = drafts;
        self
    }

    pub fn with_atomization_drafts(
        mut self, drafts: FxHashMap<BlockId, AtomizationDraftRecord>,
    ) -> Self {
        self.atomization_drafts = drafts;
        self
    }

    pub fn with_distillation_drafts(
        mut self, drafts: FxHashMap<BlockId, DistillationDraftRecord>,
    ) -> Self {
        self.distillation_drafts = drafts;
        self
    }

    pub fn with_instruction_drafts(
        mut self, drafts: FxHashMap<BlockId, InstructionDraftRecord>,
    ) -> Self {
        self.instruction_drafts = drafts;
        self
    }

    pub fn with_probe_drafts(mut self, drafts: FxHashMap<BlockId, ProbeDraftRecord>) -> Self {
        self.probe_drafts = drafts;
        self
    }

    pub fn with_view_collapsed(mut self, view_collapsed: FxHashMap<BlockId, bool>) -> Self {
        self.view_collapsed = view_collapsed;
        self
    }

    pub fn with_friend_blocks(
        mut self, friend_blocks: FxHashMap<BlockId, Vec<FriendBlock>>,
    ) -> Self {
        self.friend_blocks = friend_blocks;
        self
    }

    pub fn with_block_panel_state(mut self, state: FxHashMap<BlockId, BlockPanelBarState>) -> Self {
        self.block_panel_state = state;
        self
    }

    pub fn with_hint(mut self, hint: Option<String>) -> Self {
        self.hint = hint;
        self
    }
}

impl Default for BlockStore {
    fn default() -> Self {
        Self::default_store()
    }
}

impl PartialEq for BlockStore {
    /// Compare two stores for equality.
    ///
    /// Note: `mount_table` and `hint` are excluded because they are transient
    /// runtime state, not persisted data.
    fn eq(&self, other: &Self) -> bool {
        self.roots == other.roots
            && self.archive == other.archive
            && self.nodes == other.nodes
            && self.points == other.points
            && self.amplification_drafts == other.amplification_drafts
            && self.atomization_drafts == other.atomization_drafts
            && self.distillation_drafts == other.distillation_drafts
            && self.instruction_drafts == other.instruction_drafts
            && self.probe_drafts == other.probe_drafts
            && self.view_collapsed == other.view_collapsed
            && self.friend_blocks == other.friend_blocks
            && self.block_panel_state == other.block_panel_state
    }
}

#[cfg(test)]
mod tests;
