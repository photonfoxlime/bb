//! Draft commands (LLM in-progress suggestions).

use super::BlockId;
use clap::Parser;

/// Draft operations (LLM in-progress suggestions).
#[derive(Debug, Parser)]
pub enum DraftCommands {
    /// Set or update an expansion draft.
    ///
    /// Expansion drafts store LLM-generated rewrite suggestions and proposed
    /// children. Used by the expand operation to present suggestions to the user.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `--rewrite`: Optional refined text for the block
    /// - `--children`: Suggested child text strings (can be repeated)
    ///
    /// # Returns
    ///
    /// Success indicator.
    ///
    /// # Example
    /// ```bash
    /// block draft expand 1v1 --rewrite "Refined version" \
    ///     --children "Proposed child 1" "Proposed child 2"
    ///
    /// # Set children only (no rewrite)
    /// block draft expand 1v1 --children "Just kids"
    /// ```
    Expand(ExpandDraftCommand),

    /// Set or update a reduction draft.
    ///
    /// Reduction drafts store a condensed version of a block's content along
    /// with references to children whose info is now captured in the reduction.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `--reduction`: The condensed text
    /// - `--redundant-children`: Child IDs whose info is captured (optional)
    ///
    /// # Example
    /// ```bash
    /// block draft reduce 1v1 \
    ///     --reduction "All the things" \
    ///     --redundant-children 1v1 1v1
    /// ```
    Reduce(ReduceDraftCommand),

    /// Set or update an instruction draft.
    ///
    /// Instruction drafts store user-authored LLM instructions for a block.
    /// These persist across sessions and are included in LLM context.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `--text`: Instruction text
    ///
    /// # Example
    /// ```bash
    /// block draft instruction 1v1 --text "Make this more concise"
    /// ```
    Instruction(InstructionDraftCommand),

    /// Set or update an inquiry draft.
    ///
    /// Inquiry drafts store the most recent LLM response to an "ask about this"
    /// query. The user can then apply or dismiss the response.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `--response`: LLM response text
    ///
    /// # Example
    /// ```bash
    /// block draft inquiry 1v1 --response "The key insight is..."
    /// ```
    Inquiry(InquiryDraftCommand),

    /// List all drafts for a block.
    ///
    /// Shows expansion, reduction, instruction, and inquiry drafts if present.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Example
    /// ```bash
    /// block draft list 1v1
    /// block draft list 1v1 --output json
    /// ```
    List(ListDraftCommand),

    /// Clear drafts for a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `--expand`: Clear expansion draft only
    /// - `--reduce`: Clear reduction draft only
    /// - `--instruction`: Clear instruction draft only
    /// - `--inquiry`: Clear inquiry draft only
    /// - `--all`: Clear all drafts (default)
    ///
    /// # Example
    /// ```bash
    /// block draft clear 1v1 --all
    /// block draft clear 1v1 --expand
    /// ```
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
