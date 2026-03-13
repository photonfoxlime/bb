//! Friend block commands.

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult, FriendInfo},
};
use crate::store::BlockStore;
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
    /// Example: `bb point friend add 1v1 2v1 --perspective "Related design"`.
    Add(AddFriendCommand),

    /// Remove a friend block.
    /// Example: `bb point friend remove 1v1 2v1`.
    Remove(RemoveFriendCommand),

    /// List friend blocks for a target.
    /// Example: `bb point friend list 1v1`.
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

// =============================================================================
// Execution
// =============================================================================

/// Execute a friend command.
pub fn execute(store: BlockStore, cmd: FriendCommands) -> (BlockStore, CliResult) {
    match cmd {
        | FriendCommands::Add(c) => execute_add(store, &c),
        | FriendCommands::Remove(c) => execute_remove(store, &c),
        | FriendCommands::List(c) => execute_list(store, &c),
    }
}

fn execute_add(mut store: BlockStore, cmd: &AddFriendCommand) -> (BlockStore, CliResult) {
    let pairs = match execute::expand_cli_pairs(&cmd.target_id, &cmd.friend_id) {
        | Ok(pairs) => pairs,
        | Err(msg) => return (store, CliResult::Error(msg)),
    };

    if pairs.len() == 1 {
        let target = execute::resolve_block_id(&store, &pairs[0].0);
        let friend = execute::resolve_block_id(&store, &pairs[0].1);
        match (target, friend) {
            | (Some(tid), Some(fid)) => {
                let mut friends = store.friend_blocks_for(&tid).to_vec();
                friends.push(crate::store::FriendBlock {
                    block_id: fid,
                    perspective: cmd.perspective.clone(),
                    parent_lineage_telescope: cmd.telescope_lineage,
                    children_telescope: cmd.telescope_children,
                });
                store.set_friend_blocks_for(&tid, friends);
                (store, CliResult::Success)
            }
            | _ => (store, CliResult::Error("Unknown block ID".to_string())),
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for (target_cli, friend_cli) in pairs {
            let input = format!("{} -> {}", target_cli.0, friend_cli.0);
            let target = execute::resolve_block_id(&store, &target_cli);
            let friend = execute::resolve_block_id(&store, &friend_cli);
            match (target, friend) {
                | (Some(tid), Some(fid)) => {
                    let mut friends = store.friend_blocks_for(&tid).to_vec();
                    friends.push(crate::store::FriendBlock {
                        block_id: fid,
                        perspective: cmd.perspective.clone(),
                        parent_lineage_telescope: cmd.telescope_lineage,
                        children_telescope: cmd.telescope_children,
                    });
                    store.set_friend_blocks_for(&tid, friends);
                    outputs.push(BatchOutput::Success { input });
                }
                | _ => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("friend.add", outputs, errors)))
    }
}

fn execute_remove(mut store: BlockStore, cmd: &RemoveFriendCommand) -> (BlockStore, CliResult) {
    let pairs = match execute::expand_cli_pairs(&cmd.target_id, &cmd.friend_id) {
        | Ok(pairs) => pairs,
        | Err(msg) => return (store, CliResult::Error(msg)),
    };

    if pairs.len() == 1 {
        let target = execute::resolve_block_id(&store, &pairs[0].0);
        let friend = execute::resolve_block_id(&store, &pairs[0].1);
        match (target, friend) {
            | (Some(tid), Some(fid)) => {
                let mut friends = store.friend_blocks_for(&tid).to_vec();
                friends.retain(|f| f.block_id != fid);
                store.set_friend_blocks_for(&tid, friends);
                (store, CliResult::Success)
            }
            | _ => (store, CliResult::Error("Unknown block ID".to_string())),
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for (target_cli, friend_cli) in pairs {
            let input = format!("{} -> {}", target_cli.0, friend_cli.0);
            let target = execute::resolve_block_id(&store, &target_cli);
            let friend = execute::resolve_block_id(&store, &friend_cli);
            match (target, friend) {
                | (Some(tid), Some(fid)) => {
                    let mut friends = store.friend_blocks_for(&tid).to_vec();
                    friends.retain(|f| f.block_id != fid);
                    store.set_friend_blocks_for(&tid, friends);
                    outputs.push(BatchOutput::Success { input });
                }
                | _ => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("friend.remove", outputs, errors)))
    }
}

fn execute_list(store: BlockStore, cmd: &ListFriendCommand) -> (BlockStore, CliResult) {
    let target = execute::resolve_block_id(&store, &cmd.target_id);
    match target {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(tid) => {
            let friends: Vec<FriendInfo> = store
                .friend_blocks_for(&tid)
                .iter()
                .map(|f| FriendInfo {
                    id: format!("{}", f.block_id),
                    perspective: f.perspective.clone(),
                    telescope_lineage: f.parent_lineage_telescope,
                    telescope_children: f.children_telescope,
                })
                .collect();
            (store, CliResult::FriendList(friends))
        }
    }
}
