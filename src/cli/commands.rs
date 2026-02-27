//! CLI commands: high-level command enums.

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
#[derive(Debug, Parser)]
#[command(name = "blooming-blockery", about, long_about)]
pub struct BlockCli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to the block store file.
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<std::path::PathBuf>,

    /// Enable verbose output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format for query commands.
    #[arg(long, global = true, value_name = "FORMAT", default_value = "table")]
    pub output: OutputFormat,
}

/// Available commands.
#[derive(Debug, Parser)]
pub enum Commands {
    /// Launch the GUI (default).
    Gui,
    /// Generate shell completions.
    GenerateCompletion {
        #[arg(value_name = "SHELL")]
        shell: clap_complete::Shell,
    },
    /// Block store manipulation commands.
    #[command(subcommand)]
    Block(BlockCommands),
}

/// Block store manipulation commands.
#[derive(Debug, Parser)]
pub enum BlockCommands {
    Roots(RootCommand),
    Show(ShowCommand),
    Find(FindCommand),
    #[command(subcommand)]
    Tree(TreeCommands),
    #[command(subcommand)]
    Nav(NavCommands),
    #[command(subcommand)]
    Draft(DraftCommands),
    #[command(subcommand)]
    Fold(FoldCommands),
    #[command(subcommand)]
    Friend(FriendCommands),
    #[command(subcommand)]
    Mount(MountCommands),
    #[command(subcommand)]
    Panel(PanelCommands),
    Context(ContextCommand),
}
