//! Core block identity and structure types.
//!
//! This module defines [`BlockId`], [`BlockNode`], [`FriendBlock`], and related
//! types that form the structural skeleton of the block tree. Content (points)
//! and metadata (drafts, panel state) live in [`BlockStore`] and its submodules.

use super::mount::MountFormat;
use serde::{Deserialize, Serialize};
use slotmap::Key;

slotmap::new_key_type! {
    pub struct BlockId;
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ffi = self.data().as_ffi();
        let index = ffi & 0xFFFFFFFF;
        let generation = ffi >> 32;
        write!(f, "{}v{}", index, generation)
    }
}

/// Persisted friend relation from a source block to a target block.
///
/// Friend blocks are user-selected related context for a block: they are not
/// children but extra blocks whose text (and optional perspective) is included
/// when building LLM context for distill/amplify. The block that "has" the
/// friends is the *source* (key in `BlockStore::friend_blocks`); each
/// [`FriendBlock`] points to another block in the graph and an optional
/// framing string (perspective) for how the source should interpret that friend.
///
/// - [`Self::block_id`] points to the friend block in the main store graph.
/// - [`Self::perspective`] is optional source-authored framing text that describes how
///   the source block should interpret that friend block.
/// - [`Self::parent_lineage_telescope`] controls whether the friend block's parent lineage
///   is included in LLM context.
/// - [`Self::children_telescope`] controls whether the friend block's children
///   are included in LLM context.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FriendBlock {
    /// Target friend block id.
    pub block_id: BlockId,
    /// Optional source-authored framing for this friend relation.
    #[serde(default)]
    pub perspective: Option<String>,
    /// Whether to include the friend block's parent lineage in LLM context.
    #[serde(default)]
    pub parent_lineage_telescope: bool,
    /// Whether to include the friend block's children in LLM context.
    #[serde(default)]
    pub children_telescope: bool,
}

/// Specifies the direction for moving a block relative to a target block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Move the source block to immediately before the target.
    Before,
    /// Move the source block to immediately after the target.
    After,
    /// Move the source block to be the last child of the target.
    Under,
}

/// Persisted block panel bar state: which panel (if any) is open for a block.
///
/// This is stored per-block so each block remembers its own panel state
/// across app restarts. Unlike runtime UI state, this survives save/load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockPanelBarState {
    /// Friends panel - shows user-selected friend blocks for LLM context.
    Friends,
    /// Instruction panel - text editor for LLM instructions.
    Instruction,
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

/// Internal projection used during snapshot/extract to override mount paths.
#[derive(Debug, Clone)]
pub struct MountProjection {
    pub path: std::path::PathBuf,
    pub format: MountFormat,
}
