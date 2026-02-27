//! Block Store CLI: Declarative command-line interface types.
//!
//! This module defines the complete CLI API for manipulating the block store
//! via [clap](https://docs.rs/clap) derive macros. Each command, argument, and
//! variant is documented with usage patterns, examples, and error conditions.
//!
//! # Design Principles
//!
//! - Subcommand hierarchy: Commands are grouped by domain (tree, nav, draft,
//!   fold, friend, mount, panel) for discoverability.
//! - Idempotency: Commands that modify state return clear success/failure
//!   indicators. Read commands are safe and side-effect free.
//! - Rich errors: Each command documents its failure modes for debugging.
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

use crate::store::{MountFormat as StoreMountFormat, PanelBarState as StorePanelBarState};
use clap::ValueEnum;

pub mod commands;
pub mod context;
pub mod draft;
pub mod execute;
pub mod fold;
pub mod friend;
pub mod mount;
pub mod nav;
pub mod output;
pub mod panel;
pub mod query;
pub mod results;
pub mod tree;

pub use commands::{BlockCli, BlockCommands, Commands};
pub use output::print_result;
pub use results::CliResult;

// ============================================================================
// CLI Custom Types
// ============================================================================

/// Block ID type for CLI argument parsing.
///
/// Accepts block IDs in clean "NvG" format (e.g., `1v1`, `2v3`) where:
/// - `N` = slot index in the store
/// - `G` = generation counter (incremented on reuse)
///
/// Matching is case-insensitive and flexible.
///
/// # Examples
///
/// ```bash
/// block show 1v1
/// block show 2v3
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockId(pub String);

impl std::str::FromStr for BlockId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s.is_empty() {
            return Err("Invalid BlockId: cannot be empty".to_string());
        }

        Ok(Self(s.to_string()))
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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

/// Output format for query commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// JSON output for scripting.
    Json,
    /// Table format for human readability.
    Table,
}

#[cfg(test)]
mod tests_simple;

#[cfg(test)]
mod tests_integration;
