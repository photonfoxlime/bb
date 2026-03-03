//! Friend block commands.

use super::BlockId;
use clap::Parser;

/// Friend block operations.
#[derive(Debug, Parser)]
pub enum FriendCommands {
    /// Add a friend block.
    ///
    /// Friend blocks are extra context blocks included in LLM requests for
    /// the target block. They are not children but related blocks with
    /// optional perspective framing.
    /// Fails if either ID is unknown or if `target_id` equals `friend_id`.
    /// Example: `bb friend add 1v1 2v1 --perspective "Related design"`.
    Add(AddFriendCommand),

    /// Remove a friend block.
    /// Example: `bb friend remove 1v1 2v1`.
    Remove(RemoveFriendCommand),

    /// List friend blocks for a target.
    /// Example: `bb friend list 1v1 --output json`.
    List(ListFriendCommand),
}

/// Add a friend block.
#[derive(Debug, Parser)]
pub struct AddFriendCommand {
    /// Target block that will have the friend.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,

    /// Block to add as a friend.
    #[arg(value_name = "FRIEND_ID")]
    pub friend_id: BlockId,

    /// Optional framing text for interpreting this friend.
    ///
    /// Describes how the target should view this friend block.
    #[arg(long, value_name = "TEXT")]
    pub perspective: Option<String>,

    /// Include friend's parent lineage in LLM context.
    ///
    /// When enabled, the friend's full ancestry (root to parent) is included.
    #[arg(long)]
    pub telescope_lineage: bool,

    /// Include friend's children in LLM context.
    ///
    /// When enabled, the friend's direct children text is included.
    #[arg(long)]
    pub telescope_children: bool,
}

/// Remove a friend block.
#[derive(Debug, Parser)]
pub struct RemoveFriendCommand {
    /// Target block.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,

    /// Friend to remove.
    #[arg(value_name = "FRIEND_ID")]
    pub friend_id: BlockId,
}

/// List friend blocks.
#[derive(Debug, Parser)]
pub struct ListFriendCommand {
    /// Target block to query.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,
}
