//! Fold (collapse) state commands.

use super::BlockId;
use clap::Parser;

/// Fold (collapse) state operations.
#[derive(Debug, Parser)]
pub enum FoldCommands {
    /// Toggle the fold state of a block.
    ///
    /// If collapsed, expands to show children. If expanded, collapses to hide.
    /// Returns `true` when the block is collapsed after the operation.
    /// Example: `blooming-blockery block fold toggle 1v1`.
    Toggle(ToggleFoldCommand),

    /// Get the fold state of a block.
    /// Returns `true` if collapsed and `false` if expanded.
    /// Example: `blooming-blockery block fold status 1v1`.
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
