//! CLI command results and output types.
//!
//! This module defines the result types returned by `BlockCommands::execute()`.
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
//! BlockCommands::execute() -> CliResult
//!     └─> print_result(&CliResult, OutputFormat)
//!         ├─> JSON: serde_json::json!() for all variants
//!         └─> Table: Human-readable formatting per variant
//! ```

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
    Show { id: BlockId, text: String, children: Vec<String> },
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
    Lineage(Vec<String>),
    /// LLM context information.
    ///
    /// Returned by the `context` command, providing the data
    /// that would be sent to an LLM for this block.
    Context { lineage: Vec<String>, children: Vec<String>, friends: usize },
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
    /// Panel sidebar state.
    ///
    /// Returned by `panel get`, showing the current sidebar mode.
    PanelState(Option<String>),
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
    /// Summary text describing what was reduced.
    pub reduction: String,
    /// IDs of child blocks marked as redundant.
    pub redundant_children: Vec<String>,
}

/// Search match result.
///
/// A single block matching a `find` query, containing its ID
/// and full point text.
#[derive(Debug, serde::Serialize)]
pub struct Match {
    /// Block ID in hex format (e.g., "1v1b3c4d5e").
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
    /// Friend block ID in hex format.
    pub id: String,
    /// Perspective text (optional annotation for the link).
    pub perspective: Option<String>,
    /// Whether to telescope (expand) parent lineage when viewing.
    pub telescope_lineage: bool,
    /// Whether to telescope (expand) children when viewing.
    pub telescope_children: bool,
}
