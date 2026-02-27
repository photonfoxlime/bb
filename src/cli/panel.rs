//! Panel state commands.

use crate::cli::types::{BlockId, PanelBarStateCli};
use clap::Parser;

/// Panel state operations.
#[derive(Debug, Parser)]
pub enum PanelCommands {
    /// Set the panel state for a block.
    ///
    /// Persists which panel (Friends or Instruction) is open for a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `panel`: Panel name (friends or instruction)
    ///
    /// # Example
    /// ```bash
    /// block panel set 0x123 friends
    /// block panel set 0x123 instruction
    /// ```
    Set(SetPanelCommand),

    /// Get the panel state for a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Example
    /// ```bash
    /// block panel get 0x123
    /// # Output: {"panel": "friends"}
    /// ```
    Get(GetPanelCommand),

    /// Clear the panel state.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Example
    /// ```bash
    /// block panel clear 0x123
    /// ```
    Clear(ClearPanelCommand),
}

/// Set panel state.
#[derive(Debug, Parser)]
pub struct SetPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Panel to show.
    ///
    /// - `friends`: Show friends panel
    /// - `instruction`: Show instruction editor
    #[arg(value_name = "PANEL")]
    pub panel: PanelBarStateCli,
}

/// Get panel state.
#[derive(Debug, Parser)]
pub struct GetPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Clear panel state.
#[derive(Debug, Parser)]
pub struct ClearPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}
