//! Navigation stack: drill-down view through block subtrees.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! # Overview
//!
//! The navigation system enables users to "drill down" into block subtrees,
//! whether from the main document or external files. Instead of showing all
//! blocks flat in one tree, users can focus on a specific branch by navigating
//! into it.
//!
//! # Design Decisions
//!
//! ## Stack-Based Navigation
//!
//! Navigation is modeled as a stack of [`NavigationLayer`]s. Each layer represents
//! one level of drill-down, tracking:
//! - The block whose children are currently visible
//! - The optional file path (for external documents or mount points)
//!
//! The stack approach was chosen over a tree or parent-pointer design because:
//! - It naturally supports linear navigation (drill down, go back)
//! - Breadcrumbs are trivial to render from the layer list
//! - Undo/redo can snapshot the entire stack as a single value
//!
//! ## External File Integration
//!
//! External files (JSON format) are loaded into the main [`BlockStore`] via
//! [`BlockStore::rekey_sub_store`], which assigns fresh block IDs to avoid
//! collisions. The re-keyed blocks are then navigated to as a new layer.
//!
//! **Why merge instead of isolate?** Keeping all blocks in one store simplifies:
//! - Editor buffer management (single source of truth)
//! - Undo/redo (one store to snapshot)
//! - Friend block relationships (can link across "documents")
//!
//! ## Path Tracking
//!
//! Each layer optionally stores a file path. This serves two purposes:
//! 1. **Breadcrumb display**: Shows the source file name for context
//! 2. **Mount point detection**: When navigating into a mount, the path is
//!    derived from the mount metadata automatically
//!
//! ## Error Handling
//!
//! External file load failures are surfaced to the user via the error banner
//! system ([`AppError::Persistence`]). The navigation handler does not attempt
//! recovery; it logs the error and lets the user decide the next action.
//!
//! # Invariants
//!
//! - The navigation stack always has at least one layer (the root)
//! - All block IDs in the stack must exist in the store
//! - External file blocks are re-keyed to avoid ID collisions
//! - Navigation state is part of the undo snapshot for consistency

use crate::app::error::{AppError, UiError};
use crate::app::{AppState, Message};
use crate::store::{BlockId, BlockStore};
use iced::Task;
use std::path::PathBuf;

/// Messages for navigation operations.
///
/// These messages drive state transitions for the navigation stack,
/// including drill-down, breadcrumb navigation, and external file loading.
#[derive(Debug, Clone)]
pub enum NavigationMessage {
    /// Navigate into a block's subtree.
    ///
    /// The block must exist in the store and have children. A new layer
    /// is pushed onto the stack, and the view shifts to show the block's
    /// children.
    Enter(BlockId),

    /// Jump to a specific breadcrumb depth.
    ///
    /// Pops all layers above the specified depth, effectively navigating
    /// back to a previous point in the drill-down path. Depth 0 is root.
    GoTo(usize),

    /// Pop to root.
    ///
    /// Returns to the main document root, clearing all drill-down layers.
    Home,

    /// Open an external file dialog.
    ///
    /// Launches a file picker for JSON files. On success, transitions to
    /// [`Self::OpenExternalPicked`] with the selected path.
    OpenExternalDialog,

    /// Result of opening an external file dialog.
    ///
    /// Carries the selected path (or `None` if cancelled). Triggers
    /// asynchronous file loading.
    OpenExternalPicked {
        /// Selected file path, or `None` if dialog was cancelled.
        path: Option<PathBuf>,
    },

    /// External file loaded successfully (internal).
    ///
    /// Carries the loaded store and its root block ID. The handler merges
    /// the external store into the main store and navigates to the new root.
    ExternalLoaded {
        /// Path to the loaded file.
        path: PathBuf,
        /// The loaded block store.
        store: BlockStore,
        /// Root block ID of the loaded store.
        root_id: BlockId,
    },

    /// External file load failed (internal).
    ///
    /// Triggers an error banner to inform the user.
    ExternalLoadFailed {
        /// Path to the file that failed to load.
        path: PathBuf,
        /// Error message describing the failure.
        error: String,
    },
}

/// A single layer in the navigation stack.
///
/// Represents one level of drill-down into a block subtree.
#[derive(Debug, Clone)]
pub struct NavigationLayer {
    /// The block whose subtree is currently visible.
    ///
    /// This block's children are rendered as the "roots" in the tree view.
    pub block_id: BlockId,

    /// Optional file path for breadcrumb display.
    ///
    /// - `None`: Block belongs to the main document
    /// - `Some(path)`: Block was loaded from an external file or is a mount point
    ///
    /// The path is used for display in breadcrumbs, showing users the source
    /// of the current view. For mount points, this is derived automatically
    /// from the mount metadata.
    pub path: Option<PathBuf>,
}

/// Navigation stack: tracks drill-down path through blocks.
///
/// The stack maintains a linear history of navigation, where each layer
/// represents drilling deeper into a block's subtree. The top layer
/// (last element) is the current view.
///
/// # Invariants
///
/// - Always contains at least one layer (the root)
/// - All block IDs reference valid blocks in the store
/// - External file paths are preserved for breadcrumb display
///
/// # Undo/Redo Integration
///
/// The navigation stack is part of the undo snapshot. When undoing, both
/// the store state and navigation position are restored together, ensuring
/// consistency between structure and view.
#[derive(Debug, Clone, Default)]
pub struct NavigationStack {
    /// Stack of navigation layers. Bottom (index 0) = root, top = current view.
    layers: Vec<NavigationLayer>,
}

impl NavigationStack {
    /// Start at main document root.
    ///
    /// Creates a new stack with a single layer pointing to the root block.
    /// The root has no associated file path (`path = None`).
    pub fn new_root(block_id: BlockId) -> Self {
        Self { layers: vec![NavigationLayer { block_id, path: None }] }
    }

    /// Push a new layer onto the stack.
    ///
    /// Called when navigating into a block's subtree. The new layer
    /// becomes the current view (top of stack).
    ///
    /// # Arguments
    ///
    /// * `block_id` - The block whose children should be shown
    /// * `path` - Optional file path (auto-derived for mount points)
    pub fn push(&mut self, block_id: BlockId, path: Option<PathBuf>) {
        self.layers.push(NavigationLayer { block_id, path });
    }

    /// Pop to a specific depth (0 = root).
    ///
    /// Removes all layers above the specified depth, navigating back
    /// to a previous point in the drill-down path.
    ///
    /// # Arguments
    ///
    /// * `depth` - Target depth (0-based index into layers)
    ///
    /// # Panics
    ///
    /// If `depth` exceeds the current stack length, the stack is truncated
    /// to its current size (no-op for out-of-bounds).
    pub fn pop_to(&mut self, depth: usize) {
        if depth < self.layers.len() {
            self.layers.truncate(depth + 1);
        }
    }

    /// Get the current (top) layer.
    ///
    /// Returns the layer representing the current view. This is the
    /// block whose children are being displayed.
    pub fn current(&self) -> Option<&NavigationLayer> {
        self.layers.last()
    }

    /// Get all layers for breadcrumb rendering.
    ///
    /// Returns a slice of all layers from root to current. The UI
    /// renders these as clickable breadcrumbs.
    pub fn layers(&self) -> &[NavigationLayer] {
        &self.layers
    }

    /// Get the current block id.
    ///
    /// Convenience method to get the block ID of the current view
    /// without accessing the full layer struct.
    pub fn current_block_id(&self) -> Option<BlockId> {
        self.current().map(|l| l.block_id)
    }
}

/// Process one navigation message and return a follow-up task (if any).
///
/// This is the main entry point for navigation state transitions.
/// It validates inputs, updates the navigation stack, and triggers
/// side effects like file loading or error reporting.
///
/// # Arguments
///
/// * `state` - Mutable reference to the application state
/// * `message` - The navigation message to process
///
/// # Returns
///
/// A task for asynchronous operations (file dialogs, file loading),
/// or `Task::none()` for synchronous state updates.
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
            // Show error banner to user
            state.record_error(AppError::Persistence(UiError::from_message(format!(
                "Failed to open external document '{}': {}",
                path.display(),
                error
            ))));
            Task::none()
        }
    }
}

/// Load an external file into a [`BlockStore`].
///
/// Attempts to parse the file as JSON first (native format), then
/// Markdown (for mount compatibility). Returns the loaded store
/// and its root block ID.
///
/// # Arguments
///
/// * `path` - Path to the file to load
///
/// # Errors
///
/// Returns an error string if:
/// - File cannot be read (IO error)
/// - File is not valid JSON (parse error)
/// - File has no root block (structural error)
///
/// # Implementation Notes
///
/// Currently only JSON format is supported. Markdown support requires
/// exposing the markdown parser from the store module.
async fn load_external_file(path: &std::path::Path) -> Result<(BlockStore, BlockId), String> {
    let path = path.to_path_buf();
    let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
        .await
        .map_err(|e| format!("failed to read file: {e}"))?
        .map_err(|e| format!("failed to read file: {e}"))?;

    // Try JSON first (native format), then Markdown
    let store = if let Ok(store) = serde_json::from_str::<BlockStore>(&content) {
        store
    } else {
        // Markdown support not yet available for external open
        return Err("External file must be valid JSON (Markdown not yet supported)".to_string());
    };

    let root_id = *store.roots().first().ok_or("external file has no root block")?;
    Ok((store, root_id))
}
