//! Tree structure commands.

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::{BlockStore, Direction};
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
    /// Example: `bb tree add-child 1v1 "My new idea"`.
    AddChild(AddChildCommand),

    /// Add a sibling block after a given block.
    ///
    /// Creates a new block with the given text and inserts it immediately
    /// after the target block in its parent's child list (or in roots).
    /// Returns the new sibling block ID.
    /// Fails if `block_id` is not found.
    /// Example: `bb tree add-sibling 1v1 "Next sibling"`.
    AddSibling(AddSiblingCommand),

    /// Wrap a block with a new parent.
    ///
    /// Inserts a new parent block at the target block's current position,
    /// making the target the first child of the new parent.
    /// Returns the new parent block ID.
    /// Fails if `block_id` is not found.
    /// Example: `bb tree wrap 1v1 "New parent section"`.
    Wrap(WrapCommand),

    /// Duplicate a subtree.
    ///
    /// Deep-clones the source block and its entire subtree, inserting the
    /// copy immediately after the original.
    /// Returns the root ID of the cloned subtree.
    /// Fails if `block_id` is not found.
    /// Example: `bb tree duplicate 1v1`.
    Duplicate(DuplicateCommand),

    /// Delete a subtree.
    ///
    /// Removes the block and all its descendants. Cleans up all associated
    /// metadata: drafts, friend references, panel state, and mount origins.
    ///
    /// If the deletion empties the root list, a single empty root is created.
    /// Returns all removed block IDs.
    /// Fails if `block_id` is not found.
    /// Example: `bb tree delete 1v1`.
    Delete(DeleteCommand),

    /// Move a block relative to a target.
    ///
    /// Repositions the source block to be before, after, or under the target.
    /// The source block (and its subtree) retains its internal structure.
    /// Source and target must be different blocks, and source must not be an
    /// ancestor of target. `--under` requires a non-mount target.
    /// Example: `bb tree move 1v1 2v1 --after`.
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

// =============================================================================
// Execution
// =============================================================================

/// Execute a tree command.
pub fn execute(store: BlockStore, cmd: TreeCommands) -> (BlockStore, CliResult) {
    match cmd {
        | TreeCommands::AddChild(c) => execute_add_child(store, &c),
        | TreeCommands::AddSibling(c) => execute_add_sibling(store, &c),
        | TreeCommands::Wrap(c) => execute_wrap(store, &c),
        | TreeCommands::Duplicate(c) => execute_duplicate(store, &c),
        | TreeCommands::Delete(c) => execute_delete(store, &c),
        | TreeCommands::Move(c) => execute_move(store, &c),
    }
}

fn execute_add_child(mut store: BlockStore, cmd: &AddChildCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.parent_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown parent block ID".to_string())),
            | Some(parent_id) => match store.append_child(&parent_id, cmd.text.clone()) {
                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                | None => (
                    store,
                    CliResult::Error("Failed to add child (parent may be a mount)".to_string()),
                ),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError {
                    input,
                    error: "Unknown parent block ID".to_string(),
                }),
                | Some(parent_id) => match store.append_child(&parent_id, cmd.text.clone()) {
                    | Some(new_id) => outputs.push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Failed to add child (parent may be a mount)".to_string(),
                    }),
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("tree.add-child", outputs, errors)))
    }
}

fn execute_add_sibling(mut store: BlockStore, cmd: &AddSiblingCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.append_sibling(&block_id, cmd.text.clone()) {
                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                | None => (store, CliResult::Error("Failed to add sibling".to_string())),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.append_sibling(&block_id, cmd.text.clone()) {
                    | Some(new_id) => outputs.push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Failed to add sibling".to_string(),
                    }),
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("tree.add-sibling", outputs, errors)))
    }
}

fn execute_wrap(mut store: BlockStore, cmd: &WrapCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.insert_parent(&block_id, cmd.text.clone()) {
                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                | None => (store, CliResult::Error("Failed to wrap block".to_string())),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.insert_parent(&block_id, cmd.text.clone()) {
                    | Some(new_id) => outputs.push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Failed to wrap block".to_string(),
                    }),
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("tree.wrap", outputs, errors)))
    }
}

fn execute_duplicate(mut store: BlockStore, cmd: &DuplicateCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.duplicate_subtree_after(&block_id) {
                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                | None => (store, CliResult::Error("Failed to duplicate".to_string())),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.duplicate_subtree_after(&block_id) {
                    | Some(new_id) => outputs.push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Failed to duplicate".to_string(),
                    }),
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("tree.duplicate", outputs, errors)))
    }
}

fn execute_delete(mut store: BlockStore, cmd: &DeleteCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.remove_block_subtree(&block_id) {
                | Some(ids) => {
                    let ids_str: Vec<String> = ids.iter().map(|i| format!("{}", i)).collect();
                    (store, CliResult::Removed(ids_str))
                }
                | None => (store, CliResult::Error("Failed to delete".to_string())),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.remove_block_subtree(&block_id) {
                    | Some(ids) => outputs.push(BatchOutput::Removed {
                        input,
                        removed: ids.iter().map(|id| format!("{}", id)).collect(),
                    }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Failed to delete".to_string(),
                    }),
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("tree.delete", outputs, errors)))
    }
}

fn execute_move(mut store: BlockStore, cmd: &MoveCommand) -> (BlockStore, CliResult) {
    let pairs = match execute::expand_cli_pairs(&cmd.source_id, &cmd.target_id) {
        | Ok(pairs) => pairs,
        | Err(msg) => return (store, CliResult::Error(msg)),
    };

    let dir = if cmd.before {
        Direction::Before
    } else if cmd.after {
        Direction::After
    } else {
        Direction::Under
    };

    if pairs.len() == 1 {
        let source = execute::resolve_block_id(&store, &pairs[0].0);
        let target = execute::resolve_block_id(&store, &pairs[0].1);
        match (source, target) {
            | (Some(src), Some(tgt)) => match store.move_block(&src, &tgt, dir) {
                | Some(()) => (store, CliResult::Success),
                | None => (
                    store,
                    CliResult::Error("Move failed (check constraints)".to_string()),
                ),
            },
            | _ => (store, CliResult::Error("Unknown source or target block ID".to_string())),
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for (source_cli, target_cli) in pairs {
            let input = format!("{} -> {}", source_cli.0, target_cli.0);
            let source = execute::resolve_block_id(&store, &source_cli);
            let target = execute::resolve_block_id(&store, &target_cli);
            match (source, target) {
                | (Some(src), Some(tgt)) => match store.move_block(&src, &tgt, dir) {
                    | Some(()) => outputs.push(BatchOutput::Success { input }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Move failed (check constraints)".to_string(),
                    }),
                },
                | _ => errors.push(BatchError {
                    input,
                    error: "Unknown source or target block ID".to_string(),
                }),
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("tree.move", outputs, errors)))
    }
}
