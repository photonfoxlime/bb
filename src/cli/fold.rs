//! Fold (collapse) state commands.

use crate::cli::types::BlockId;
use clap::Parser;

/// Fold (collapse) state operations.
#[derive(Debug, Parser)]
pub enum FoldCommands {
    /// Toggle the fold state of a block.
    ///
    /// If collapsed, expands to show children. If expanded, collapses to hide.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block (must have children to be collapsible)
    ///
    /// # Returns
    ///
    /// New fold state: `true` = collapsed, `false` = expanded.
    ///
    /// # Example
    /// ```bash
    /// block fold toggle 0x123
    /// # Output: {"collapsed": true}
    /// ```
    Toggle(ToggleFoldCommand),

    /// Get the fold state of a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Returns
    ///
    /// `true` if collapsed, `false` if expanded.
    ///
    /// # Example
    /// ```bash
    /// block fold status 0x123
    /// # Output: {"collapsed": false}
    /// ```
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
