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

pub mod commands;
pub mod context;
pub mod draft;
pub mod execute;
pub mod fold;
pub mod friend;
pub mod mount;
pub mod nav;
pub mod panel;
pub mod query;
pub mod results;
pub mod tree;
pub mod types;

pub use commands::{BlockCli, BlockCommands, Commands};
pub use results::{CliResult, ExpansionDraftInfo, FriendInfo, Match, ReductionDraftInfo};
pub use types::{BlockId, MountFormatCli, OutputFormat, PanelBarStateCli};
