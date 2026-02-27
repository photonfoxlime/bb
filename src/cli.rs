//! Block Store CLI: Declarative command-line interface types.
//!
//! This module defines the complete CLI API for manipulating the block store
//! via [clap](https://docs.rs/clap) derive macros. Each command, argument, and
//! variant is documented with usage patterns, examples, and error conditions.
//!
//! # Design Principles
//!
//! - **Subcommand hierarchy**: Commands are grouped by domain (tree, nav, draft,
//!   fold, friend, mount, panel) for discoverability.
//! - **Idempotency**: Commands that modify state return clear success/failure
//!   indicators. Read commands are safe and side-effect free.
//! - **Rich errors**: Each command documents its failure modes for debugging.
//!
//! # Usage
//!
//! ```ignore
//! use clap::Parser;
//! use crate::cli::BlockCli;
//!
//! let args = BlockCli::parse();
//! match args.command {
//!     Commands::Tree(cmd) => /* ... */,
//!     Commands::Nav(cmd) => /* ... */,
//!     // ...
//! }
//! ```
//!
//! # Example Invocations
//!
//! ```bash
//! # Add a child block
//! block tree add-child 0x1a2b3c "New idea"
//!
//! # Move a block after another
//! block tree move 0xsource 0xtarget --after
//!
//! # Set expansion draft
//! block draft expand 0xblock --rewrite "Refined text" --children "Child 1" "Child 2"
//!
//! # Mount a file
//! block mount set 0xblock /path/to/file.md --format markdown
//! ```

use crate::store::{
    Direction, MountFormat as StoreMountFormat, PanelBarState as StorePanelBarState,
};
use crate::store as store_module;
use clap::{Parser, ValueEnum};

// ============================================================================
// Custom Type Aliases for CLI
// ============================================================================

/// Block ID type for CLI argument parsing.
///
/// Parses the string and resolves it against the store's slotmap.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockId(pub String);

impl std::str::FromStr for BlockId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Accept with or without 0x prefix
        let hex_part = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);

        if hex_part.len() != 10 {
            return Err(format!(
                "Invalid BlockId: expected 10 hex characters after 0x, got {} ('{}')",
                hex_part.len(),
                s
            ));
        }

        // Validate hex characters
        for c in hex_part.chars() {
            if !c.is_ascii_hexdigit() {
                return Err(format!("Invalid hex character '{}' in BlockId", c));
            }
        }

        Ok(Self(s.to_string()))
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Mount Format CLI type
// ---------------------------------------------------------------------------

/// Mount format type for CLI argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MountFormatCli(pub StoreMountFormat);

impl std::str::FromStr for MountFormatCli {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            | "json" => Ok(Self(StoreMountFormat::Json)),
            | "markdown" | "md" => Ok(Self(StoreMountFormat::Markdown)),
            | _ => Err(format!("Invalid mount format: '{}'. Expected 'json' or 'markdown'.", s)),
        }
    }
}

impl std::fmt::Display for MountFormatCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            | StoreMountFormat::Json => write!(f, "json"),
            | StoreMountFormat::Markdown => write!(f, "markdown"),
        }
    }
}

impl From<MountFormatCli> for StoreMountFormat {
    fn from(f: MountFormatCli) -> Self {
        f.0
    }
}

// ---------------------------------------------------------------------------
// Panel State CLI type
// ---------------------------------------------------------------------------

/// Panel state type for CLI argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PanelBarStateCli(pub StorePanelBarState);

impl std::str::FromStr for PanelBarStateCli {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            | "friends" => Ok(Self(StorePanelBarState::Friends)),
            | "instruction" => Ok(Self(StorePanelBarState::Instruction)),
            | _ => {
                Err(format!("Invalid panel state: '{}'. Expected 'friends' or 'instruction'.", s))
            }
        }
    }
}

impl std::fmt::Display for PanelBarStateCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            | StorePanelBarState::Friends => write!(f, "friends"),
            | StorePanelBarState::Instruction => write!(f, "instruction"),
        }
    }
}

impl From<PanelBarStateCli> for StorePanelBarState {
    fn from(s: PanelBarStateCli) -> Self {
        s.0
    }
}

// ============================================================================
// Root Command
// ============================================================================

/// CLI application for manipulating the block document store.
///
/// This CLI provides programmatic access to all block store operations including
/// tree mutations, navigation, drafts, folds, friends, mounts, and panel state.
#[derive(Debug, Parser)]
#[command(name = "blooming-blockery", about, long_about)]
pub struct BlockCli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to the block store file.
    /// Defaults to `./blocks.json` in the current directory.
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<std::path::PathBuf>,

    /// Enable verbose output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format for query commands that
    /// return structured data (show, find, list, context).
    #[arg(long, global = true, value_name = "FORMAT", default_value = "table")]
    pub output: OutputFormat,
}

/// Output format for query commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// JSON output for scripting.
    Json,
    /// Table format for human readability.
    Table,
}

// ============================================================================
// Command Groups
// ============================================================================

/// Available commands.
#[derive(Debug, Parser)]
pub enum Commands {
    /// Launch the GUI (default).
    ///
    /// Opens the interactive document editor.
    ///
    /// # Example
    /// ```bash
    /// block gui
    /// blooming-blockery
    /// ```
    Gui,

    /// Generate shell completions.
    ///
    /// Outputs shell completion scripts for bash, zsh, fish, etc.
    ///
    /// # Arguments
    ///
    /// - `shell`: Target shell (bash, elvish, fish, powershell, zsh)
    ///
    /// # Example
    /// ```bash
    /// block generate-completion zsh
    /// block generate-completion bash > /etc/bash_completion.d/bb
    /// ```
    GenerateCompletion {
        /// The shell to generate completions for.
        #[arg(value_name = "SHELL")]
        shell: clap_complete::Shell,
    },

    /// Block store manipulation commands.
    ///
    /// Query and modify the block document store directly.
    ///
    /// # Example
    /// ```bash
    /// block roots
    /// block tree add-child 0x123 "New idea"
    /// block find "query"
    /// ```
    #[command(subcommand)]
    Block(BlockCommands),
}

// ============================================================================
// Block Commands Definition
// ============================================================================

/// Block store manipulation commands.
///
/// These commands provide direct access to the block store for scripting
/// and automation.
#[derive(Debug, Parser)]
pub enum BlockCommands {
    /// Query root block IDs.
    ///
    /// Returns all top-level blocks in the forest.
    Roots(RootCommand),

    /// Show detailed information about a block.
    Show(ShowCommand),

    /// Search blocks by text content.
    Find(FindCommand),

    /// Tree structure operations.
    #[command(subcommand)]
    Tree(TreeCommands),

    /// Navigation operations.
    #[command(subcommand)]
    Nav(NavCommands),

    /// Draft operations.
    #[command(subcommand)]
    Draft(DraftCommands),

    /// Fold state operations.
    #[command(subcommand)]
    Fold(FoldCommands),

    /// Friend block operations.
    #[command(subcommand)]
    Friend(FriendCommands),

    /// Mount operations.
    #[command(subcommand)]
    Mount(MountCommands),

    /// Panel state operations.
    #[command(subcommand)]
    Panel(PanelCommands),

    /// Get LLM context for a block.
    Context(ContextCommand),
}

// ============================================================================
// Query Commands
// ============================================================================

/// Query root block IDs.
#[derive(Debug, Parser)]
pub struct RootCommand {}

/// Show detailed information about a block.
#[derive(Debug, Parser)]
pub struct ShowCommand {
    /// The block ID to display.
    ///
    /// Must be a valid 10-character hex string (e.g., `0x1a2b3c4d5e`).
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: ID not found in store.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Search blocks by text content.
#[derive(Debug, Parser)]
pub struct FindCommand {
    /// Search query string.
    ///
    /// Case-insensitive substring match against all block text content.
    /// Empty string returns no results.
    ///
    /// # Example
    /// ```bash
    /// block find "TODO"
    /// block find ""  # Returns empty result
    /// ```
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Maximum number of results to return.
    ///
    /// Defaults to 100. Use `--limit 10` for minimal output.
    ///
    /// # Example
    /// ```bash
    /// block find "design" --limit 5
    /// ```
    #[arg(long, short, value_name = "N", default_value = "100")]
    pub limit: usize,
}

// ============================================================================
// Tree Commands
// ============================================================================

/// Tree structure operations.
#[derive(Debug, Parser)]
pub enum TreeCommands {
    /// Add a child block under a parent.
    ///
    /// Creates a new block with the given text as its point and appends it
    /// as the last child of the specified parent.
    ///
    /// # Arguments
    ///
    /// - `parent_id`: ID of an existing block (must not be a mount node)
    /// - `text`: Initial text content for the new block
    ///
    /// # Returns
    ///
    /// The newly created block ID.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Parent ID not found.
    /// - `InvalidOperation`: Parent is a mount node (cannot have children).
    ///
    /// # Example
    /// ```bash
    /// block tree add-child 0x1a2b3c "My new idea"
    /// # Returns: 0x9z8y7x
    /// ```
    AddChild(AddChildCommand),

    /// Add a sibling block after a given block.
    ///
    /// Creates a new block with the given text and inserts it immediately
    /// after the target block in its parent's child list (or in roots).
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block
    /// - `text`: Initial text content for the new sibling
    ///
    /// # Returns
    ///
    /// The newly created sibling block ID.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    ///
    /// # Example
    /// ```bash
    /// block tree add-sibling 0x1a2b3c "Next sibling"
    /// # Returns: 0x7w6v5u
    /// ```
    AddSibling(AddSiblingCommand),

    /// Wrap a block with a new parent.
    ///
    /// Inserts a new parent block at the target block's current position,
    /// making the target the first child of the new parent.
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block (the child to wrap)
    /// - `text`: Initial text content for the new parent
    ///
    /// # Returns
    ///
    /// The newly created parent block ID.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    ///
    /// # Example
    /// ```bash
    /// block tree wrap 0x1a2b3c "New parent section"
    /// # Returns: 0x4t3s2r
    /// # Before: root -> [0x1a2b3c]
    /// # After:  root -> [0x4t3s2r] -> [0x1a2b3c]
    /// ```
    Wrap(WrapCommand),

    /// Duplicate a subtree.
    ///
    /// Deep-clones the source block and its entire subtree, inserting the
    /// copy immediately after the original.
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block to duplicate
    ///
    /// # Returns
    ///
    /// The root ID of the cloned subtree.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    ///
    /// # Example
    /// ```bash
    /// block tree duplicate 0x1a2b3c
    /// # Returns: 0x1q2w3e
    /// ```
    Duplicate(DuplicateCommand),

    /// Delete a subtree.
    ///
    /// Removes the block and all its descendants. Cleans up all associated
    /// metadata: drafts, friend references, panel state, and mount origins.
    ///
    /// If the deletion empties the root list, a single empty root is created.
    ///
    /// # Arguments
    ///
    /// - `block_id`: ID of an existing block to delete
    ///
    /// # Returns
    ///
    /// List of all removed block IDs (including descendants).
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block ID not found.
    /// - `InvalidOperation`: Attempting to delete the last root (allowed but
    ///   results in a new empty root).
    ///
    /// # Example
    /// ```bash
    /// block tree delete 0x1a2b3c
    /// # Returns: {"removed":["0x1a2b3c","0x4d5e6f","0x7g8h9i"]}
    /// ```
    Delete(DeleteCommand),

    /// Move a block relative to a target.
    ///
    /// Repositions the source block to be before, after, or under the target.
    /// The source block (and its subtree) retains its internal structure.
    ///
    /// # Arguments
    ///
    /// - `source_id`: Block to move
    /// - `target_id`: Reference block for positioning
    /// - `--before`, `--after`, `--under`: Positioning direction
    ///
    /// # Constraints
    ///
    /// - Source and target must be different blocks.
    /// - Source must not be an ancestor of target (would create cycle).
    /// - `--under` requires target is not a mount node.
    ///
    /// # Returns
    ///
    /// Success indicator (or error with reason).
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Either ID not found.
    /// - `InvalidOperation`: Source is ancestor of target (cycle).
    /// - `InvalidOperation`: `--under` on mount node.
    ///
    /// # Example
    /// ```bash
    /// block tree move 0xsource 0xtarget --before
    /// block tree move 0xsource 0xtarget --after
    /// block tree move 0xsource 0xtarget --under
    /// ```
    Move(MoveCommand),
}

/// Add a child block under a parent.
#[derive(Debug, Parser)]
pub struct AddChildCommand {
    /// Parent block ID.
    ///
    /// Must be an existing block that is not a mount node.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Parent not found.
    /// - `InvalidOperation`: Parent is a mount.
    #[arg(value_name = "PARENT_ID")]
    pub parent_id: BlockId,

    /// Initial text content for the new child block.
    ///
    /// Can be any string, including empty string.
    ///
    /// # Example
    /// ```bash
    /// block tree add-child 0x123 "My new idea"
    /// block tree add-child 0x123 ""  # Empty text
    /// ```
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Add a sibling block after a given block.
#[derive(Debug, Parser)]
pub struct AddSiblingCommand {
    /// Block to add sibling after.
    ///
    /// Can be a root block or a child block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Initial text content for the new sibling.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Wrap a block with a new parent.
#[derive(Debug, Parser)]
pub struct WrapCommand {
    /// Block to wrap (becomes first child of new parent).
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Initial text content for the new parent.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

/// Duplicate a subtree.
#[derive(Debug, Parser)]
pub struct DuplicateCommand {
    /// Block to duplicate (with all descendants).
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Delete a subtree.
#[derive(Debug, Parser)]
pub struct DeleteCommand {
    /// Block to delete (with all descendants).
    ///
    /// # Safety Note
    ///
    /// Deleting a block also removes all friend references TO that block
    /// from other blocks, and cleans up drafts/panel state.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Move a block relative to a target.
#[derive(Debug, Parser)]
pub struct MoveCommand {
    /// Block to move.
    ///
    /// The entire subtree moves with this block.
    #[arg(value_name = "SOURCE_ID")]
    pub source_id: BlockId,

    /// Target block for positioning.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,

    /// Move source to be immediately before target.
    ///
    /// Source becomes the previous sibling of target.
    ///
    /// # Example
    /// ```bash
    /// block tree move 0xsrc 0xtgt --before
    /// # Before: [..., 0xsrc, ..., 0xtgt, ...]
    /// # After:  [..., 0xtgt, 0xsrc, ...]
    /// ```
    #[arg(long, group = "direction")]
    pub before: bool,

    /// Move source to be immediately after target.
    ///
    /// Source becomes the next sibling of target.
    ///
    /// # Example
    /// ```bash
    /// block tree move 0xsrc 0xtgt --after
    /// # Before: [..., 0xtgt, ..., 0xsrc, ...]
    /// # After:  [..., 0xtgt, 0xsrc, ...]
    /// ```
    #[arg(long, group = "direction")]
    pub after: bool,

    /// Move source to be the last child of target.
    ///
    /// Target must not be a mount node.
    ///
    /// # Example
    /// ```bash
    /// block tree move 0xsrc 0xtgt --under
    /// # Before: 0xtgt -> []
    /// # After:  0xtgt -> [0xsrc]
    /// ```
    #[arg(long, group = "direction")]
    pub under: bool,
}

// ============================================================================
// Navigation Commands
// ============================================================================

/// Navigation operations.
#[derive(Debug, Parser)]
pub enum NavCommands {
    /// Get the next visible block in DFS order.
    ///
    /// Traverses depth-first, descending into uncollapsed blocks and skipping
    /// collapsed subtrees. Returns `null` if at the last visible block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Current block position
    ///
    /// # Returns
    ///
    /// Next visible block ID, or null if at end.
    ///
    /// # Example
    /// ```bash
    /// block nav next 0x1a2b3c
    /// # Output: 0x4d5e6f
    /// ```
    Next(NextCommand),

    /// Get the previous visible block in DFS order.
    ///
    /// Traverses backward, descending into deepest visible descendants of
    /// previous siblings. Returns `null` if at the first visible block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Current block position
    ///
    /// # Returns
    ///
    /// Previous visible block ID, or null if at start.
    ///
    /// # Example
    /// ```bash
    /// block nav prev 0x4d5e6f
    /// # Output: 0x1a2b3c
    /// ```
    Prev(PrevCommand),

    /// Get the lineage (ancestor chain) for a block.
    ///
    /// Returns all ancestor block texts from root to the target (exclusive of
    /// target's own text).
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Returns
    ///
    /// Vector of ancestor block texts in order (root → parent).
    ///
    /// # Example
    /// ```bash
    /// block nav lineage 0xdeep
    /// # Output: ["Root", "Section", "Subsection"]
    /// ```
    Lineage(LineageCommand),
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

// ============================================================================
// Draft Commands
// ============================================================================

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
    /// block draft expand 0x123 --rewrite "Refined version" \
    ///     --children "Proposed child 1" "Proposed child 2"
    ///
    /// # Set children only (no rewrite)
    /// block draft expand 0x123 --children "Just kids"
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
    /// block draft reduce 0x123 \
    ///     --reduction "All the things" \
    ///     --redundant-children 0x456 0x789
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
    /// block draft instruction 0x123 --text "Make this more concise"
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
    /// block draft inquiry 0x123 --response "The key insight is..."
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
    /// block draft list 0x123
    /// block draft list 0x123 --output json
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
    /// block draft clear 0x123 --all
    /// block draft clear 0x123 --expand
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

// ============================================================================
// Fold Commands
// ============================================================================

/// Fold (collapse) state operations.
#[derive(Debug, Parser)]
pub enum FoldCommands {
    /// Toggle the fold state of a block.
    ///
    /// If collapsed, expands to show children. If expanded, collapses to hide.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block (must have children to be collapsible)
    ///
    /// # Returns
    ///
    /// New fold state: `true` = collapsed, `false` = expanded.
    ///
    /// # Example
    /// ```bash
    /// block fold toggle 0x123
    /// # Output: {"collapsed": true}
    /// ```
    Toggle(ToggleFoldCommand),

    /// Get the fold state of a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Returns
    ///
    /// `true` if collapsed, `false` if expanded.
    ///
    /// # Example
    /// ```bash
    /// block fold status 0x123
    /// # Output: {"collapsed": false}
    /// ```
    Status(StatusFoldCommand),
}

/// Toggle fold state.
#[derive(Debug, Parser)]
pub struct ToggleFoldCommand {
    /// Block to toggle.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Get fold status.
#[derive(Debug, Parser)]
pub struct StatusFoldCommand {
    /// Block to query.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

// ============================================================================
// Friend Commands
// ============================================================================

/// Friend block operations.
#[derive(Debug, Parser)]
pub enum FriendCommands {
    /// Add a friend block.
    ///
    /// Friend blocks are extra context blocks included in LLM requests for
    /// the target block. They are not children but related blocks with
    /// optional perspective framing.
    ///
    /// # Arguments
    ///
    /// - `target_id`: Block that "has" the friends
    /// - `friend_id`: Block to add as a friend
    /// - `--perspective`: Optional framing text for how to interpret the friend
    /// - `--telescope-lineage`: Include friend's parent lineage in context
    /// - `--telescope-children`: Include friend's children in context
    ///
    /// # Returns
    ///
    /// Success indicator.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Either ID not found.
    /// - `InvalidOperation`: Adding self as friend.
    ///
    /// # Example
    /// ```bash
    /// block friend add 0x123 0x456 --perspective "Related design"
    /// block friend add 0x123 0x789 --telescope-lineage --telescope-children
    /// ```
    Add(AddFriendCommand),

    /// Remove a friend block.
    ///
    /// # Arguments
    ///
    /// - `target_id`: Block that has the friend
    /// - `friend_id`: Friend to remove
    ///
    /// # Example
    /// ```bash
    /// block friend remove 0x123 0x456
    /// ```
    Remove(RemoveFriendCommand),

    /// List friend blocks for a target.
    ///
    /// # Arguments
    ///
    /// - `target_id`: Block to query
    ///
    /// # Example
    /// ```bash
    /// block friend list 0x123
    /// block friend list 0x123 --output json
    /// ```
    List(ListFriendCommand),
}

/// Add a friend block.
#[derive(Debug, Parser)]
pub struct AddFriendCommand {
    /// Target block that will have the friend.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,

    /// Block to add as a friend.
    #[arg(value_name = "FRIEND_ID")]
    pub friend_id: BlockId,

    /// Optional framing text for interpreting this friend.
    ///
    /// Describes how the target should view this friend block.
    #[arg(long, value_name = "TEXT")]
    pub perspective: Option<String>,

    /// Include friend's parent lineage in LLM context.
    ///
    /// When enabled, the friend's full ancestry (root to parent) is included.
    #[arg(long)]
    pub telescope_lineage: bool,

    /// Include friend's children in LLM context.
    ///
    /// When enabled, the friend's direct children text is included.
    #[arg(long)]
    pub telescope_children: bool,
}

/// Remove a friend block.
#[derive(Debug, Parser)]
pub struct RemoveFriendCommand {
    /// Target block.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,

    /// Friend to remove.
    #[arg(value_name = "FRIEND_ID")]
    pub friend_id: BlockId,
}

/// List friend blocks.
#[derive(Debug, Parser)]
pub struct ListFriendCommand {
    /// Target block to query.
    #[arg(value_name = "TARGET_ID")]
    pub target_id: BlockId,
}

// ============================================================================
// Mount Commands
// ============================================================================

/// Mount operations (external file integration).
#[derive(Debug, Parser)]
pub enum MountCommands {
    /// Set a mount path on a block.
    ///
    /// Converts a leaf block into a mount point referencing an external file.
    /// The block must have no children.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block (must be leaf)
    /// - `path`: Path to external block store file
    /// - `--format`: File format (json or markdown, default: json)
    ///
    /// # Returns
    ///
    /// Success indicator.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block not found.
    /// - `InvalidOperation`: Block has children (cannot mount).
    ///
    /// # Example
    /// ```bash
    /// block mount set 0x123 /data/external.json
    /// block mount set 0x123 /notes/notes.md --format markdown
    /// ```
    Set(SetMountCommand),

    /// Expand a mount point.
    ///
    /// Loads the external file, re-keys its blocks into the main store, and
    /// replaces the mount node with inline children.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Mount point block
    /// - `--base-dir`: Base directory for resolving relative paths
    ///
    /// # Returns
    ///
    /// Root IDs of the loaded subtree.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block not found.
    /// - `InvalidOperation`: Block is not a mount.
    /// - `IoError`: Failed to read or parse mount file.
    ///
    /// # Example
    /// ```bash
    /// block mount expand 0x123
    /// block mount expand 0x123 --base-dir /projects/app
    /// ```
    Expand(ExpandMountCommand),

    /// Collapse an expanded mount.
    ///
    /// Removes the loaded blocks, restores the mount node with its original path.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Expanded mount point
    ///
    /// # Returns
    ///
    /// Success indicator.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block not found.
    /// - `InvalidOperation`: Block is not an expanded mount.
    ///
    /// # Example
    /// ```bash
    /// block mount collapse 0x123
    /// ```
    Collapse(CollapseMountCommand),

    /// Extract a subtree to an external file.
    ///
    /// Saves the block's children (and their subtrees) to a file, then replaces
    /// the block with a mount node pointing to that file.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Block to extract
    /// - `--output`: Output file path
    /// - `--base-dir`: Base directory for relative path computation
    ///
    /// # Returns
    ///
    /// Success indicator.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: Block not found.
    /// - `InvalidOperation`: Block has no children.
    /// - `IoError`: Failed to write file.
    ///
    /// # Example
    /// ```bash
    /// block mount extract 0x123 --output /backup/notes.json
    /// block mount extract 0x123 --output notes.md --format markdown
    /// ```
    Extract(ExtractMountCommand),

    /// Show mount information for a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Block to query
    ///
    /// # Example
    /// ```bash
    /// block mount info 0x123
    /// ```
    Info(InfoMountCommand),
}

/// Set mount path.
#[derive(Debug, Parser)]
pub struct SetMountCommand {
    /// Block to convert to mount (must be leaf).
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Path to external file.
    ///
    /// Can be relative or absolute. Relative paths are resolved against
    /// the store's base directory.
    #[arg(value_name = "PATH")]
    pub path: std::path::PathBuf,

    /// Mount file format.
    ///
    /// - `json`: Full block store JSON format (default)
    /// - `markdown`: Markdown Mount v1 format
    #[arg(long, value_name = "FORMAT", default_value = "json")]
    pub format: MountFormatCli,
}

/// Expand mount.
#[derive(Debug, Parser)]
pub struct ExpandMountCommand {
    /// Mount point block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Collapse mount.
#[derive(Debug, Parser)]
pub struct CollapseMountCommand {
    /// Expanded mount point.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Extract subtree to file.
#[derive(Debug, Parser)]
pub struct ExtractMountCommand {
    /// Block to extract (becomes mount).
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Output file path.
    #[arg(long, short, value_name = "PATH")]
    pub output: std::path::PathBuf,

    /// Output format (inferred from extension if not specified).
    #[arg(long, value_name = "FORMAT")]
    pub format: Option<MountFormatCli>,
}

/// Show mount info.
#[derive(Debug, Parser)]
pub struct InfoMountCommand {
    /// Block to query.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

// ============================================================================
// Panel Commands
// ============================================================================

/// Panel state operations.
#[derive(Debug, Parser)]
pub enum PanelCommands {
    /// Set the panel state for a block.
    ///
    /// Persists which panel (Friends or Instruction) is open for a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    /// - `panel`: Panel name (friends or instruction)
    ///
    /// # Example
    /// ```bash
    /// block panel set 0x123 friends
    /// block panel set 0x123 instruction
    /// ```
    Set(SetPanelCommand),

    /// Get the panel state for a block.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Example
    /// ```bash
    /// block panel get 0x123
    /// # Output: {"panel": "friends"}
    /// ```
    Get(GetPanelCommand),

    /// Clear the panel state.
    ///
    /// # Arguments
    ///
    /// - `block_id`: Target block
    ///
    /// # Example
    /// ```bash
    /// block panel clear 0x123
    /// ```
    Clear(ClearPanelCommand),
}

/// Set panel state.
#[derive(Debug, Parser)]
pub struct SetPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// Panel to show.
    ///
    /// - `friends`: Show friends panel
    /// - `instruction`: Show instruction editor
    #[arg(value_name = "PANEL")]
    pub panel: PanelBarStateCli,
}

/// Get panel state.
#[derive(Debug, Parser)]
pub struct GetPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Clear panel state.
#[derive(Debug, Parser)]
pub struct ClearPanelCommand {
    /// Target block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

// ============================================================================
// Context Command
// ============================================================================

/// Get LLM context for a block.
#[derive(Debug, Parser)]
pub struct ContextCommand {
    /// Target block.
    ///
    /// The context includes:
    /// - Lineage: ancestor block texts (root to parent)
    /// - Children: direct children's text
    /// - Friends: friend block info with perspectives
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

// ============================================================================
// BlockCommands Execution
// ============================================================================

impl BlockCommands {
    /// Execute a block command with the given store.
    ///
    /// This method handles all block manipulation commands, operating on the
    /// provided store and returning the modified store (or the same one if
    /// no changes were made).
    ///
    /// # Arguments
    ///
    /// - `store`: The block store to operate on
    /// - `base_dir`: Base directory for resolving relative mount paths
    /// - `output`: Output format for query results
    ///
    /// # Returns
    ///
    /// Modified store (or original if no changes) and command result.
    pub fn execute(
        self,
        mut store: crate::store::BlockStore,
        base_dir: &std::path::Path,
        output: OutputFormat,
    ) -> (crate::store::BlockStore, CliResult) {
        match self {
            // Query commands - no store modification
            BlockCommands::Roots(RootCommand {}) => {
                let roots: Vec<String> = store
                    .roots()
                    .iter()
                    .map(|id| format!("{:?}", id))
                    .collect();
                (store, CliResult::Roots(roots))
            }
            BlockCommands::Show(cmd) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(id) => {
                        let text = store.point(&id).unwrap_or_default();
                        let children: Vec<String> = store
                            .children(&id)
                            .iter()
                            .map(|c| format!("{:?}", c))
                            .collect();
                        (store, CliResult::Show { id, text, children })
                    }
                }
            }
            BlockCommands::Find(cmd) => {
                let matches: Vec<Match> = store
                    .roots()
                    .iter()
                    .flat_map(|root| Self::find_in_subtree(&store, root, &cmd.query))
                    .take(cmd.limit)
                    .collect();
                (store, CliResult::Find(matches))
            }
            // Tree commands
            BlockCommands::Tree(TreeCommands::AddChild(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.parent_id);
                match id {
                    None => (store, CliResult::Error("Unknown parent block ID".to_string())),
                    Some(parent_id) => {
                        let new_id = store.append_child(&parent_id, cmd.text.clone());
                        match new_id {
                            Some(new_id) => (store, CliResult::BlockId(new_id)),
                            None => (store, CliResult::Error("Failed to add child (parent may be a mount)".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Tree(TreeCommands::AddSibling(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let new_id = store.append_sibling(&block_id, cmd.text.clone());
                        match new_id {
                            Some(new_id) => (store, CliResult::BlockId(new_id)),
                            None => (store, CliResult::Error("Failed to add sibling".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Tree(TreeCommands::Wrap(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let new_id = store.insert_parent(&block_id, cmd.text.clone());
                        match new_id {
                            Some(new_id) => (store, CliResult::BlockId(new_id)),
                            None => (store, CliResult::Error("Failed to wrap block".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Tree(TreeCommands::Duplicate(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let new_id = store.duplicate_subtree_after(&block_id);
                        match new_id {
                            Some(new_id) => (store, CliResult::BlockId(new_id)),
                            None => (store, CliResult::Error("Failed to duplicate".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Tree(TreeCommands::Delete(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let removed = store.remove_block_subtree(&block_id);
                        match removed {
                            Some(ids) => {
                                let ids_str: Vec<String> = ids.iter().map(|i| format!("{:?}", i)).collect();
                                (store, CliResult::Removed(ids_str))
                            }
                            None => (store, CliResult::Error("Failed to delete".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Tree(TreeCommands::Move(cmd)) => {
                let source = Self::resolve_block_id(&store, &cmd.source_id);
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                match (source, target) {
                    (Some(src), Some(tgt)) => {
                        let dir = if cmd.before {
                            crate::store::Direction::Before
                        } else if cmd.after {
                            crate::store::Direction::After
                        } else {
                            crate::store::Direction::Under
                        };
                        let result = store.move_block(&src, &tgt, dir);
                        match result {
                            Some(()) => (store, CliResult::Success),
                            None => (store, CliResult::Error("Move failed (check constraints)".to_string())),
                        }
                    }
                    _ => (store, CliResult::Error("Unknown source or target block ID".to_string())),
                }
            }
            // Navigation commands
            BlockCommands::Nav(NavCommands::Next(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let next = store.next_visible_in_dfs(&block_id);
                        (store, CliResult::OptionalBlockId(next))
                    }
                }
            }
            BlockCommands::Nav(NavCommands::Prev(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let prev = store.prev_visible_in_dfs(&block_id);
                        (store, CliResult::OptionalBlockId(prev))
                    }
                }
            }
            BlockCommands::Nav(NavCommands::Lineage(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let lineage = store.lineage_points_for_id(&block_id);
                        let points: Vec<String> = lineage.points().map(String::from).collect();
                        (store, CliResult::Lineage(points))
                    }
                }
            }
            // Draft commands
            BlockCommands::Draft(DraftCommands::Expand(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let draft = store_module::ExpansionDraftRecord {
                            rewrite: cmd.rewrite,
                            children: cmd.children,
                        };
                        store.insert_expansion_draft(block_id, draft);
                        (store, CliResult::Success)
                    }
                }
            }
            BlockCommands::Draft(DraftCommands::Reduce(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let redundant: Vec<_> = cmd.redundant_children
                            .iter()
                            .filter_map(|c| Self::resolve_block_id(&store, c))
                            .collect();
                        let draft = store_module::ReductionDraftRecord {
                            reduction: cmd.reduction,
                            redundant_children: redundant,
                        };
                        store.insert_reduction_draft(block_id, draft);
                        (store, CliResult::Success)
                    }
                }
            }
            BlockCommands::Draft(DraftCommands::Instruction(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        store.set_instruction_draft(block_id, cmd.text);
                        (store, CliResult::Success)
                    }
                }
            }
            BlockCommands::Draft(DraftCommands::Inquiry(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        store.set_inquiry_draft(block_id, cmd.response);
                        (store, CliResult::Success)
                    }
                }
            }
            BlockCommands::Draft(DraftCommands::List(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let expansion = store.expansion_draft(&block_id).map(|d| ExpansionDraftInfo {
                            rewrite: d.rewrite.clone(),
                            children: d.children.clone(),
                        });
                        let reduction = store.reduction_draft(&block_id).map(|d| ReductionDraftInfo {
                            reduction: d.reduction.clone(),
                            redundant_children: d.redundant_children.iter().map(|id| format!("{:?}", id)).collect(),
                        });
                        let instruction = store.instruction_draft(&block_id).map(|d| d.instruction.clone());
                        let inquiry = store.inquiry_draft(&block_id).map(|d| d.response.clone());
                        (store, CliResult::DraftList { expansion, reduction, instruction, inquiry })
                    }
                }
            }
            BlockCommands::Draft(DraftCommands::Clear(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
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
            }
            // Fold commands
            BlockCommands::Fold(FoldCommands::Toggle(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let collapsed = store.toggle_collapsed(&block_id);
                        (store, CliResult::Collapsed(collapsed))
                    }
                }
            }
            BlockCommands::Fold(FoldCommands::Status(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let collapsed = store.is_collapsed(&block_id);
                        (store, CliResult::Collapsed(collapsed))
                    }
                }
            }
            // Friend commands
            BlockCommands::Friend(FriendCommands::Add(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                let friend = Self::resolve_block_id(&store, &cmd.friend_id);
                match (target, friend) {
                    (Some(tid), Some(fid)) => {
                        let mut friends = store.friend_blocks_for(&tid).to_vec();
                        friends.push(store_module::FriendBlock {
                            block_id: fid,
                            perspective: cmd.perspective,
                            parent_lineage_telescope: cmd.telescope_lineage,
                            children_telescope: cmd.telescope_children,
                        });
                        store.set_friend_blocks_for(&tid, friends);
                        (store, CliResult::Success)
                    }
                    _ => (store, CliResult::Error("Unknown block ID".to_string())),
                }
            }
            BlockCommands::Friend(FriendCommands::Remove(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                let friend = Self::resolve_block_id(&store, &cmd.friend_id);
                match (target, friend) {
                    (Some(tid), Some(fid)) => {
                        let mut friends = store.friend_blocks_for(&tid).to_vec();
                        friends.retain(|f| f.block_id != fid);
                        store.set_friend_blocks_for(&tid, friends);
                        (store, CliResult::Success)
                    }
                    _ => (store, CliResult::Error("Unknown block ID".to_string())),
                }
            }
            BlockCommands::Friend(FriendCommands::List(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                match target {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(tid) => {
                        let friends: Vec<FriendInfo> = store.friend_blocks_for(&tid)
                            .iter()
                            .map(|f| FriendInfo {
                                id: format!("{:?}", f.block_id),
                                perspective: f.perspective.clone(),
                                telescope_lineage: f.parent_lineage_telescope,
                                telescope_children: f.children_telescope,
                            })
                            .collect();
                        (store, CliResult::FriendList(friends))
                    }
                }
            }
            // Mount commands
            BlockCommands::Mount(MountCommands::Set(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let result = store.set_mount_path_with_format(&block_id, cmd.path, cmd.format.into());
                        match result {
                            Some(()) => (store, CliResult::Success),
                            None => (store, CliResult::Error("Failed to set mount path (block may have children)".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Mount(MountCommands::Expand(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        match store.expand_mount(&block_id, base_dir) {
                            Ok(_) => (store, CliResult::Success),
                            Err(e) => (store, CliResult::Error(format!("Expand failed: {}", e))),
                        }
                    }
                }
            }
            BlockCommands::Mount(MountCommands::Collapse(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        match store.collapse_mount(&block_id) {
                            Some(()) => (store, CliResult::Success),
                            None => (store, CliResult::Error("Block is not an expanded mount".to_string())),
                        }
                    }
                }
            }
            BlockCommands::Mount(MountCommands::Extract(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        match store.save_subtree_to_file(&block_id, &cmd.output, base_dir) {
                            Ok(()) => (store, CliResult::Success),
                            Err(e) => (store, CliResult::Error(format!("Extract failed: {}", e))),
                        }
                    }
                }
            }
            BlockCommands::Mount(MountCommands::Info(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let node = store.node(&block_id);
                        let mount_entry = store.mount_table().entry(block_id);
                        let result = match (node, mount_entry) {
                            (Some(store_module::BlockNode::Mount { path, format }), None) => {
                                CliResult::MountInfo {
                                    path: Some(path.display().to_string()),
                                    format: format!("{:?}", format),
                                    expanded: false,
                                }
                            }
                            (_, Some(entry)) => {
                                CliResult::MountInfo {
                                    path: Some(entry.path.display().to_string()),
                                    format: format!("{:?}", entry.format),
                                    expanded: true,
                                }
                            }
                            _ => CliResult::Error("Block is not a mount".to_string()),
                        };
                        (store, result)
                    }
                }
            }
            // Panel commands
            BlockCommands::Panel(PanelCommands::Set(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        store.set_panel_state(&block_id, Some(cmd.panel.into()));
                        (store, CliResult::Success)
                    }
                }
            }
            BlockCommands::Panel(PanelCommands::Get(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let state = store.panel_state(&block_id).map(|s| match s {
                            store_module::PanelBarState::Friends => "friends",
                            store_module::PanelBarState::Instruction => "instruction",
                        });
                        (store, CliResult::PanelState(state.map(String::from)))
                    }
                }
            }
            BlockCommands::Panel(PanelCommands::Clear(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        store.set_panel_state(&block_id, None);
                        (store, CliResult::Success)
                    }
                }
            }
            // Context command
            BlockCommands::Context(cmd) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    None => (store, CliResult::Error("Unknown block ID".to_string())),
                    Some(block_id) => {
                        let context = store.block_context_for_id(&block_id);
                        let lineage: Vec<String> = context.lineage.points().map(String::from).collect();
                        let children = context.existing_children;
                        let friends = context.friend_blocks.len();
                        (store, CliResult::Context { lineage, children, friends })
                    }
                }
            }
        }
    }

    /// Resolve a CLI BlockId to an actual store BlockId.
    ///
    /// This is a placeholder - in the real implementation, we'd need to
    /// search the slotmap for the matching ID.
    fn resolve_block_id(
        store: &crate::store::BlockStore,
        cli_id: &BlockId,
    ) -> Option<crate::store::BlockId> {
        // TODO: Implement proper ID resolution - for now, iterate and match
        // This is inefficient but works for small stores
        let cli_str = cli_id.0.strip_prefix("0x").unwrap_or(&cli_id.0);
        for (id, _) in &store.nodes {
            let id_str = format!("{:?}", id);
            let id_str = id_str.strip_prefix("0x").unwrap_or(&id_str);
            if id_str.eq_ignore_ascii_case(cli_str) {
                return Some(id);
            }
        }
        None
    }

    /// Find all blocks matching a query in their text content.
    fn find_in_subtree(
        store: &crate::store::BlockStore,
        root: &crate::store::BlockId,
        query: &str,
    ) -> Vec<Match> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        Self::find_recursive(store, root, &query_lower, &mut results);
        results
    }

    fn find_recursive(
        store: &crate::store::BlockStore,
        id: &crate::store::BlockId,
        query: &str,
        results: &mut Vec<Match>,
    ) {
        if let Some(text) = store.point(id) {
            if text.to_lowercase().contains(query) {
                results.push(Match {
                    id: format!("{:?}", id),
                    text,
                });
            }
        }
        for child in store.children(id) {
            Self::find_recursive(store, child, query, results);
        }
    }
}

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
    Show { id: crate::store::BlockId, text: String, children: Vec<String> },
    /// Search results.
    Find(Vec<Match>),
    /// A single block ID (e.g., from create operations).
    BlockId(crate::store::BlockId),
    /// Optional block ID (e.g., from navigation).
    OptionalBlockId(Option<crate::store::BlockId>),
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
    MountInfo {
        path: Option<String>,
        format: String,
        expanded: bool,
    },
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
