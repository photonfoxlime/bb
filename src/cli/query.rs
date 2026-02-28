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
    /// Must be a valid NvG format string (e.g., `1v14d5e`).
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: ID not found in store.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Search blocks by text content.
#[derive(Debug, Parser)]
pub struct FindCommand {
    /// Search query string.
    ///
    /// Matching is delegated to `BlockStore::find_block_point`:
    /// - case-insensitive full-query substring match, and
    /// - case-insensitive mixed-language phrase-token matching.
    ///
    /// Empty query matches all blocks in deterministic DFS order.
    ///
    /// # Example
    /// ```bash
    /// block find "TODO"
    /// block find ""  # Returns all blocks (subject to --limit)
    /// ```
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Maximum number of results to return.
    ///
    /// Defaults to 100. Use `--limit 10` for minimal output.
    ///
    /// # Example
    /// ```bash
    /// block find "design" --limit 5
    /// ```
    #[arg(long, short, value_name = "N", default_value = "100")]
    pub limit: usize,
}
