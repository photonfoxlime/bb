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
use clap::{Parser, ValueEnum};

// ============================================================================
// Custom Type Aliases for CLI
// ============================================================================

/// Block ID type for CLI argument parsing.
///
/// Accepts strings in the format `0x1a2b3c4d5e` (10 hex chars after 0x).
/// In the actual implementation, this will parse the string and resolve
/// against the store's slotmap.
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
///
/// # Exit Codes
///
/// - `0`: Success
/// - `1`: Usage error (invalid arguments)
/// - `2`: Store error (block not found, invalid operation, I/O failure)
/// - `3`: Internal error (assertion failure, panic)
///
/// # Environment
///
/// - `BLOCK_STORE_PATH`: Path to the block store file (default: `./blocks.json`)
/// - `BLOCK_BASE_DIR`: Base directory for resolving relative mount paths
#[derive(Debug, Parser)]
#[command(name = "block")]
#[command(version = "0.0.2")]
#[command(about = "Block document store CLI", long_about = None)]
pub struct BlockCli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to the block store file.
    ///
    /// Defaults to `./blocks.json` in the current directory. The file is created
    /// if it does not exist.
    ///
    /// # Example
    /// ```bash
    /// block --store /data/my-blocks.json roots
    /// ```
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<std::path::PathBuf>,

    /// Base directory for resolving relative mount paths.
    ///
    /// Defaults to the directory containing the store file.
    ///
    /// # Example
    /// ```bash
    /// block --base-dir /projects/myapp mount expand 0x123 --base-dir /data
    /// ```
    #[arg(long, global = true, value_name = "DIR")]
    pub base_dir: Option<std::path::PathBuf>,

    /// Enable verbose output.
    ///
    /// When present, commands print debug information including internal IDs,
    /// operation timing, and detailed error context.
    ///
    /// # Example
    /// ```bash
    /// block -v tree add-child 0x123 "test"
    /// block --verbose tree add-child 0x123 "test"
    /// ```
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format for query commands.
    ///
    /// Affects commands that return structured data (show, find, list, context).
    ///
    /// - `json`: Machine-readable JSON output
    /// - `table`: Human-readable table format (default)
    /// - `plain`: Minimal text, one line per item
    #[arg(long, global = true, value_name = "FORMAT", default_value = "table")]
    pub output: OutputFormat,
}

/// Output format for query commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// JSON output for scripting.
    json,
    /// Table format for human readability.
    table,
    /// Minimal plain text.
    plain,
}

// ============================================================================
// Command Groups
// ============================================================================

/// Available commands for block manipulation.
#[derive(Debug, Parser)]
pub enum Commands {
    /// Query root block IDs.
    ///
    /// Returns all top-level blocks in the forest. The store always has at least
    /// one root.
    ///
    /// # Example
    /// ```bash
    /// block roots
    /// # Output:
    /// # [ "0x1a2b3c", "0x4d5e6f" ]
    ///
    /// block roots --output json
    /// # Output:
    /// # {"roots":["0x1a2b3c","0x4d5e6f"]}
    /// ```
    Roots(RootCommand),

    /// Show detailed information about a block.
    ///
    /// Displays the block's text content, children, mount info, and metadata.
    ///
    /// # Errors
    ///
    /// - `UnknownBlock`: The provided ID does not exist in the store.
    ///
    /// # Example
    /// ```bash
    /// block show 0x1a2b3c
    /// block show 0x1a2b3c --output json
    /// ```
    Show(ShowCommand),

    /// Search blocks by text content.
    ///
    /// Performs a substring match across all block text content. Search is
    /// case-insensitive.
    ///
    /// # Example
    /// ```bash
    /// block find "Notebook"
    /// block find "design" --output json
    /// ```
    Find(FindCommand),

    /// Tree structure operations (add, move, delete, duplicate).
    ///
    /// Commands for modifying the block hierarchy.
    ///
    /// # Example
    /// ```bash
    /// block tree add-child 0x123 "New idea"
    /// block tree move 0xsrc 0xtgt --after
    /// block tree delete 0xleaf
    /// ```
    #[command(subcommand)]
    Tree(TreeCommands),

    /// Navigation operations (DFS traversal respecting folds).
    ///
    /// Commands for traversing the visible block order.
    ///
    /// # Example
    /// ```bash
    /// block nav next 0x123
    /// block nav prev 0x123
    /// block nav lineage 0x123
    /// ```
    #[command(subcommand)]
    Nav(NavCommands),

    /// Draft operations (LLM in-progress suggestions).
    ///
    /// Commands for managing expansion, reduction, instruction, and inquiry drafts.
    ///
    /// # Example
    /// ```bash
    /// block draft expand 0x123 --rewrite "Refined" --children "a" "b"
    /// block draft reduce 0x123 --reduction "Condensed"
    /// block draft list 0x123
    /// ```
    #[command(subcommand)]
    Draft(DraftCommands),

    /// Fold (collapse) state operations.
    ///
    /// Commands for toggling and querying block visibility.
    ///
    /// # Example
    /// ```bash
    /// block fold toggle 0x123
    /// block fold status 0x123
    /// ```
    #[command(subcommand)]
    Fold(FoldCommands),

    /// Friend block operations.
    ///
    /// Commands for managing related context blocks for LLM context building.
    ///
    /// # Example
    /// ```bash
    /// block friend add 0x123 0x456 --perspective "Related"
    /// block friend list 0x123
    /// block friend remove 0x123 0x456
    /// ```
    #[command(subcommand)]
    Friend(FriendCommands),

    /// Mount operations (external file integration).
    ///
    /// Commands for mounting external block store files.
    ///
    /// # Example
    /// ```bash
    /// block mount set 0x123 /path/to/file.json
    /// block mount expand 0x123
    /// block mount collapse 0x123
    /// block mount extract 0x123 --output /backup.json
    /// ```
    #[command(subcommand)]
    Mount(MountCommands),

    /// Panel state operations.
    ///
    /// Commands for persisting UI panel visibility state.
    ///
    /// # Example
    /// ```bash
    /// block panel set 0x123 friends
    /// block panel get 0x123
    /// block panel clear 0x123
    /// ```
    #[command(subcommand)]
    Panel(PanelCommands),

    /// Get LLM context for a block.
    ///
    /// Returns the full context envelope used by inquire/expand/reduce operations:
    /// lineage, direct children, and friend blocks.
    ///
    /// # Example
    /// ```bash
    /// block context 0x123
    /// block context 0x123 --output json
    /// ```
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
