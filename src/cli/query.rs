//! Query commands: roots, show, find.

use super::BlockId;
use clap::Parser;

/// Query root block IDs.
#[derive(Debug, Parser)]
pub struct RootCommand {}

/// Show detailed information about a block.
#[derive(Debug, Parser)]
pub struct ShowCommand {
    /// The block ID to display.
    ///
    /// Must be a valid NvG format string (for example, `1v1`).
    /// Fails if the ID is not found in the store.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Search blocks by text content.
#[derive(Debug, Parser)]
pub struct FindCommand {
    /// Search query string.
    ///
    /// Matching uses `BlockStore::find_block_point` with case-insensitive
    /// substring matching and mixed-language phrase-token matching.
    /// Empty query matches all blocks in deterministic DFS order.
    /// Example: `blooming-blockery block find "TODO"`.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Maximum number of results to return.
    ///
    /// Defaults to 100.
    /// Example: `blooming-blockery block find "design" --limit 5`.
    #[arg(long, short, value_name = "N", default_value = "100")]
    pub limit: usize,
}
