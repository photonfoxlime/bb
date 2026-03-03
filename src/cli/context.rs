//! Context command (LLM context for a block).

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::BlockStore;
use clap::Parser;

/// Get LLM context for a block.
#[derive(Debug, Parser)]
pub struct ContextCommand {
    /// Target block.
    ///
    /// The context includes lineage text, direct children text, and friend
    /// metadata for LLM requests.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

// =============================================================================
// Execution
// =============================================================================

/// Execute the context command.
pub fn execute(store: BlockStore, cmd: &ContextCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let context = store.block_context_for_id(&block_id);
                (store, CliResult::Context(context))
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
                    let context = store.block_context_for_id(&block_id);
                    let fmt = crate::llm::ContextFormatter::from_block_context(&context);
                    outputs.push(BatchOutput::Context {
                        input,
                        lineage: fmt.lineage_points().map(String::from).collect(),
                        children: fmt.children().clone(),
                        friends: fmt.friends_count(),
                    });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("context", outputs, errors)))
    }
}
