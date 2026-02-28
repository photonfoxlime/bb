//! Navigation commands.

use super::BlockId;
use clap::Parser;

/// Navigation operations.
#[derive(Debug, Parser)]
pub enum NavCommands {
    /// Get the next visible block in DFS order.
    ///
    /// Traverses depth-first, descending into uncollapsed blocks and skipping
    /// collapsed subtrees. Returns `null` if at the last visible block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Current block position
    ///
    /// # Returns
    ///
    /// Next visible block ID, or null if at end.
    ///
    /// # Example
    /// ```bash
    /// block nav next 1v1
    /// # Output: 2v1
    /// ```
    Next(NextCommand),

    /// Get the previous visible block in DFS order.
    ///
    /// Traverses backward, descending into deepest visible descendants of
    /// previous siblings. Returns `null` if at the first visible block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Current block position
    ///
    /// # Returns
    ///
    /// Previous visible block ID, or null if at start.
    ///
    /// # Example
    /// ```bash
    /// block nav prev 2v1
    /// # Output: 1v1
    /// ```
    Prev(PrevCommand),

    /// Get the lineage (ancestor chain) for a block.
    ///
    /// Returns all ancestor block texts from root to the target (exclusive of
    /// target's own text).
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Returns
    ///
    /// Vector of ancestor block texts in order (root → parent).
    ///
    /// # Example
    /// ```bash
    /// block nav lineage 1v1p
    /// # Output: ["Root", "Section", "Subsection"]
    /// ```
    Lineage(LineageCommand),

    /// Jump to the next query match in DFS order.
    ///
    /// This is cursor-based navigation for search workflows. It evaluates the
    /// query with the same mixed-language matcher as `block find`, then returns
    /// the nearest match strictly after `block_id` in DFS order.
    ///
    /// By default, this wraps to the first match when no later match exists.
    /// Use `--no-wrap` to return `null` instead.
    ///
    /// # Example
    /// ```bash
    /// block nav find-next 1v1 "design"
    /// block nav find-next 1v1 "design" --no-wrap
    /// ```
    FindNext(FindNextCommand),

    /// Jump to the previous query match in DFS order.
    ///
    /// Returns the nearest match strictly before `block_id` in DFS order.
    /// By default, this wraps to the last match when no earlier match exists.
    /// Use `--no-wrap` to return `null` instead.
    ///
    /// # Example
    /// ```bash
    /// block nav find-prev 2v1 "design"
    /// block nav find-prev 2v1 "design" --no-wrap
    /// ```
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
