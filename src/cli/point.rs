//! Point editing commands.
//!
//! This module provides CLI commands for modifying the text content (point) of blocks.
//! Supports both plain text and link points via the `--link` flag.

use super::BlockId;
use clap::Parser;

/// Edit the text content of a block.
///
/// By default, the text is treated as plain text. With `--link`, the text is
/// interpreted as an href and the block's point becomes a [`PointLink`] with
/// [`LinkKind`] inferred from the file extension.
#[derive(Debug, Parser)]
pub struct EditPointCommand {
    /// The block ID to edit.
    ///
    /// Must be a valid NvG format string (e.g., `1v1`, `2v3`).
    /// Fails if the ID does not exist in the store.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// The new text content (or href when `--link` is set) for the block.
    ///
    /// This replaces the existing content entirely.
    #[arg(value_name = "TEXT")]
    pub text: String,

    /// Treat the text as a link href instead of plain text.
    ///
    /// The link kind (image, markdown, or path) is inferred from the file
    /// extension. The previous content is discarded.
    #[arg(long)]
    pub link: bool,
}
