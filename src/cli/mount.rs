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
    /// Fails if the block is missing or already has children.
    /// Example: `blooming-blockery block mount set 1v1 /notes/notes.md --format markdown`.
    Set(SetMountCommand),

    /// Expand a mount point.
    ///
    /// Loads the external file, re-keys its blocks into the main store, and
    /// replaces the mount node with inline children.
    /// Returns the root IDs of the loaded subtree.
    /// Fails if the block is missing, is not a mount, or the file cannot be read.
    /// Example: `blooming-blockery block mount expand 1v1`.
    Expand(ExpandMountCommand),

    /// Collapse an expanded mount.
    ///
    /// Removes the loaded blocks, restores the mount node with its original path.
    /// Fails if the block is missing or is not an expanded mount.
    /// Example: `blooming-blockery block mount collapse 1v1`.
    Collapse(CollapseMountCommand),

    /// Move a mount file and update mount metadata.
    ///
    /// Works for both expanded and unexpanded mounts. Expanded mounts write
    /// current mounted content to the new path, while unexpanded mounts move
    /// the existing backing file.
    /// Example: `blooming-blockery block mount move 1v1 /data/moved.md`.
    Move(MoveMountCommand),

    /// Inline a single mount into the current store.
    ///
    /// If the mount is not expanded yet, this expands it first and then removes
    /// runtime mount tracking while keeping the loaded children as normal blocks.
    /// Example: `blooming-blockery block mount inline 1v1`.
    Inline(InlineMountCommand),

    /// Inline all mounts recursively under a mount point.
    ///
    /// Traverses the subtree and inlines every reachable mount.
    /// Returns the number of inlined mount points.
    /// Example: `blooming-blockery block mount inline-recursive 1v1`.
    InlineRecursive(InlineRecursiveMountCommand),

    /// Extract a subtree to an external file.
    ///
    /// Saves the block's children (and their subtrees) to a file, then replaces
    /// the block with a mount node pointing to that file.
    /// Fails if the block is missing, has no children, or the output file cannot
    /// be written.
    /// Example: `blooming-blockery block mount extract 1v1 --output notes.md --format markdown`.
    Extract(ExtractMountCommand),

    /// Show mount information for a block.
    /// Example: `blooming-blockery block mount info 1v1`.
    Info(InfoMountCommand),

    /// Save all expanded mounts back to their source files.
    ///
    /// This writes each expanded mount subtree to its tracked file path and
    /// format. Useful after editing mounted content through CLI commands.
    /// Example: `blooming-blockery block mount save`.
    Save(SaveMountsCommand),
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
    /// Use `json` for full block-store JSON (default) or `markdown` for
    /// Markdown Mount v1 format.
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

/// Move mount file.
#[derive(Debug, Parser)]
pub struct MoveMountCommand {
    /// Mount point block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,

    /// New path for the mounted file.
    #[arg(value_name = "PATH")]
    pub path: std::path::PathBuf,
}

/// Inline one mount.
#[derive(Debug, Parser)]
pub struct InlineMountCommand {
    /// Mount point block.
    #[arg(value_name = "BLOCK_ID")]
    pub block_id: BlockId,
}

/// Inline all mounts under a subtree.
#[derive(Debug, Parser)]
pub struct InlineRecursiveMountCommand {
    /// Root block to traverse.
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

/// Save all expanded mounts.
#[derive(Debug, Parser)]
pub struct SaveMountsCommand {}
