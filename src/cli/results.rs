//! CLI command results and output types.

use crate::store::BlockId;

/// CLI command result.
#[derive(Debug)]
pub enum CliResult {
    /// Command succeeded with no output.
    Success,
    /// Command failed with an error message.
    Error(String),
    /// List of root block IDs.
    Roots(Vec<String>),
    /// Show block details.
    Show { id: BlockId, text: String, children: Vec<String> },
    /// Search results.
    Find(Vec<Match>),
    /// A single block ID (e.g., from create operations).
    BlockId(BlockId),
    /// Optional block ID (e.g., from navigation).
    OptionalBlockId(Option<BlockId>),
    /// Removed block IDs.
    Removed(Vec<String>),
    /// Collapsed state.
    Collapsed(bool),
    /// Lineage points.
    Lineage(Vec<String>),
    /// LLM context.
    Context { lineage: Vec<String>, children: Vec<String>, friends: usize },
    /// Draft listing result.
    DraftList {
        expansion: Option<ExpansionDraftInfo>,
        reduction: Option<ReductionDraftInfo>,
        instruction: Option<String>,
        inquiry: Option<String>,
    },
    /// Friend list result.
    FriendList(Vec<FriendInfo>),
    /// Mount info result.
    MountInfo { path: Option<String>, format: String, expanded: bool },
    /// Panel state result.
    PanelState(Option<String>),
}

/// Expansion draft info for CLI output.
#[derive(Debug, serde::Serialize)]
pub struct ExpansionDraftInfo {
    pub rewrite: Option<String>,
    pub children: Vec<String>,
}

/// Reduction draft info for CLI output.
#[derive(Debug, serde::Serialize)]
pub struct ReductionDraftInfo {
    pub reduction: String,
    pub redundant_children: Vec<String>,
}

/// Search match result.
#[derive(Debug, serde::Serialize)]
pub struct Match {
    /// Block ID.
    pub id: String,
    /// Block text content.
    pub text: String,
}

/// Friend info for CLI output.
#[derive(Debug, serde::Serialize)]
pub struct FriendInfo {
    pub id: String,
    pub perspective: Option<String>,
    pub telescope_lineage: bool,
    pub telescope_children: bool,
}
