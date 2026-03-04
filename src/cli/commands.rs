//! CLI command entry points and top-level command enums.
//!
//! This module defines the main CLI application structure using `clap` derive macros.
//! It serves as the primary interface between user input and the block store execution engine.
//!
//! # Architecture
//!
//! Basic Block (`bb`) is organized into a hierarchical command structure:
//!
//! ```text
//! BbCli
//! ├── generate-completion
//! ├── roots, show, find (query commands)
//! ├── tree (structural edits)
//! ├── nav (navigation)
//! ├── draft (LLM interaction drafts)
//! ├── fold (collapse/expand)
//! ├── friend (cross-reference links)
//! ├── mount (external file integration)
//! ├── panel (sidebar state)
//! └── context (LLM context inspection)
//! ```
//!
//! # Design Principles
//!
//! - Global flags: `--store`, `--verbose`, and `--output` apply to all commands
//! - Subcommand organization: Related operations are grouped by domain for discoverability
//! - Type-safe arguments: Custom types like `BlockId` and `OutputFormat` enforce validity at parse time
//!
//! # Example Usage
//!
//! ```bash
//! # Query with JSON output
//! bb --output json roots
//!
//! # Tree operations
//! bb tree add-child 1v1 "New idea"
//! bb tree move 1v1 2v1 --after
//!
//! # Draft management
//! bb draft amplify 1v1 --rewrite "Refined text" --children "Child 1" "Child 2"
//!
//! # Mount external files
//! bb mount set 1v1 /path/to/file.md --format markdown
//! bb mount expand 1v1
//! ```
//!
//! # Execution Flow
//!
//! 1. CLI arguments are parsed into `Commands`
//! 2. `Commands::execute()` is called with the `BlockStore`
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
pub use super::point::EditPointCommand;
pub use super::query::{FindCommand, RootCommand, ShowCommand};
pub use super::tree::TreeCommands;

/// Basic Block: CLI for block store manipulation.
#[derive(Debug, Parser)]
#[command(
    name = "bb",
    version = env!("CARGO_PKG_VERSION"),
    about = "Basic Block: CLI for block store manipulation.",
    long_about = "Basic Block: CLI for block store manipulation. Use `blooming-blockery` for Blooming Blockery (GUI)."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to the block store file.
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<std::path::PathBuf>,

    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[arg(long, global = true, value_name = "FORMAT", default_value = "table")]
    pub output: OutputFormat,
}

/// Commands available in Basic Block (no GUI variant).
#[derive(Debug, Parser)]
pub enum Commands {
    GenerateCompletion {
        #[arg(value_name = "SHELL")]
        shell: clap_complete::Shell,
    },
    /// List all root block IDs.
    Roots(RootCommand),
    /// Show details of a specific block.
    Show(ShowCommand),
    /// Search blocks by point text using mixed-language matching.
    Find(FindCommand),
    /// Edit the text content of a block.
    Point(EditPointCommand),
    /// Structural tree editing operations.
    #[command(subcommand)]
    Tree(TreeCommands),
    /// Navigation operations (next, previous, lineage).
    #[command(subcommand)]
    Nav(NavCommands),
    /// LLM draft management (amplify, distill, instruction, probe).
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
