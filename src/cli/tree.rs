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
    /// block tree add-child 0x1a2b3c "My new idea"
    /// # Returns: 0x9z8y7x
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
    /// block tree add-sibling 0x1a2b3c "Next sibling"
    /// # Returns: 0x7w6v5u
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
    /// block tree wrap 0x1a2b3c "New parent section"
    /// # Returns: 0x4t3s2r
    /// # Before: root -> [0x1a2b3c]
    /// # After:  root -> [0x4t3s2r] -> [0x1a2b3c]
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
    /// block tree duplicate 0x1a2b3c
    /// # Returns: 0x1q2w3e
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
    /// block tree delete 0x1a2b3c
    /// # Returns: {"removed":["0x1a2b3c","0x4d5e6f","0x7g8h9i"]}
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
    /// block tree move 0xsource 0xtarget --before
    /// block tree move 0xsource 0xtarget --after
    /// block tree move 0xsource 0xtarget --under
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
    /// block tree add-child 0x123 "My new idea"
    /// block tree add-child 0x123 ""  # Empty text
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
    /// block tree move 0xsrc 0xtgt --before
    /// # Before: [..., 0xsrc, ..., 0xtgt, ...]
    /// # After:  [..., 0xtgt, 0xsrc, ...]
    /// ```
    #[arg(long, group = "direction")]
    pub before: bool,

    /// Move source to be immediately after target.
    ///
    /// Source becomes the next sibling of target.
    ///
    /// # Example
    /// ```bash
    /// block tree move 0xsrc 0xtgt --after
    /// # Before: [..., 0xtgt, ..., 0xsrc, ...]
    /// # After:  [..., 0xtgt, 0xsrc, ...]
    /// ```
    #[arg(long, group = "direction")]
    pub after: bool,

    /// Move source to be the last child of target.
    ///
    /// Target must not be a mount node.
    ///
    /// # Example
    /// ```bash
    /// block tree move 0xsrc 0xtgt --under
    /// # Before: 0xtgt -> []
    /// # After:  0xtgt -> [0xsrc]
    /// ```
    #[arg(long, group = "direction")]
    pub under: bool,
}
