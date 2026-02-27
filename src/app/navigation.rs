//! Navigation stack: tracks drill-down path through block subtrees.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! The navigation stack enables "drilling down" into block subtrees, whether
//! from the main document or external files. Each layer remembers the block id
//! and optionally the file path for breadcrumb display.

use crate::app::{AppState, Message};
use crate::store::{BlockId, BlockStore};
use iced::Task;
use std::path::PathBuf;

/// Messages for navigation operations.
#[derive(Debug, Clone)]
pub enum NavigationMessage {
    /// Navigate into a block's subtree.
    Enter(BlockId),
    /// Jump to a specific breadcrumb depth.
    GoTo(usize),
    /// Pop one layer (back button).
    Back,
    /// Pop to root.
    Home,
    /// Open an external file dialog.
    OpenExternalDialog,
    /// Result of opening an external file dialog.
    OpenExternalPicked { path: Option<PathBuf> },
    /// External file loaded successfully (internal).
    ExternalLoaded { path: PathBuf, store: BlockStore, root_id: BlockId },
    /// External file load failed (internal).
    ExternalLoadFailed { path: PathBuf, error: String },
}

/// A single layer in the navigation stack.
#[derive(Debug, Clone)]
pub struct NavigationLayer {
    /// The block whose subtree is currently visible.
    pub block_id: BlockId,
    /// Optional file path for breadcrumb display.
    /// None = main document root.
    /// For mounts: derived from mount_point's path.
    /// For external files: the file path that was opened.
    pub path: Option<PathBuf>,
}

/// Navigation stack: tracks drill-down path through blocks.
#[derive(Debug, Clone, Default)]
pub struct NavigationStack {
    layers: Vec<NavigationLayer>,
}

impl NavigationStack {
    /// Start at main document root.
    pub fn new_root(block_id: BlockId) -> Self {
        Self { layers: vec![NavigationLayer { block_id, path: None }] }
    }

    /// Push a new layer onto the stack.
    pub fn push(&mut self, block_id: BlockId, path: Option<PathBuf>) {
        self.layers.push(NavigationLayer { block_id, path });
    }

    /// Pop to a specific depth (0 = root).
    pub fn pop_to(&mut self, depth: usize) {
        if depth < self.layers.len() {
            self.layers.truncate(depth + 1);
        }
    }

    /// Get the current (top) layer.
    pub fn current(&self) -> Option<&NavigationLayer> {
        self.layers.last()
    }

    /// Get all layers for breadcrumb rendering.
    pub fn layers(&self) -> &[NavigationLayer] {
        &self.layers
    }

    /// Check if at root (only one layer).
    pub fn is_root(&self) -> bool {
        self.layers.len() <= 1
    }

    /// Get the current block id.
    pub fn current_block_id(&self) -> Option<BlockId> {
        self.current().map(|l| l.block_id)
    }
}

/// Process one navigation message and return a follow-up task (if any).
pub fn handle(state: &mut AppState, message: NavigationMessage) -> Task<Message> {
    match message {
        | NavigationMessage::Enter(block_id) => {
            // Validate block exists
            if state.store.node(&block_id).is_none() {
                tracing::warn!(block_id = ?block_id, "navigation target does not exist");
                return Task::none();
            }
            // Look up path: check if block is a mount point
            let path =
                state.store.node(&block_id).and_then(|n| n.mount_path().map(|p| p.to_path_buf()));
            state.navigation.push(block_id, path);
            // Ensure editor buffers exist for the new root
            state.editor_buffers.ensure_block(&state.store, &block_id);
            Task::none()
        }
        | NavigationMessage::GoTo(depth) => {
            state.navigation.pop_to(depth);
            // Ensure editor buffers for the new current block
            if let Some(current_id) = state.navigation.current_block_id() {
                state.editor_buffers.ensure_block(&state.store, &current_id);
            }
            Task::none()
        }
        | NavigationMessage::Back => {
            let new_len = state.navigation.layers().len().saturating_sub(2);
            state.navigation.pop_to(new_len);
            if let Some(current_id) = state.navigation.current_block_id() {
                state.editor_buffers.ensure_block(&state.store, &current_id);
            }
            Task::none()
        }
        | NavigationMessage::Home => {
            state.navigation.pop_to(0);
            if let Some(current_id) = state.navigation.current_block_id() {
                state.editor_buffers.ensure_block(&state.store, &current_id);
            }
            Task::none()
        }
        | NavigationMessage::OpenExternalDialog => {
            use rust_i18n::t;
            let title = t!("open_external_document").to_string();
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
                |path| Message::Navigation(NavigationMessage::OpenExternalPicked { path }),
            )
        }
        | NavigationMessage::OpenExternalPicked { path } => {
            if let Some(path) = path {
                Task::perform(
                    async move {
                        match load_external_file(&path).await {
                            | Ok((store, root_id)) => Ok((path, store, root_id)),
                            | Err(e) => Err((path, e)),
                        }
                    },
                    |result| match result {
                        | Ok((path, store, root_id)) => {
                            Message::Navigation(NavigationMessage::ExternalLoaded {
                                path,
                                store,
                                root_id,
                            })
                        }
                        | Err((path, error)) => {
                            Message::Navigation(NavigationMessage::ExternalLoadFailed {
                                path,
                                error,
                            })
                        }
                    },
                )
            } else {
                Task::none()
            }
        }
        | NavigationMessage::ExternalLoaded { path, store, root_id } => {
            // Merge external store into main store
            // Use the current navigation root as the mount_point for origin tracking
            let mount_point = state.navigation.current_block_id().unwrap_or(root_id);
            let (new_roots, _all_ids) = state.store.rekey_sub_store(&store, &mount_point);
            if let Some(&new_root) = new_roots.first() {
                state.navigation.push(new_root, Some(path));
                state.editor_buffers.ensure_block(&state.store, &new_root);
            }
            Task::none()
        }
        | NavigationMessage::ExternalLoadFailed { path, error } => {
            tracing::error!(path = %path.display(), %error, "failed to load external file");
            // TODO: show error banner to user
            Task::none()
        }
    }
}

async fn load_external_file(path: &std::path::Path) -> Result<(BlockStore, BlockId), String> {
    let path = path.to_path_buf();
    let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
        .await
        .map_err(|e| format!("failed to read file: {e}"))?
        .map_err(|e| format!("failed to read file: {e}"))?;

    // Try JSON first, then Markdown
    let store = if let Ok(store) = serde_json::from_str::<BlockStore>(&content) {
        store
    } else {
        // Try markdown - need to expose this from store module
        return Err("Markdown mount files not yet supported for external open".to_string());
    };

    let root_id = *store.roots().first().ok_or("external file has no root block")?;
    Ok((store, root_id))
}
