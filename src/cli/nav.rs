//! Navigation commands.

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::BlockStore;
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

// =============================================================================
// Execution
// =============================================================================

/// Execute a nav command.
pub fn execute(store: BlockStore, cmd: NavCommands) -> (BlockStore, CliResult) {
    match cmd {
        | NavCommands::Next(c) => execute_next(store, &c),
        | NavCommands::Prev(c) => execute_prev(store, &c),
        | NavCommands::Lineage(c) => execute_lineage(store, &c),
        | NavCommands::FindNext(c) => execute_find_next(store, &c),
        | NavCommands::FindPrev(c) => execute_find_prev(store, &c),
    }
}

fn execute_next(store: BlockStore, cmd: &NextCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let next = store.next_visible_in_dfs(&block_id);
                (store, CliResult::OptionalBlockId(next))
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
                    let next = store.next_visible_in_dfs(&block_id);
                    outputs.push(BatchOutput::OptionalId {
                        input,
                        id: next.map(|id| format!("{}", id)),
                    });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("nav.next", outputs, errors)))
    }
}

fn execute_prev(store: BlockStore, cmd: &PrevCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let prev = store.prev_visible_in_dfs(&block_id);
                (store, CliResult::OptionalBlockId(prev))
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
                    let prev = store.prev_visible_in_dfs(&block_id);
                    outputs.push(BatchOutput::OptionalId {
                        input,
                        id: prev.map(|id| format!("{}", id)),
                    });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("nav.prev", outputs, errors)))
    }
}

fn execute_lineage(store: BlockStore, cmd: &LineageCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let lineage = store.lineage_points_for_id(&block_id);
                (store, CliResult::Lineage(lineage))
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
                    let lineage = store.lineage_points_for_id(&block_id);
                    outputs.push(BatchOutput::Lineage { input, lineage });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("nav.lineage", outputs, errors)))
    }
}

fn execute_find_next(store: BlockStore, cmd: &FindNextCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            let next = find_relative_query_match(
                &store,
                &block_id,
                &cmd.query,
                true,
                !cmd.no_wrap,
            );
            (store, CliResult::OptionalBlockId(next))
        }
    }
}

fn execute_find_prev(store: BlockStore, cmd: &FindPrevCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            let prev = find_relative_query_match(
                &store,
                &block_id,
                &cmd.query,
                false,
                !cmd.no_wrap,
            );
            (store, CliResult::OptionalBlockId(prev))
        }
    }
}

/// Find the nearest query match before/after a cursor block in DFS order.
fn find_relative_query_match(
    store: &BlockStore,
    cursor: &crate::store::BlockId,
    query: &str,
    forward: bool,
    wrap: bool,
) -> Option<crate::store::BlockId> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let matches = store.find_block_point(query);
    if matches.is_empty() {
        return None;
    }

    let all_ids = store.find_block_point("");
    let cursor_position = all_ids.iter().position(|id| id == cursor)?;

    if forward {
        for matched in &matches {
            let Some(position) = all_ids.iter().position(|id| id == matched) else {
                continue;
            };
            if position > cursor_position {
                return Some(*matched);
            }
        }
        if wrap {
            return matches.first().copied();
        }
        return None;
    }

    let mut candidate = None;
    for matched in &matches {
        let Some(position) = all_ids.iter().position(|id| id == matched) else {
            continue;
        };
        if position < cursor_position {
            candidate = Some(*matched);
        } else {
            break;
        }
    }

    if candidate.is_some() {
        candidate
    } else if wrap {
        matches.last().copied()
    } else {
        None
    }
}
