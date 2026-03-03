//! Mount commands (external file integration).

use super::{
    BlockId, MountFormatCli, execute,
    results::{BatchError, BatchOutput, CliResult},
};
use crate::store::{BlockStore, MountFormat};
use clap::Parser;

/// Mount operations (external file integration).
#[derive(Debug, Parser)]
pub enum MountCommands {
    /// Set a mount path on a block.
    ///
    /// Converts a leaf block into a mount point referencing an external file.
    /// The block must have no children.
    /// Fails if the block is missing or already has children.
    /// Example: `bb mount set 1v1 /notes/notes.md --format markdown`.
    Set(SetMountCommand),

    /// Expand a mount point.
    ///
    /// Loads the external file, re-keys its blocks into the main store, and
    /// replaces the mount node with inline children.
    /// Returns the root IDs of the loaded subtree.
    /// Fails if the block is missing, is not a mount, or the file cannot be read.
    /// Example: `bb mount expand 1v1`.
    Expand(ExpandMountCommand),

    /// Collapse an expanded mount.
    ///
    /// Removes the loaded blocks, restores the mount node with its original path.
    /// Fails if the block is missing or is not an expanded mount.
    /// Example: `bb mount collapse 1v1`.
    Collapse(CollapseMountCommand),

    /// Move a mount file and update mount metadata.
    ///
    /// Works for both expanded and unexpanded mounts. Expanded mounts write
    /// current mounted content to the new path, while unexpanded mounts move
    /// the existing backing file.
    /// Example: `bb mount move 1v1 /data/moved.md`.
    Move(MoveMountCommand),

    /// Inline a single mount into the current store.
    ///
    /// If the mount is not expanded yet, this expands it first and then removes
    /// runtime mount tracking while keeping the loaded children as normal blocks.
    /// Example: `bb mount inline 1v1`.
    Inline(InlineMountCommand),

    /// Inline all mounts recursively under a mount point.
    ///
    /// Traverses the subtree and inlines every reachable mount.
    /// Returns the number of inlined mount points.
    /// Example: `bb mount inline-recursive 1v1`.
    InlineRecursive(InlineRecursiveMountCommand),

    /// Extract a subtree to an external file.
    ///
    /// Saves the block's children (and their subtrees) to a file, then replaces
    /// the block with a mount node pointing to that file.
    /// Fails if the block is missing, has no children, or the output file cannot
    /// be written.
    /// Example: `bb mount extract 1v1 --output notes.md --format markdown`.
    Extract(ExtractMountCommand),

    /// Show mount information for a block.
    /// Example: `bb mount info 1v1`.
    Info(InfoMountCommand),

    /// Save all expanded mounts back to their source files.
    ///
    /// This writes each expanded mount subtree to its tracked file path and
    /// format. Useful after editing mounted content through CLI commands.
    /// Example: `bb mount save`.
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

// =============================================================================
// Execution
// =============================================================================

/// Execute a mount command.
pub fn execute(
    store: BlockStore, cmd: MountCommands, base_dir: &std::path::Path,
) -> (BlockStore, CliResult) {
    match cmd {
        | MountCommands::Set(c) => execute_set(store, &c),
        | MountCommands::Expand(c) => execute_expand(store, &c, base_dir),
        | MountCommands::Collapse(c) => execute_collapse(store, &c),
        | MountCommands::Move(c) => execute_move(store, &c, base_dir),
        | MountCommands::Inline(c) => execute_inline(store, &c, base_dir),
        | MountCommands::InlineRecursive(c) => execute_inline_recursive(store, &c, base_dir),
        | MountCommands::Extract(c) => execute_extract(store, &c, base_dir),
        | MountCommands::Info(c) => execute_info(store, &c),
        | MountCommands::Save(_) => execute_save(store),
    }
}

fn execute_set(mut store: BlockStore, cmd: &SetMountCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    let format: MountFormat = cmd.format.into();
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                match store.set_mount_path_with_format(&block_id, cmd.path.clone(), format) {
                    | Some(()) => (store, CliResult::Success),
                    | None => (
                        store,
                        CliResult::Error(
                            "Failed to set mount path (block may have children)".to_string(),
                        ),
                    ),
                }
            }
        }
    } else {
        if !execute::is_directory_like(&cmd.path) {
            return (
                store,
                CliResult::Error("Batch mount set requires a directory-like PATH".to_string()),
            );
        }

        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        let ext = execute::mount_format_extension(format);
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    let path = execute::batch_child_file_path(&cmd.path, &input, ext);
                    match store.set_mount_path_with_format(&block_id, path, format) {
                        | Some(()) => outputs.push(BatchOutput::Success { input }),
                        | None => errors.push(BatchError {
                            input,
                            error: "Failed to set mount path (block may have children)".to_string(),
                        }),
                    }
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.set", outputs, errors)))
    }
}

fn execute_expand(
    mut store: BlockStore, cmd: &ExpandMountCommand, base_dir: &std::path::Path,
) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.expand_mount(&block_id, base_dir) {
                | Ok(_) => (store, CliResult::Success),
                | Err(e) => (store, CliResult::Error(format!("Expand failed: {}", e))),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.expand_mount(&block_id, base_dir) {
                    | Ok(_) => outputs.push(BatchOutput::Success { input }),
                    | Err(e) => {
                        errors.push(BatchError { input, error: format!("Expand failed: {}", e) })
                    }
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.expand", outputs, errors)))
    }
}

fn execute_collapse(mut store: BlockStore, cmd: &CollapseMountCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.collapse_mount(&block_id) {
                | Some(()) => (store, CliResult::Success),
                | None => (store, CliResult::Error("Block is not an expanded mount".to_string())),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.collapse_mount(&block_id) {
                    | Some(()) => outputs.push(BatchOutput::Success { input }),
                    | None => errors.push(BatchError {
                        input,
                        error: "Block is not an expanded mount".to_string(),
                    }),
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.collapse", outputs, errors)))
    }
}

fn execute_move(
    mut store: BlockStore, cmd: &MoveMountCommand, base_dir: &std::path::Path,
) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.move_mount_file(&block_id, &cmd.path, base_dir) {
                | Ok(()) => (store, CliResult::Success),
                | Err(e) => (store, CliResult::Error(format!("Move failed: {}", e))),
            },
        }
    } else {
        if !execute::is_directory_like(&cmd.path) {
            return (
                store,
                CliResult::Error("Batch mount move requires a directory-like PATH".to_string()),
            );
        }

        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    let ext = execute::mount_format_extension(
                        execute::mount_format_for_block(&store, &block_id)
                            .unwrap_or(MountFormat::Json),
                    );
                    let path = execute::batch_child_file_path(&cmd.path, &input, ext);
                    match store.move_mount_file(&block_id, &path, base_dir) {
                        | Ok(()) => outputs.push(BatchOutput::Success { input }),
                        | Err(e) => {
                            errors.push(BatchError { input, error: format!("Move failed: {}", e) })
                        }
                    }
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.move", outputs, errors)))
    }
}

fn execute_inline(
    mut store: BlockStore, cmd: &InlineMountCommand, base_dir: &std::path::Path,
) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.inline_mount(&block_id, base_dir) {
                | Ok(()) => (store, CliResult::Success),
                | Err(e) => (store, CliResult::Error(format!("Inline failed: {}", e))),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.inline_mount(&block_id, base_dir) {
                    | Ok(()) => outputs.push(BatchOutput::Success { input }),
                    | Err(e) => {
                        errors.push(BatchError { input, error: format!("Inline failed: {}", e) })
                    }
                },
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.inline", outputs, errors)))
    }
}

fn execute_inline_recursive(
    mut store: BlockStore, cmd: &InlineRecursiveMountCommand, base_dir: &std::path::Path,
) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.inline_mount_recursive(&block_id, base_dir) {
                | Ok(count) => (store, CliResult::MountInlined(count)),
                | Err(e) => (store, CliResult::Error(format!("Inline recursive failed: {}", e))),
            },
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => match store.inline_mount_recursive(&block_id, base_dir) {
                    | Ok(count) => outputs.push(BatchOutput::InlinedCount { input, count }),
                    | Err(e) => errors.push(BatchError {
                        input,
                        error: format!("Inline recursive failed: {}", e),
                    }),
                },
            }
        }
        (
            store,
            CliResult::Batch(execute::make_batch_result("mount.inline-recursive", outputs, errors)),
        )
    }
}

fn execute_extract(
    mut store: BlockStore, cmd: &ExtractMountCommand, base_dir: &std::path::Path,
) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    let format_override = cmd.format.map(Into::into);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => match store.save_subtree_to_file_with_format(
                &block_id,
                &cmd.output,
                base_dir,
                format_override,
            ) {
                | Ok(()) => (store, CliResult::Success),
                | Err(e) => (store, CliResult::Error(format!("Extract failed: {}", e))),
            },
        }
    } else {
        if !execute::is_directory_like(&cmd.output) {
            return (
                store,
                CliResult::Error(
                    "Batch mount extract requires a directory-like --output PATH".to_string(),
                ),
            );
        }

        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        let ext = format_override.map(execute::mount_format_extension).unwrap_or("json");

        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    let path = execute::batch_child_file_path(&cmd.output, &input, ext);
                    match store.save_subtree_to_file_with_format(
                        &block_id,
                        &path,
                        base_dir,
                        format_override,
                    ) {
                        | Ok(()) => outputs.push(BatchOutput::Success { input }),
                        | Err(e) => errors
                            .push(BatchError { input, error: format!("Extract failed: {}", e) }),
                    }
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.extract", outputs, errors)))
    }
}

fn execute_info(store: BlockStore, cmd: &InfoMountCommand) -> (BlockStore, CliResult) {
    let targets = execute::expand_cli_targets(&cmd.block_id);
    if targets.len() == 1 {
        let id = execute::resolve_block_id(&store, &targets[0]);
        match id {
            | None => (store, CliResult::Error("Unknown block ID".to_string())),
            | Some(block_id) => {
                let node = store.node(&block_id);
                let mount_entry = store.mount_table().entry(block_id);
                let result = match (node, mount_entry) {
                    | (Some(crate::store::BlockNode::Mount { path, format }), None) => {
                        CliResult::MountInfo {
                            path: Some(path.display().to_string()),
                            format: format!("{}", format),
                            expanded: false,
                        }
                    }
                    | (_, Some(entry)) => CliResult::MountInfo {
                        path: Some(entry.path.display().to_string()),
                        format: format!("{}", entry.format),
                        expanded: true,
                    },
                    | _ => CliResult::Error("Block is not a mount".to_string()),
                };
                (store, result)
            }
        }
    } else {
        let mut outputs = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            let input = target.0.clone();
            match execute::resolve_block_id(&store, &target) {
                | None => errors.push(BatchError { input, error: "Unknown block ID".to_string() }),
                | Some(block_id) => {
                    let node = store.node(&block_id);
                    let mount_entry = store.mount_table().entry(block_id);
                    match (node, mount_entry) {
                        | (Some(crate::store::BlockNode::Mount { path, format }), None) => outputs
                            .push(BatchOutput::MountInfo {
                                input,
                                path: Some(path.display().to_string()),
                                format: format!("{}", format),
                                expanded: false,
                            }),
                        | (_, Some(entry)) => outputs.push(BatchOutput::MountInfo {
                            input,
                            path: Some(entry.path.display().to_string()),
                            format: format!("{}", entry.format),
                            expanded: true,
                        }),
                        | _ => errors
                            .push(BatchError { input, error: "Block is not a mount".to_string() }),
                    }
                }
            }
        }
        (store, CliResult::Batch(execute::make_batch_result("mount.info", outputs, errors)))
    }
}

fn execute_save(store: BlockStore) -> (BlockStore, CliResult) {
    match store.save_mounts() {
        | Ok(()) => (store, CliResult::Success),
        | Err(e) => (store, CliResult::Error(format!("Save mounts failed: {}", e))),
    }
}
