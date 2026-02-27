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
    /// block nav next 0x1a2b3c
    /// # Output: 0x4d5e6f
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
    /// block nav prev 0x4d5e6f
    /// # Output: 0x1a2b3c
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
    /// block nav lineage 0xdeep
    /// # Output: ["Root", "Section", "Subsection"]
    /// ```
    Lineage(LineageCommand),
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
