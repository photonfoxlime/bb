//! Query commands: roots, show, find.

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult, Match, ShowResult},
};
use crate::store::BlockStore;
use clap::Parser;

/// Query root block IDs.
#[derive(Debug, Parser)]
pub struct RootCommand {}

/// Show detailed information about a block.
#[derive(Debug, Parser)]
pub struct ShowCommand {
    /// The block ID to display.
    ///
    /// Must be a valid UUID string.
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
    /// Example: `bb find "TODO"`.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Maximum number of results to return.
    ///
    /// Defaults to 100.
    /// Example: `bb find "design" --limit 5`.
    #[arg(long, short, value_name = "N", default_value = "100")]
    pub limit: usize,
}

// =============================================================================
// Execution
// =============================================================================

/// Execute the `roots` command.
pub fn execute_roots(store: BlockStore) -> (BlockStore, CliResult) {
    let roots: Vec<String> = store.roots().iter().map(|id| format!("{}", id)).collect();
    (store, CliResult::Roots(roots))
}

/// Execute the `show` command.
pub fn execute_show(store: BlockStore, cmd: &ShowCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(id) => {
                let text = store.point(&id).unwrap_or_default();
                let children: Vec<String> =
                    store.children(&id).iter().map(|c| format!("{}", c)).collect();
                let show = ShowResult { id: format!("{}", id), text, children };
                (store, CliResult::Show(show))
            }
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(id) => {
                    let text = store.point(&id).unwrap_or_default();
                    let children: Vec<String> =
                        store.children(&id).iter().map(|c| format!("{}", c)).collect();
                    let show = ShowResult { id: format!("{}", id), text, children };
                    outputs.push(BatchOutput::Show { input, show });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("query.show", outputs, errors)))
    }
}

/// Execute the `find` command.
pub fn execute_find(store: BlockStore, cmd: &FindCommand) -> (BlockStore, CliResult) {
    let matches: Vec<Match> = store
        .find_block_point(&cmd.query)
        .into_iter()
        .filter_map(|id| {
            let text = store.point(&id)?;
            Some(Match { id: format!("{}", id), text })
        })
        .take(cmd.limit)
        .collect();
    (store, CliResult::Find(matches))
}
