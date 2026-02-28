//! Point editing commands.
//!
//! This module provides CLI commands for modifying the text content (point) of blocks.

use super::BlockId;
use clap::Parser;

/// Edit the text content of a block.
#[derive(Debug, Parser)]
pub struct EditPointCommand {
    /// The block ID to edit.
    ///
    /// Must be a valid NvG format string (e.g., `1v1`, `2v3`).
    /// Fails if the ID does not exist in the store.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// The new text content for the block.
    ///
    /// This replaces the existing text entirely.
    #[arg(value_name = "TEXT")]
    pub text: String,
}
