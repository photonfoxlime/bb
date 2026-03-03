//! Panel state commands.

use super::{BlockId, BlockPanelBarStateCli};
use clap::Parser;

/// Panel state operations.
#[derive(Debug, Parser)]
pub enum PanelCommands {
    /// Set the block panel state for a block.
    ///
    /// Persists which panel (Friends or Instruction) is open for a block.
    /// Example: `bb panel set 1v1 friends`.
    Set(SetPanelCommand),

    /// Get the block panel state for a block.
    /// Example: `bb panel get 1v1`.
    Get(GetPanelCommand),

    /// Clear the block panel state.
    /// Example: `bb panel clear 1v1`.
    Clear(ClearPanelCommand),
}

/// Set block panel state.
#[derive(Debug, Parser)]
pub struct SetPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Panel to show.
    ///
    /// Use `friends` to show the friends panel or `instruction` to show the
    /// instruction editor.
    #[arg(value_name = "PANEL")]
    pub panel: BlockPanelBarStateCli,
}

/// Get block panel state.
#[derive(Debug, Parser)]
pub struct GetPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Clear block panel state.
#[derive(Debug, Parser)]
pub struct ClearPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}
