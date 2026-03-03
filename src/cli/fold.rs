//! Fold (collapse) state commands.

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::BlockStore;
use clap::Parser;

/// Fold (collapse) state operations.
#[derive(Debug, Parser)]
pub enum FoldCommands {
    /// Toggle the fold state of a block.
    ///
    /// If collapsed, expands to show children. If expanded, collapses to hide.
    /// Returns `true` when the block is collapsed after the operation.
    /// Example: `bb fold toggle 1v1`.
    Toggle(ToggleFoldCommand),

    /// Get the fold state of a block.
    /// Returns `true` if collapsed and `false` if expanded.
    /// Example: `bb fold status 1v1`.
    Status(StatusFoldCommand),
}

/// Toggle fold state.
#[derive(Debug, Parser)]
pub struct ToggleFoldCommand {
    /// Block to toggle.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Get fold status.
#[derive(Debug, Parser)]
pub struct StatusFoldCommand {
    /// Block to query.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

// =============================================================================
// Execution
// =============================================================================

/// Execute a fold command.
pub fn execute(store: BlockStore, cmd: FoldCommands) -> (BlockStore, CliResult) {
    match cmd {
        | FoldCommands::Toggle(c) => execute_toggle(store, &c),
        | FoldCommands::Status(c) => execute_status(store, &c),
    }
}

fn execute_toggle(mut store: BlockStore, cmd: &ToggleFoldCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let collapsed = store.toggle_collapsed(&block_id);
                (store, CliResult::Collapsed(collapsed))
            }
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    let collapsed = store.toggle_collapsed(&block_id);
                    outputs.push(BatchOutput::Collapsed { input, collapsed });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("fold.toggle", outputs, errors)))
    }
}

fn execute_status(store: BlockStore, cmd: &StatusFoldCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let collapsed = store.is_collapsed(&block_id);
                (store, CliResult::Collapsed(collapsed))
            }
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    let collapsed = store.is_collapsed(&block_id);
                    outputs.push(BatchOutput::Collapsed { input, collapsed });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("fold.status", outputs, errors)))
    }
}
