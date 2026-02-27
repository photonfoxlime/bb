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
    ///
    /// # Arguments
    ///
    /// - `parent_id`: ID of an existing block (must not be a mount node)
    /// - `text`: Initial text content for the new block
    ///
    /// # Returns
    ///
    /// The newly created block ID.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Parent ID not found.
    /// - `InvalidOperation`: Parent is a mount node (cannot have children).
    ///
    /// # Example
    /// ```bash
    /// block tree add-child 1v1 "My new idea"
    /// # Returns: 2v1
    /// ```
    AddChild(AddChildCommand),

    /// Add a sibling block after a given block.
    ///
    /// Creates a new block with the given text and inserts it immediately
    /// after the target block in its parent's child list (or in roots).
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block
    /// - `text`: Initial text content for the new sibling
    ///
    /// # Returns
    ///
    /// The newly created sibling block ID.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    ///
    /// # Example
    /// ```bash
    /// block tree add-sibling 1v1 "Next sibling"
    /// # Returns: 3v1
    /// ```
    AddSibling(AddSiblingCommand),

    /// Wrap a block with a new parent.
    ///
    /// Inserts a new parent block at the target block's current position,
    /// making the target the first child of the new parent.
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block (the child to wrap)
    /// - `text`: Initial text content for the new parent
    ///
    /// # Returns
    ///
    /// The newly created parent block ID.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    ///
    /// # Example
    /// ```bash
    /// block tree wrap 1v1 "New parent section"
    /// # Returns: 4v1
    /// # Before: root -> [1v1]
    /// # After:  root -> [4v1] -> [1v1]
    /// ```
    Wrap(WrapCommand),

    /// Duplicate a subtree.
    ///
    /// Deep-clones the source block and its entire subtree, inserting the
    /// copy immediately after the original.
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block to duplicate
    ///
    /// # Returns
    ///
    /// The root ID of the cloned subtree.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    ///
    /// # Example
    /// ```bash
    /// block tree duplicate 1v1
    /// # Returns: 5v1
    /// ```
    Duplicate(DuplicateCommand),

    /// Delete a subtree.
    ///
    /// Removes the block and all its descendants. Cleans up all associated
    /// metadata: drafts, friend references, panel state, and mount origins.
    ///
    /// If the deletion empties the root list, a single empty root is created.
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block to delete
    ///
    /// # Returns
    ///
    /// List of all removed block IDs (including descendants).
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    /// - `InvalidOperation`: Attempting to delete the last root (allowed but
    ///   results in a new empty root).
    ///
    /// # Example
    /// ```bash
    /// block tree delete 1v1
    /// # Returns: {"removed":["1v1","2v1","7v1"]}
    /// ```
    Delete(DeleteCommand),

    /// Move a block relative to a target.
    ///
    /// Repositions the source block to be before, after, or under the target.
    /// The source block (and its subtree) retains its internal structure.
    ///
    /// # Arguments
    ///
    /// - `source_id`: Block to move
    /// - `target_id`: Reference block for positioning
    /// - `--before`, `--after`, `--under`: Positioning direction
    ///
    /// # Constraints
    ///
    /// - Source and target must be different blocks.
    /// - Source must not be an ancestor of target (would create cycle).
    /// - `--under` requires target is not a mount node.
    ///
    /// # Returns
    ///
    /// Success indicator (or error with reason).
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Either ID not found.
    /// - `InvalidOperation`: Source is ancestor of target (cycle).
    /// - `InvalidOperation`: `--under` on mount node.
    ///
    /// # Example
    /// ```bash
    /// block tree move 1v1 2v1 --before
    /// block tree move 1v1 2v1 --after
    /// block tree move 1v1 2v1 --under
    /// ```
    Move(MoveCommand),
}

/// Add a child block under a parent.
#[derive(Debug, Parser)]
pub struct AddChildCommand {
    /// Parent block ID.
    ///
    /// Must be an existing block that is not a mount node.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Parent not found.
    /// - `InvalidOperation`: Parent is a mount.
    #[arg(value_name = "PARENT_ID")]
    pub parent_id: BlockId,

    /// Initial text content for the new child block.
    ///
    /// Can be any string, including empty string.
    ///
    /// # Example
    /// ```bash
    /// block tree add-child 1v1 "My new idea"
    /// block tree add-child 1v1 ""  # Empty text
    /// ```
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
    /// # Safety Note
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
    ///
    /// # Example
    /// ```bash
    /// block tree move 1v1 2v1 --before
    /// # Before: [..., 1v1, ..., 2v1, ...]
    /// # After:  [..., 2v1, 1v1, ...]
    /// ```
    #[arg(long, group = "direction")]
    pub before: bool,

    /// Move source to be immediately after target.
    ///
    /// Source becomes the next sibling of target.
    ///
    /// # Example
    /// ```bash
    /// block tree move 1v1 2v1 --after
    /// # Before: [..., 2v1, ..., 1v1, ...]
    /// # After:  [..., 2v1, 1v1, ...]
    /// ```
    #[arg(long, group = "direction")]
    pub after: bool,

    /// Move source to be the last child of target.
    ///
    /// Target must not be a mount node.
    ///
    /// # Example
    /// ```bash
    /// block tree move 1v1 2v1 --under
    /// # Before: 2v1 -> []
    /// # After:  2v1 -> [1v1]
    /// ```
    #[arg(long, group = "direction")]
    pub under: bool,
}
