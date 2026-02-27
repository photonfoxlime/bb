//! CLI command entry points and top-level command enums.
//!
//! This module defines the main CLI application structure using `clap` derive macros.
//! It serves as the primary interface between user input and the block store execution engine.
//!
//! # Architecture
//!
//! The CLI is organized into a hierarchical command structure:
//!
//! ```text
//! BlockCli
//! ├── gui (default)
//! ├── generate-completion
//! └── block (BlockCommands)
//!     ├── roots, show, find (query commands)
//!     ├── tree (structural edits)
//!     ├── nav (navigation)
//!     ├── draft (LLM interaction drafts)
//!     ├── fold (collapse/expand)
//!     ├── friend (cross-reference links)
//!     ├── mount (external file integration)
//!     ├── panel (sidebar state)
//!     └── context (LLM context inspection)
//! ```
//!
//! # Design Principles
//!
//! - **Global flags**: `--store`, `--verbose`, and `--output` apply to all commands
//! - **Subcommand organization**: Related operations are grouped by domain for discoverability
//! - **Type-safe arguments**: Custom types like `BlockId` and `OutputFormat` enforce validity at parse time
//!
//! # Example Usage
//!
//! ```bash
//! # Query with JSON output
//! block --output json roots
//!
//! # Tree operations
//! block tree add-child 1v1b3c "New idea"
//! block tree move 1v1 2v1 --after
//!
//! # Draft management
//! block draft expand 1v1 --rewrite "Refined text" --children "Child 1" "Child 2"
//!
//! # Mount external files
//! block mount set 1v1 /path/to/file.md --format markdown
//! block mount expand 1v1
//! ```
//!
//! # Execution Flow
//!
//! 1. CLI arguments are parsed into `BlockCommands`
//! 2. `BlockCommands::execute()` is called with the `BlockStore`
//! 3. A `CliResult` is returned and formatted by `cli::output::print_result()`

use super::OutputFormat;
use clap::Parser;

pub use super::context::ContextCommand;
pub use super::draft::DraftCommands;
pub use super::fold::FoldCommands;
pub use super::friend::FriendCommands;
pub use super::mount::MountCommands;
pub use super::nav::NavCommands;
pub use super::panel::PanelCommands;
pub use super::query::{FindCommand, RootCommand, ShowCommand};
pub use super::tree::TreeCommands;

/// CLI application for manipulating the block document store.
///
/// This is the main entry point for the `block` command-line tool.
/// All flags are marked as `global = true`, allowing them to be specified
/// before or after the subcommand for user convenience.
#[derive(Debug, Parser)]
#[command(name = "blooming-blockery", about, long_about)]
pub struct BlockCli {
    /// The command to execute. Defaults to `Gui` if not specified.
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to the block store file.
    ///
    /// If not provided, defaults to [`crate::paths::AppPaths::data_file()`].
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<std::path::PathBuf>,

    /// Enable verbose output.
    ///
    /// Currently unused; reserved for future debugging output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format for query results.
    ///
    /// - `table`: Human-readable format (default)
    /// - `json`: Machine-readable JSON for scripting
    #[arg(long, global = true, value_name = "FORMAT", default_value = "table")]
    pub output: OutputFormat,
}

/// Top-level command categories.
#[derive(Debug, Parser)]
pub enum Commands {
    /// Launch the GUI (default).
    ///
    /// Opens the interactive graphical interface for visual block editing.
    Gui,
    /// Generate shell completions.
    ///
    /// Outputs completion scripts for bash, zsh, fish, elvish, and powershell.
    GenerateCompletion {
        #[arg(value_name = "SHELL")]
        shell: clap_complete::Shell,
    },
    /// Block store manipulation commands.
    ///
    /// This subcommand group contains all operations for reading and modifying
    /// the block document store.
    #[command(subcommand)]
    Block(BlockCommands),
}

/// Block store manipulation commands.
///
/// Each variant represents a distinct operation on the block store.
/// Commands are executed by `BlockCommands::execute()` which returns
/// both the (possibly modified) store and a `CliResult` for output.
#[derive(Debug, Parser)]
pub enum BlockCommands {
    /// List all root block IDs.
    Roots(RootCommand),
    /// Show details of a specific block.
    Show(ShowCommand),
    /// Search blocks by text content (case-insensitive).
    Find(FindCommand),
    /// Structural tree editing operations.
    #[command(subcommand)]
    Tree(TreeCommands),
    /// Navigation operations (next, previous, lineage).
    #[command(subcommand)]
    Nav(NavCommands),
    /// LLM draft management (expand, reduce, instruction, inquiry).
    #[command(subcommand)]
    Draft(DraftCommands),
    /// Fold/collapse block visibility.
    #[command(subcommand)]
    Fold(FoldCommands),
    /// Friend block (cross-reference) management.
    #[command(subcommand)]
    Friend(FriendCommands),
    /// External file mount operations.
    #[command(subcommand)]
    Mount(MountCommands),
    /// Panel sidebar state management.
    #[command(subcommand)]
    Panel(PanelCommands),
    /// Get block context for LLM requests.
    Context(ContextCommand),
}
