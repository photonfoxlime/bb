//! Commands execution implementation.
//!
//! This module implements the dispatch logic for CLI commands. Execution logic
//! for each command category lives in its corresponding module (query, tree,
//! nav, draft, fold, friend, mount, panel, context, point).

use super::{
    BlockId,
    commands::Commands,
    context, draft, fold, friend, mount, nav, panel, point, query,
    results::{BatchError, BatchOutput, BatchResult},
    tree,
};
use crate::store::{BlockStore, MountFormat};

// =============================================================================
// Command dispatch
// =============================================================================

impl Commands {
    /// Execute a block command with the given store.
    ///
    /// This is the main entry point for CLI command execution. It dispatches
    /// to the appropriate module's execute function based on the command variant.
    pub fn execute(
        self, store: BlockStore, base_dir: &std::path::Path,
    ) -> (BlockStore, super::results::CliResult) {
        match self {
            | Commands::GenerateCompletion { .. } => {
                unreachable!("GenerateCompletion is handled before execute")
            }
            | Commands::Roots(_) => query::execute_roots(store),
            | Commands::Show(cmd) => query::execute_show(store, &cmd),
            | Commands::Find(cmd) => query::execute_find(store, &cmd),
            | Commands::Point(cmd) => point::execute_point(store, &cmd),
            | Commands::Tree(cmd) => tree::execute(store, cmd),
            | Commands::Nav(cmd) => nav::execute(store, cmd),
            | Commands::Draft(cmd) => draft::execute(store, cmd),
            | Commands::Fold(cmd) => fold::execute(store, cmd),
            | Commands::Friend(cmd) => friend::execute(store, cmd),
            | Commands::Mount(cmd) => mount::execute(store, cmd, base_dir),
            | Commands::Panel(cmd) => panel::execute(store, cmd),
            | Commands::Context(cmd) => context::execute(store, &cmd),
        }
    }
}

// =============================================================================
// Shared execution helpers
// =============================================================================

/// Expand one CLI ID field into one-or-many targets.
///
/// Batch mode is enabled by providing comma-separated IDs in a single
/// argument (for example, `1v1,2v1,3v1`). Empty tokens are ignored.
pub fn expand_cli_targets(single: &BlockId) -> Vec<BlockId> {
    let mut all = Vec::new();
    for part in single.0.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        all.push(BlockId(trimmed.to_string()));
    }
    if all.is_empty() {
        all.push(single.clone());
    }
    all
}

/// Expand two CLI ID fields into operation pairs.
///
/// Pairing rules:
/// - same lengths: zip by index,
/// - one side has length 1: broadcast over the other side,
/// - otherwise: return an error.
pub fn expand_cli_pairs(
    left: &BlockId, right: &BlockId,
) -> Result<Vec<(BlockId, BlockId)>, String> {
    let lefts = expand_cli_targets(left);
    let rights = expand_cli_targets(right);

    if lefts.len() == rights.len() {
        return Ok(lefts.into_iter().zip(rights).collect());
    }

    if lefts.len() == 1 {
        let left = lefts[0].clone();
        return Ok(rights.into_iter().map(|right| (left.clone(), right)).collect());
    }

    if rights.len() == 1 {
        let right = rights[0].clone();
        return Ok(lefts.into_iter().map(|left| (left, right.clone())).collect());
    }

    Err("batch pair mismatch: ID list lengths must match, or one side must contain exactly one ID"
        .to_string())
}

/// Resolve a CLI BlockId string to an actual store BlockId.
///
/// Performs flexible, case-insensitive matching on block ID strings.
/// Format: `NvG` where N=index and G=generation (e.g., `1v1`, `2v3`).
pub fn resolve_block_id(store: &BlockStore, cli_id: &BlockId) -> Option<crate::store::BlockId> {
    let cli_str = &cli_id.0;
    for (id, _) in &store.nodes {
        let id_str = format!("{}", id);
        if id_str.eq_ignore_ascii_case(cli_str) {
            return Some(id);
        }
    }
    None
}

/// Build a standardized continue-on-error batch result.
pub fn make_batch_result(
    operation: &str, outputs: Vec<BatchOutput>, errors: Vec<BatchError>,
) -> BatchResult {
    let successes = outputs.len();
    let failures = errors.len();
    BatchResult { operation: operation.to_string(), successes, failures, outputs, errors }
}

/// Returns true when a path should be treated as a directory target.
pub fn is_directory_like(path: &std::path::Path) -> bool {
    path.is_dir() || path.extension().is_none()
}

/// Build a per-target file path under a directory-like base path.
pub fn batch_child_file_path(
    base: &std::path::Path, target: &str, ext: &str,
) -> std::path::PathBuf {
    base.join(format!("{}.{}", target, ext))
}

/// File extension used for each mount format in batch path generation.
pub fn mount_format_extension(format: MountFormat) -> &'static str {
    match format {
        | MountFormat::Json => "json",
        | MountFormat::Markdown => "md",
    }
}

/// Best-effort mount format lookup for a block.
pub fn mount_format_for_block(
    store: &BlockStore, block_id: &crate::store::BlockId,
) -> Option<MountFormat> {
    if let Some(entry) = store.mount_table().entry(*block_id) {
        return Some(entry.format);
    }
    match store.node(block_id) {
        | Some(crate::store::BlockNode::Mount { format, .. }) => Some(*format),
        | _ => None,
    }
}
