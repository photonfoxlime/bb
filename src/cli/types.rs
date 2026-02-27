//! CLI custom types for argument parsing.

use crate::store::{MountFormat as StoreMountFormat, PanelBarState as StorePanelBarState};
use clap::ValueEnum;

/// Block ID type for CLI argument parsing.
///
/// Parses the string and resolves it against the store's slotmap.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockId(pub String);

impl std::str::FromStr for BlockId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        let hex_part = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);

        if hex_part.len() != 10 {
            return Err(format!(
                "Invalid BlockId: expected 10 hex characters after 0x, got {} ('{}')",
                hex_part.len(),
                s
            ));
        }

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
