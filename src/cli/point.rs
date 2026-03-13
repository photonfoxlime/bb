//! Point editing commands.
//!
//! Subcommands for modifying the point content of a block. Point content has
//! two independent parts: plain `text` and a `links` list. Each subcommand
//! targets exactly one part.

use super::{
    BlockId, execute, friend,
    friend::FriendCommands,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::{BlockStore, PointLink};
use clap::Parser;

// =============================================================================
// Subcommands
// =============================================================================

/// Edit a block's point content (text and links).
#[derive(Debug, Parser)]
pub enum PointCommands {
    /// Replace the plain text of a block, leaving its links unchanged.
    Set(SetPointCommand),
    /// Append a new link to a block's link list.
    #[command(name = "link-add")]
    LinkAdd(LinkAddCommand),
    /// Remove a link from a block by its zero-based index.
    #[command(name = "link-remove")]
    LinkRemove(LinkRemoveCommand),

    /// Friend block (cross-reference) management.
    #[command(subcommand)]
    Friend(FriendCommands),
}

/// Replace the plain text of a block.
#[derive(Debug, Parser)]
pub struct SetPointCommand {
    /// Block ID to edit. Accepts comma-separated IDs for batch mode.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// New plain text. Existing links are preserved.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Append a link to a block's link list.
#[derive(Debug, Parser)]
pub struct LinkAddCommand {
    /// Block ID to add a link to. Accepts comma-separated IDs for batch mode.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Link target: a file path or URL.
    ///
    /// The link kind (image, markdown, or path) is inferred from the file
    /// extension.
    #[arg(value_name = "HREF")]
    pub href: String,

    /// Human-readable label. When absent the href is shown directly.
    #[arg(long, value_name = "LABEL")]
    pub label: Option<String>,

    /// Framing note for how this block should interpret the link.
    #[arg(long, value_name = "PERSPECTIVE")]
    pub perspective: Option<String>,
}

/// Remove a link from a block by its zero-based index.
#[derive(Debug, Parser)]
pub struct LinkRemoveCommand {
    /// Block ID to remove a link from. Accepts comma-separated IDs for batch
    /// mode.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Zero-based index of the link to remove.
    #[arg(value_name = "INDEX")]
    pub index: usize,
}

// =============================================================================
// Execution
// =============================================================================

/// Execute a `point` subcommand.
pub fn execute(store: BlockStore, cmd: PointCommands) -> (BlockStore, CliResult) {
    match cmd {
        | PointCommands::Set(cmd) => execute_set(store, cmd),
        | PointCommands::LinkAdd(cmd) => execute_link_add(store, cmd),
        | PointCommands::LinkRemove(cmd) => execute_link_remove(store, cmd),
        | PointCommands::Friend(cmd) => friend::execute(store, cmd),
    }
}

fn execute_set(mut store: BlockStore, cmd: SetPointCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        match execute::resolve_block_id(&store, &targets[0]) {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                store.update_point(&block_id, cmd.text.clone());
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
                    store.update_point(&block_id, cmd.text.clone());
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("point set", outputs, errors)))
    }
}

fn execute_link_add(mut store: BlockStore, cmd: LinkAddCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        match execute::resolve_block_id(&store, &targets[0]) {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                store.add_link_to_point(&block_id, build_link(&cmd));
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
                    store.add_link_to_point(&block_id, build_link(&cmd));
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("point link-add", outputs, errors)))
    }
}

fn execute_link_remove(mut store: BlockStore, cmd: LinkRemoveCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        match execute::resolve_block_id(&store, &targets[0]) {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                store.remove_link_from_point(&block_id, cmd.index);
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
                    store.remove_link_from_point(&block_id, cmd.index);
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("point link-remove", outputs, errors)))
    }
}

fn build_link(cmd: &LinkAddCommand) -> PointLink {
    let mut link = PointLink::infer(&cmd.href);
    if let Some(label) = &cmd.label {
        link = link.with_label(label.clone());
    }
    if let Some(perspective) = &cmd.perspective {
        link = link.with_perspective(perspective.clone());
    }
    link
}
