//! Navigation commands.

use super::BlockId;
use clap::Parser;

/// Navigation operations.
#[derive(Debug, Parser)]
pub enum NavCommands {
    /// Get the next visible block in DFS order.
    ///
    /// Traverses depth-first, descending into uncollapsed blocks and skipping
    /// collapsed subtrees.
    /// Returns `null` when there is no next visible block.
    /// Example: `bb nav next 1v1`.
    Next(NextCommand),

    /// Get the previous visible block in DFS order.
    ///
    /// Traverses backward, descending into deepest visible descendants of
    /// previous siblings.
    /// Returns `null` when there is no previous visible block.
    /// Example: `bb nav prev 2v1`.
    Prev(PrevCommand),

    /// Get the lineage (ancestor chain) for a block.
    ///
    /// Returns all ancestor block texts from root to the target (exclusive of
    /// target's own text).
    /// Example: `bb nav lineage 1v1`.
    Lineage(LineageCommand),

    /// Jump to the next query match in DFS order.
    ///
    /// This is cursor-based navigation for search workflows. It evaluates the
    /// query with the same mixed-language matcher as `bb find`, then returns
    /// the nearest match strictly after `block_id` in DFS order.
    ///
    /// By default, this wraps to the first match when no later match exists.
    /// Use `--no-wrap` to return `null` instead.
    /// Example: `bb nav find-next 1v1 "design" --no-wrap`.
    FindNext(FindNextCommand),

    /// Jump to the previous query match in DFS order.
    ///
    /// Returns the nearest match strictly before `block_id` in DFS order.
    /// By default, this wraps to the last match when no earlier match exists.
    /// Use `--no-wrap` to return `null` instead.
    /// Example: `bb nav find-prev 2v1 "design" --no-wrap`.
    FindPrev(FindPrevCommand),
}

/// Get the next visible block.
#[derive(Debug, Parser)]
pub struct NextCommand {
    /// Current block position.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Get the previous visible block.
#[derive(Debug, Parser)]
pub struct PrevCommand {
    /// Current block position.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Get the lineage for a block.
#[derive(Debug, Parser)]
pub struct LineageCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Jump to the next query match from a cursor block.
#[derive(Debug, Parser)]
pub struct FindNextCommand {
    /// Current cursor block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Search query string.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Disable wrap-around when no later match exists.
    #[arg(long)]
    pub no_wrap: bool,
}

/// Jump to the previous query match from a cursor block.
#[derive(Debug, Parser)]
pub struct FindPrevCommand {
    /// Current cursor block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Search query string.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Disable wrap-around when no earlier match exists.
    #[arg(long)]
    pub no_wrap: bool,
}
