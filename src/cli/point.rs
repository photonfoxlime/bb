//! Point editing commands.
//!
//! This module provides CLI commands for modifying the text content (point) of blocks.
//! Supports appending link chips via the `--link` flag.

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::{BlockStore, PointLink};
use clap::Parser;

/// Edit the text content of a block.
///
/// By default, the text is treated as plain text and replaces the block's
/// current text. With `--link`, the text is interpreted as an href and a new
/// [`PointLink`] is appended to the block's links; the existing text is
/// unchanged.
#[derive(Debug, Parser)]
pub struct EditPointCommand {
    /// The block ID to edit.
    ///
    /// Must be a valid NvG format string (e.g., `1v1`, `2v3`).
    /// Fails if the ID does not exist in the store.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// The new text content (or href when `--link` is set) for the block.
    #[arg(value_name = "TEXT")]
    pub text: String,

    /// Append the text as a link href rather than setting the plain text.
    ///
    /// The link kind (image, markdown, or path) is inferred from the file
    /// extension. The block's existing text is preserved.
    #[arg(long)]
    pub link: bool,
}

// =============================================================================
// Execution
// =============================================================================

/// Execute the `point` command.
pub fn execute_point(mut store: BlockStore, cmd: &EditPointCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                if cmd.link {
                    store.add_link_to_point(&block_id, PointLink::infer(&cmd.text));
                } else {
                    store.update_point(&block_id, cmd.text.clone());
                }
                (store, CliResult::Success)
            }
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    if cmd.link {
                        store.add_link_to_point(&block_id, PointLink::infer(&cmd.text));
                    } else {
                        store.update_point(&block_id, cmd.text.clone());
                    }
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("point", outputs, errors)))
    }
}
