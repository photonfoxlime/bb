//! Mount and file I/O handler: file-backed block management.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Handles expanding/collapsing mount points (blocks backed by external files),
//! save/load dialogs, mount relocation, and mount inlining.

use super::editor_buffers::EditorBuffers;
use super::error::{AppError, UiError};
use super::{AppState, Message};
use crate::paths::AppPaths;
use crate::store::{BlockId, MountFormat};
use iced::Task;
use rust_i18n::t;

/// Messages for mount, file I/O, and system theme operations.
#[derive(Debug, Clone)]
pub enum MountFileMessage {
    /// Expand one mount point.
    ExpandMount(BlockId),
    /// Collapse one expanded mount back to a path link.
    CollapseMount(BlockId),
    /// Save a subtree into a mount file.
    SaveToFile(BlockId),
    /// Save-file picker result.
    SaveToFilePicked { block_id: BlockId, path: Option<std::path::PathBuf> },
    /// Attach a leaf block to an existing file.
    LoadFromFile(BlockId),
    /// Load-file picker result.
    LoadFromFilePicked { block_id: BlockId, path: Option<std::path::PathBuf> },
    /// Move a mounted file and update mount metadata.
    MoveMount(BlockId),
    /// Move-mount save-file picker result.
    MoveMountPicked { block_id: BlockId, path: Option<std::path::PathBuf> },
    /// Inline all mounted files reachable from this block.
    ///
    /// Uses a two-click confirmation flow: first click arms, second click executes.
    InlineMountAll(BlockId),
}

/// Process one mount/file message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: MountFileMessage) -> Task<Message> {
    match message {
        | MountFileMessage::ExpandMount(block_id) => {
            let base_dir = AppPaths::data_dir().unwrap_or_default();
            state.mutate_with_undo_and_persist("after expanding mount", |state| {
                    match state.store.expand_mount(&block_id, &base_dir) {
                        | Ok(new_roots) => {
                            tracing::info!(block_id = ?block_id, children = new_roots.len(), "expanded mount");
                            for &id in &new_roots {
                                state.editor_buffers.ensure_subtree(&state.store, &id);
                            }
                            true
                        }
                        | Err(err) => {
                            tracing::error!(block_id = ?block_id, %err, "failed to expand mount");
                            state.record_error(AppError::Mount(UiError::from_message(&err)));
                            false
                        }
                    }
                });
            Task::none()
        }
        | MountFileMessage::CollapseMount(block_id) => {
            state.mutate_with_undo_and_persist("after collapsing mount", |state| {
                if let Some(()) = state.store.collapse_mount(&block_id) {
                    tracing::info!(block_id = ?block_id, "collapsed mount");
                    state.editor_buffers = EditorBuffers::from_store(&state.store);
                    return true;
                }
                false
            });
            Task::none()
        }
        | MountFileMessage::SaveToFile(block_id) => {
            state.set_overflow_open(false);
            let title = t!("save_block_to_file").to_string();
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title(title)
                        .add_filter("JSON", &["json"])
                        .add_filter("Markdown", &["md", "markdown"])
                        .save_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| {
                    Message::MountFile(MountFileMessage::SaveToFilePicked { block_id, path })
                },
            )
        }
        | MountFileMessage::SaveToFilePicked { block_id, path } => {
            if let Some(path) = path {
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                state.mutate_with_undo_and_persist("after save-to-file", |state| {
                        match state.store.save_subtree_to_file(&block_id, &path, &base_dir) {
                            | Ok(()) => {
                                let mount_format = state
                                    .store
                                    .node(&block_id)
                                    .and_then(|node| node.mount_format())
                                    .unwrap_or(MountFormat::Json);
                                tracing::info!(block_id = ?block_id, path = %path.display(), ?mount_format, "saved subtree to file");
                                match state.store.expand_mount(&block_id, &base_dir) {
                                    | Ok(new_roots) => {
                                        for &id in &new_roots {
                                            state.editor_buffers.ensure_subtree(&state.store, &id);
                                        }
                                    }
                                    | Err(err) => {
                                        tracing::error!(block_id = ?block_id, %err, "failed to re-expand after save-to-file");
                                        state.record_error(AppError::Mount(UiError::from_message(&err)));
                                    }
                                }
                                true
                            }
                            | Err(err) => {
                                tracing::error!(block_id = ?block_id, %err, "failed to save subtree to file");
                                state.record_error(AppError::Mount(UiError::from_message(&err)));
                                false
                            }
                        }
                    });
            }
            Task::none()
        }
        | MountFileMessage::LoadFromFile(block_id) => {
            state.set_overflow_open(false);
            let title = t!("load_block_from_file").to_string();
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title(title)
                        .add_filter("JSON", &["json"])
                        .add_filter("Markdown", &["md", "markdown"])
                        .pick_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| {
                    Message::MountFile(MountFileMessage::LoadFromFilePicked { block_id, path })
                },
            )
        }
        | MountFileMessage::LoadFromFilePicked { block_id, path } => {
            if let Some(path) = path {
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                state.mutate_with_undo_and_persist("after load-from-file", |state| {
                        let rel_path = path
                            .strip_prefix(&base_dir)
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|_| path.clone());
                        let mount_format = match path
                            .extension()
                            .and_then(std::ffi::OsStr::to_str)
                            .map(str::to_ascii_lowercase)
                            .as_deref()
                        {
                            | Some("md") | Some("markdown") => MountFormat::Markdown,
                            | _ => MountFormat::Json,
                        };
                        let mounted = match mount_format {
                            | MountFormat::Json => state.store.set_mount_path(&block_id, rel_path),
                            | MountFormat::Markdown => {
                                state.store.set_mount_path_with_format(&block_id, rel_path, mount_format)
                            }
                        };
                        if mounted.is_none() {
                            tracing::error!(block_id = ?block_id, "block has children or does not exist; cannot load");
                            return false;
                        }
                        match state.store.expand_mount(&block_id, &base_dir) {
                            | Ok(new_roots) => {
                                tracing::info!(block_id = ?block_id, path = %path.display(), children = new_roots.len(), "loaded file into block");
                                for &id in &new_roots {
                                    state.editor_buffers.ensure_subtree(&state.store, &id);
                                }
                            }
                            | Err(err) => {
                                tracing::error!(block_id = ?block_id, %err, "failed to expand after load-from-file");
                                state.record_error(AppError::Mount(UiError::from_message(&err)));
                            }
                        }
                        true
                    });
            }
            Task::none()
        }
        | MountFileMessage::MoveMount(block_id) => {
            state.set_overflow_open(false);
            let title = t!("move_mounted_file").to_string();
            Task::perform(
                async move {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title(title)
                        .add_filter("JSON", &["json"])
                        .add_filter("Markdown", &["md", "markdown"])
                        .save_file()
                        .await;
                    dialog.map(|handle| handle.path().to_path_buf())
                },
                move |path| {
                    Message::MountFile(MountFileMessage::MoveMountPicked { block_id, path })
                },
            )
        }
        | MountFileMessage::MoveMountPicked { block_id, path } => {
            if let Some(path) = path {
                let base_dir = AppPaths::data_dir().unwrap_or_default();
                state.mutate_with_undo_and_persist("after moving mount file", |state| match state
                    .store
                    .move_mount_file(&block_id, &path, &base_dir)
                {
                    | Ok(()) => {
                        tracing::info!(
                            block_id = ?block_id,
                            path = %path.display(),
                            "moved mount file"
                        );
                        true
                    }
                    | Err(err) => {
                        tracing::error!(
                            block_id = ?block_id,
                            path = %path.display(),
                            %err,
                            "failed to move mount file"
                        );
                        state.record_error(AppError::Mount(UiError::from_message(&err)));
                        false
                    }
                });
            }
            Task::none()
        }
        | MountFileMessage::InlineMountAll(block_id) => {
            if state.ui_state.pending_inline_mount_confirmation != Some(block_id) {
                state.ui_state.pending_inline_mount_confirmation = Some(block_id);
                tracing::info!(block_id = ?block_id, "armed inline-all confirmation for mount");
                return Task::none();
            }

            state.ui_state.pending_inline_mount_confirmation = None;
            let base_dir = AppPaths::data_dir().unwrap_or_default();
            state.mutate_with_undo_and_persist(
                "after inlining mounted subtree",
                |state| match state.store.inline_mount_recursive(&block_id, &base_dir) {
                    | Ok(inlined_mount_count) => {
                        tracing::info!(
                            block_id = ?block_id,
                            inlined_mount_count,
                            "inlined mounted subtree"
                        );
                        state.editor_buffers = EditorBuffers::from_store(&state.store);
                        true
                    }
                    | Err(err) => {
                        tracing::error!(block_id = ?block_id, %err, "failed to inline mounts");
                        state.record_error(AppError::Mount(UiError::from_message(&err)));
                        false
                    }
                },
            );
            Task::none()
        }
    }
}
