//! Panel state commands.

use super::{
    BlockId, BlockPanelBarStateCli, execute,
    results::CliResult,
};
use crate::store::{BlockStore, BlockPanelBarState};
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

// =============================================================================
// Execution
// =============================================================================

/// Execute a panel command.
pub fn execute(store: BlockStore, cmd: PanelCommands) -> (BlockStore, CliResult) {
    match cmd {
        | PanelCommands::Set(c) => execute_set(store, &c),
        | PanelCommands::Get(c) => execute_get(store, &c),
        | PanelCommands::Clear(c) => execute_clear(store, &c),
    }
}

fn execute_set(mut store: BlockStore, cmd: &SetPanelCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            store.set_block_panel_state(&block_id, Some(cmd.panel.into()));
            (store, CliResult::Success)
        }
    }
}

fn execute_get(store: BlockStore, cmd: &GetPanelCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            let state = store.block_panel_state(&block_id).map(|s| match s {
                | BlockPanelBarState::Friends => "friends",
                | BlockPanelBarState::Instruction => "instruction",
            });
            (store, CliResult::BlockPanelState(state.map(String::from)))
        }
    }
}

fn execute_clear(mut store: BlockStore, cmd: &ClearPanelCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            store.set_block_panel_state(&block_id, None);
            (store, CliResult::Success)
        }
    }
}
