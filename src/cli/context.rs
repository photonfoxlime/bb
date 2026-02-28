//! Context command (LLM context for a block).

use super::BlockId;
use clap::Parser;

/// Get LLM context for a block.
#[derive(Debug, Parser)]
pub struct ContextCommand {
    /// Target block.
    ///
    /// The context includes lineage text, direct children text, and friend
    /// metadata for LLM requests.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}
