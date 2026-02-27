//! Context command (LLM context for a block).

use crate::cli::types::BlockId;
use clap::Parser;

/// Get LLM context for a block.
#[derive(Debug, Parser)]
pub struct ContextCommand {
    /// Target block.
    ///
    /// The context includes:
    /// - Lineage: ancestor block texts (root to parent)
    /// - Children: direct children's text
    /// - Friends: friend block info with perspectives
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}
