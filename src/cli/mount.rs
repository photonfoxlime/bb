//! Mount commands (external file integration).

use super::{BlockId, MountFormatCli};
use clap::Parser;

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
