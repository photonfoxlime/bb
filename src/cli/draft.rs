//! Draft commands (LLM in-progress suggestions).

use super::{
    BlockId, execute,
    results::{BatchError, BatchOutput, CliResult, ExpansionDraftInfo, ReductionDraftInfo},
};
use crate::store::BlockStore;
use clap::Parser;

/// Draft operations (LLM in-progress suggestions).
#[derive(Debug, Parser)]
pub enum DraftCommands {
    /// Set or update an expansion draft.
    ///
    /// Expansion drafts store LLM-generated rewrite suggestions and proposed
    /// children. Used by the expand operation to present suggestions to the user.
    /// Provide `--rewrite` and/or one or more `--children` values.
    /// Example: `bb draft expand 1v1 --rewrite "Refined version" --children "Proposed child 1" "Proposed child 2"`.
    Expand(ExpandDraftCommand),

    /// Set or update a reduction draft.
    ///
    /// Reduction drafts store a condensed version of a block's content along
    /// with references to children whose info is now captured in the reduction.
    /// Use `--reduction` for the condensed text and optionally add
    /// `--redundant-children` IDs.
    /// Example: `bb draft reduce 1v1 --reduction "All the things"`.
    Reduce(ReduceDraftCommand),

    /// Set or update an instruction draft.
    ///
    /// Instruction drafts store user-authored LLM instructions for a block.
    /// These persist across sessions and are included in LLM context.
    /// Example: `bb draft instruction 1v1 --text "Make this more concise"`.
    Instruction(InstructionDraftCommand),

    /// Set or update an inquiry draft.
    ///
    /// Inquiry drafts store the most recent LLM response to an "ask about this"
    /// query. The user can then apply or dismiss the response.
    /// Example: `bb draft inquiry 1v1 --response "The key insight is..."`.
    Inquiry(InquiryDraftCommand),

    /// List all drafts for a block.
    ///
    /// Shows expansion, reduction, instruction, and inquiry drafts if present.
    /// Example: `bb draft list 1v1 --output json`.
    List(ListDraftCommand),

    /// Clear drafts for a block.
    ///
    /// Use specific flags to clear selected draft kinds, or rely on `--all`
    /// (the default) to clear everything.
    /// Example: `bb draft clear 1v1 --expand`.
    Clear(ClearDraftCommand),
}

/// Set expansion draft.
#[derive(Debug, Parser)]
pub struct ExpandDraftCommand {
    /// Target block ID.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Optional refined text suggestion.
    ///
    /// If not provided, any existing rewrite is cleared.
    #[arg(long, value_name = "TEXT")]
    pub rewrite: Option<String>,

    /// Suggested child text strings.
    ///
    /// Can be repeated to add multiple children.
    /// If not provided, any existing children are cleared.
    #[arg(long, value_name = "TEXT")]
    pub children: Vec<String>,
}

/// Set reduction draft.
#[derive(Debug, Parser)]
pub struct ReduceDraftCommand {
    /// Target block ID.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Condensed text suggestion.
    #[arg(long, value_name = "TEXT")]
    pub reduction: String,

    /// Child IDs whose info is captured by the reduction.
    ///
    /// These children may be deleted after applying the reduction.
    #[arg(long, value_name = "BLOCK_ID")]
    pub redundant_children: Vec<BlockId>,
}

/// Set instruction draft.
#[derive(Debug, Parser)]
pub struct InstructionDraftCommand {
    /// Target block ID.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Instruction text for LLM.
    ///
    /// Empty string clears the draft.
    #[arg(long, value_name = "TEXT")]
    pub text: String,
}

/// Set inquiry draft.
#[derive(Debug, Parser)]
pub struct InquiryDraftCommand {
    /// Target block ID.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// LLM response text.
    ///
    /// Trimmed of leading/trailing whitespace.
    /// Empty (after trim) clears the draft.
    #[arg(long, value_name = "TEXT")]
    pub response: String,
}

/// List all drafts.
#[derive(Debug, Parser)]
pub struct ListDraftCommand {
    /// Target block ID.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Clear drafts.
#[derive(Debug, Parser)]
pub struct ClearDraftCommand {
    /// Target block ID.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Clear expansion draft.
    #[arg(long)]
    pub expand: bool,

    /// Clear reduction draft.
    #[arg(long)]
    pub reduce: bool,

    /// Clear instruction draft.
    #[arg(long)]
    pub instruction: bool,

    /// Clear inquiry draft.
    #[arg(long)]
    pub inquiry: bool,

    /// Clear all drafts.
    ///
    /// This is the default if no specific flag is provided.
    #[arg(long, default_value = "true")]
    pub all: bool,
}

// =============================================================================
// Execution
// =============================================================================

/// Execute a draft command.
pub fn execute(store: BlockStore, cmd: DraftCommands) -> (BlockStore, CliResult) {
    match cmd {
        | DraftCommands::Expand(c) => execute_expand(store, &c),
        | DraftCommands::Reduce(c) => execute_reduce(store, &c),
        | DraftCommands::Instruction(c) => execute_instruction(store, &c),
        | DraftCommands::Inquiry(c) => execute_inquiry(store, &c),
        | DraftCommands::List(c) => execute_list(store, &c),
        | DraftCommands::Clear(c) => execute_clear(store, &c),
    }
}

fn execute_expand(mut store: BlockStore, cmd: &ExpandDraftCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            let draft = crate::store::ExpansionDraftRecord {
                rewrite: cmd.rewrite.clone(),
                children: cmd.children.clone(),
            };
            store.insert_expansion_draft(block_id, draft);
            (store, CliResult::Success)
        }
    }
}

fn execute_reduce(mut store: BlockStore, cmd: &ReduceDraftCommand) -> (BlockStore, CliResult) {
    let id = execute::resolve_block_id(&store, &cmd.block_id);
    match id {
        | None => (store, CliResult::Error("Unknown block ID".to_string())),
        | Some(block_id) => {
            let redundant: Vec<_> = cmd
                .redundant_children
                .iter()
                .filter_map(|c| execute::resolve_block_id(&store, c))
                .collect();
            let draft = crate::store::ReductionDraftRecord {
                reduction: Some(cmd.reduction.clone()),
                redundant_children: redundant,
            };
            store.insert_reduction_draft(block_id, draft);
            (store, CliResult::Success)
        }
    }
}

fn execute_instruction(
    mut store: BlockStore, cmd: &InstructionDraftCommand,
) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                store.set_instruction_draft(block_id, cmd.text.clone());
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
                    store.set_instruction_draft(block_id, cmd.text.clone());
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("draft.instruction", outputs, errors)))
    }
}

fn execute_inquiry(mut store: BlockStore, cmd: &InquiryDraftCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                store.set_inquiry_draft(block_id, cmd.response.clone());
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
                    store.set_inquiry_draft(block_id, cmd.response.clone());
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("draft.inquiry", outputs, errors)))
    }
}

fn execute_list(store: BlockStore, cmd: &ListDraftCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let expansion = store.expansion_draft(&block_id).map(|d| ExpansionDraftInfo {
                    rewrite: d.rewrite.clone(),
                    children: d.children.clone(),
                });
                let reduction = store.reduction_draft(&block_id).map(|d| ReductionDraftInfo {
                    reduction: d.reduction.clone(),
                    redundant_children: d
                        .redundant_children
                        .iter()
                        .map(|id| format!("{}", id))
                        .collect(),
                });
                let instruction = store.instruction_draft(&block_id).map(|d| d.instruction.clone());
                let inquiry = store.inquiry_draft(&block_id).map(|d| d.response.clone());
                (store, CliResult::DraftList { expansion, reduction, instruction, inquiry })
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
                    let expansion = store.expansion_draft(&block_id).map(|d| ExpansionDraftInfo {
                        rewrite: d.rewrite.clone(),
                        children: d.children.clone(),
                    });
                    let reduction = store.reduction_draft(&block_id).map(|d| ReductionDraftInfo {
                        reduction: d.reduction.clone(),
                        redundant_children: d
                            .redundant_children
                            .iter()
                            .map(|id| format!("{}", id))
                            .collect(),
                    });
                    let instruction =
                        store.instruction_draft(&block_id).map(|d| d.instruction.clone());
                    let inquiry = store.inquiry_draft(&block_id).map(|d| d.response.clone());
                    outputs.push(BatchOutput::DraftList {
                        input,
                        expansion,
                        reduction,
                        instruction,
                        inquiry,
                    });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("draft.list", outputs, errors)))
    }
}

fn execute_clear(mut store: BlockStore, cmd: &ClearDraftCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                if cmd.all || cmd.expand {
                    store.remove_expansion_draft(&block_id);
                }
                if cmd.all || cmd.reduce {
                    store.remove_reduction_draft(&block_id);
                }
                if cmd.all || cmd.instruction {
                    store.remove_instruction_draft(&block_id);
                }
                if cmd.all || cmd.inquiry {
                    store.remove_inquiry_draft(&block_id);
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
                    if cmd.all || cmd.expand {
                        store.remove_expansion_draft(&block_id);
                    }
                    if cmd.all || cmd.reduce {
                        store.remove_reduction_draft(&block_id);
                    }
                    if cmd.all || cmd.instruction {
                        store.remove_instruction_draft(&block_id);
                    }
                    if cmd.all || cmd.inquiry {
                        store.remove_inquiry_draft(&block_id);
                    }
                    outputs.push(BatchOutput::Success { input });
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("draft.clear", outputs, errors)))
    }
}
