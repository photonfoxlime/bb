//! CLI command results and output types.
//!
//! This module defines the result types returned by `Commands::execute()`.
//! These types encapsulate all possible outcomes of CLI operations and are
//! subsequently formatted by `cli::output::print_result()` based on the
//! user's requested output format (JSON or table).
//!
//! # Design
//!
//! - `CliResult` is the single source of truth for CLI output
//! - All variants are serializable to JSON for `--output json` mode
//! - Auxiliary structs (`Match`, `FriendInfo`, etc.) are marked with `#[derive(serde::Serialize)]`
//!
//! # Output Flow
//!
//! ```text
//! Commands::execute() -> CliResult
//!     └─> print_result(&CliResult, OutputFormat)
//!         ├─> JSON: serde_json::json!() for all variants
//!         └─> Table: Human-readable formatting per variant
//! ```

use crate::llm::{BlockContext, ChildrenContext, LineageContext};
use crate::store::BlockId;

/// Result of executing a block command.
///
/// Each variant corresponds to a specific type of command output.
/// The `print_result()` function in `cli::output` handles formatting
/// for both JSON and table output modes.
#[derive(Debug)]
pub enum CliResult {
    /// Command succeeded with no structured output.
    ///
    /// Printed as "OK" in table mode.
    Success,
    /// Command failed with an error message.
    ///
    /// Printed to stderr as "Error: {msg}" in both modes.
    Error(String),
    /// List of root block IDs.
    ///
    /// Returned by the `roots` command.
    Roots(Vec<String>),
    /// Show block details.
    ///
    /// Returned by the `show` command, containing the block's ID,
    /// point text, and immediate children IDs.
    Show(ShowResult),
    /// Search results from the `find` command.
    Find(Vec<Match>),
    /// A single block ID from creation operations.
    ///
    /// Returned by tree editing commands like `add-child`, `add-sibling`,
    /// `wrap`, and `duplicate`.
    BlockId(BlockId),
    /// Optional block ID from navigation operations.
    ///
    /// Returned by `next` and `prev` commands. `None` indicates
    /// no further navigation in the requested direction.
    OptionalBlockId(Option<BlockId>),
    /// List of removed block IDs.
    ///
    /// Returned by the `delete` command, containing all IDs in the
    /// removed subtree.
    Removed(Vec<String>),
    /// Collapsed state boolean.
    ///
    /// Returned by `fold toggle` and `fold status` commands.
    Collapsed(bool),
    /// Lineage points from ancestor chain.
    ///
    /// Returned by the `nav lineage` command, containing the
    /// point text of all ancestors from root to parent.
    Lineage(LineageContext),
    /// LLM context information.
    ///
    /// Returned by the `context` command, providing the data
    /// that would be sent to an LLM for this block.
    Context(BlockContext),
    /// Draft listing result.
    ///
    /// Returned by `draft list`, showing all active drafts for a block.
    DraftList {
        expansion: Option<ExpansionDraftInfo>,
        reduction: Option<ReductionDraftInfo>,
        instruction: Option<String>,
        inquiry: Option<String>,
    },
    /// Friend list result.
    ///
    /// Returned by `friend list`, containing all cross-references
    /// for the target block.
    FriendList(Vec<FriendInfo>),
    /// Mount information.
    ///
    /// Returned by `mount info`, describing the mount path, format,
    /// and expansion state.
    MountInfo { path: Option<String>, format: String, expanded: bool },
    /// Number of inlined mount points.
    ///
    /// Returned by `mount inline-recursive`.
    MountInlined(usize),
    /// Panel sidebar state.
    ///
    /// Returned by `panel get`, showing the current sidebar mode.
    BlockPanelState(Option<String>),
    /// Batch execution report with per-item outputs and collected failures.
    ///
    /// Used by commands that support processing multiple target IDs in one run
    /// while continuing through all items and reporting failures at the end.
    Batch(BatchResult),
}

/// Continue-on-error report for batched CLI operations.
#[derive(Debug, serde::Serialize)]
pub struct BatchResult {
    /// Operation identifier (for example, `tree.add-child`).
    pub operation: String,
    /// Number of successful items.
    pub successes: usize,
    /// Number of failed items.
    pub failures: usize,
    /// Per-item outputs produced by successful items.
    pub outputs: Vec<BatchOutput>,
    /// Per-item errors collected during execution.
    pub errors: Vec<BatchError>,
}

/// One per-item error in a batched operation.
#[derive(Debug, serde::Serialize)]
pub struct BatchError {
    /// User-provided input identifier for this item.
    pub input: String,
    /// Human-readable error message.
    pub error: String,
}

/// Typed per-item output values for batch results.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatchOutput {
    /// Created block ID output.
    Id { input: String, id: String },
    /// Removed IDs output (subtree deletion).
    Removed { input: String, removed: Vec<String> },
    /// Boolean fold-state output.
    Collapsed { input: String, collapsed: bool },
    /// Optional block ID output for navigation operations.
    OptionalId { input: String, id: Option<String> },
    /// Lineage text output.
    Lineage { input: String, lineage: LineageContext },
    /// Show command output.
    Show { input: String, show: ShowResult },
    /// Context command output.
    Context { input: String, lineage: Vec<String>, children: ChildrenContext, friends: usize },
    /// Draft listing output.
    DraftList {
        input: String,
        expansion: Option<ExpansionDraftInfo>,
        reduction: Option<ReductionDraftInfo>,
        instruction: Option<String>,
        inquiry: Option<String>,
    },
    /// Mount information output.
    MountInfo { input: String, path: Option<String>, format: String, expanded: bool },
    /// Inline-recursive count output.
    InlinedCount { input: String, count: usize },
    /// Success marker without extra payload.
    Success { input: String },
}

/// Expansion draft data for CLI output.
///
/// Contains the planned rewrite text and child blocks for an
/// expansion draft created by `draft expand`.
#[derive(Debug, serde::Serialize)]
pub struct ExpansionDraftInfo {
    /// The rewritten text for the block (if changed).
    pub rewrite: Option<String>,
    /// Planned child block texts to add.
    pub children: Vec<String>,
}

/// Reduction draft data for CLI output.
///
/// Contains the reduction summary and marked redundant children
/// created by `draft reduce`.
#[derive(Debug, serde::Serialize)]
pub struct ReductionDraftInfo {
    /// Summary text; `None` if user rejected it (children review only).
    pub reduction: Option<String>,
    /// IDs of child blocks marked as redundant.
    pub redundant_children: Vec<String>,
}

/// Show command result: block ID, point text, and child block IDs.
#[derive(Debug, serde::Serialize)]
pub struct ShowResult {
    /// Block ID in NvG format.
    pub id: String,
    /// Point text content.
    pub text: String,
    /// Direct child block IDs.
    pub children: Vec<String>,
}

/// Search match result.
///
/// A single block matching a `find` query, containing its ID
/// and full point text.
#[derive(Debug, serde::Serialize)]
pub struct Match {
    /// Block ID in NvG format (e.g., "1v1").
    pub id: String,
    /// Full point text content of the block.
    pub text: String,
}

/// Friend block information for CLI output.
///
/// Describes a cross-reference link from one block to another,
/// including telescope settings for context expansion.
#[derive(Debug, serde::Serialize)]
pub struct FriendInfo {
    /// Friend block ID in NvG format.
    pub id: String,
    /// Perspective text (optional annotation for the link).
    pub perspective: Option<String>,
    /// Whether to telescope (expand) parent lineage when viewing.
    pub telescope_lineage: bool,
    /// Whether to telescope (expand) children when viewing.
    pub telescope_children: bool,
}
