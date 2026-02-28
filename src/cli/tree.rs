//! Tree structure commands.

use super::BlockId;
use clap::Parser;

/// Tree structure operations.
#[derive(Debug, Parser)]
pub enum TreeCommands {
    /// Add a child block under a parent.
    ///
    /// Creates a new block with the given text as its point and appends it
    /// as the last child of the specified parent.
    /// Returns the new block ID.
    /// Fails if the parent is missing or is a mount node.
    /// Example: `blooming-blockery block tree add-child 1v1 "My new idea"`.
    AddChild(AddChildCommand),

    /// Add a sibling block after a given block.
    ///
    /// Creates a new block with the given text and inserts it immediately
    /// after the target block in its parent's child list (or in roots).
    /// Returns the new sibling block ID.
    /// Fails if `block_id` is not found.
    /// Example: `blooming-blockery block tree add-sibling 1v1 "Next sibling"`.
    AddSibling(AddSiblingCommand),

    /// Wrap a block with a new parent.
    ///
    /// Inserts a new parent block at the target block's current position,
    /// making the target the first child of the new parent.
    /// Returns the new parent block ID.
    /// Fails if `block_id` is not found.
    /// Example: `blooming-blockery block tree wrap 1v1 "New parent section"`.
    Wrap(WrapCommand),

    /// Duplicate a subtree.
    ///
    /// Deep-clones the source block and its entire subtree, inserting the
    /// copy immediately after the original.
    /// Returns the root ID of the cloned subtree.
    /// Fails if `block_id` is not found.
    /// Example: `blooming-blockery block tree duplicate 1v1`.
    Duplicate(DuplicateCommand),

    /// Delete a subtree.
    ///
    /// Removes the block and all its descendants. Cleans up all associated
    /// metadata: drafts, friend references, panel state, and mount origins.
    ///
    /// If the deletion empties the root list, a single empty root is created.
    /// Returns all removed block IDs.
    /// Fails if `block_id` is not found.
    /// Example: `blooming-blockery block tree delete 1v1`.
    Delete(DeleteCommand),

    /// Move a block relative to a target.
    ///
    /// Repositions the source block to be before, after, or under the target.
    /// The source block (and its subtree) retains its internal structure.
    /// Source and target must be different blocks, and source must not be an
    /// ancestor of target. `--under` requires a non-mount target.
    /// Example: `blooming-blockery block tree move 1v1 2v1 --after`.
    Move(MoveCommand),
}

/// Add a child block under a parent.
#[derive(Debug, Parser)]
pub struct AddChildCommand {
    /// Parent block ID.
    ///
    /// Must be an existing block that is not a mount node.
    #[arg(value_name = "PARENT_ID")]
    pub parent_id: BlockId,

    /// Initial text content for the new child block.
    ///
    /// Can be any string, including empty string.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Add a sibling block after a given block.
#[derive(Debug, Parser)]
pub struct AddSiblingCommand {
    /// Block to add sibling after.
    ///
    /// Can be a root block or a child block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Initial text content for the new sibling.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Wrap a block with a new parent.
#[derive(Debug, Parser)]
pub struct WrapCommand {
    /// Block to wrap (becomes first child of new parent).
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Initial text content for the new parent.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Duplicate a subtree.
#[derive(Debug, Parser)]
pub struct DuplicateCommand {
    /// Block to duplicate (with all descendants).
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Delete a subtree.
#[derive(Debug, Parser)]
pub struct DeleteCommand {
    /// Block to delete (with all descendants).
    ///
    /// Deleting a block also removes all friend references TO that block
    /// from other blocks, and cleans up drafts/panel state.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Move a block relative to a target.
#[derive(Debug, Parser)]
pub struct MoveCommand {
    /// Block to move.
    ///
    /// The entire subtree moves with this block.
    #[arg(value_name = "SOURCE_ID")]
    pub source_id: BlockId,

    /// Target block for positioning.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,

    /// Move source to be immediately before target.
    ///
    /// Source becomes the previous sibling of target.
    #[arg(long, group = "direction")]
    pub before: bool,

    /// Move source to be immediately after target.
    ///
    /// Source becomes the next sibling of target.
    #[arg(long, group = "direction")]
    pub after: bool,

    /// Move source to be the last child of target.
    ///
    /// Target must not be a mount node.
    #[arg(long, group = "direction")]
    pub under: bool,
}
