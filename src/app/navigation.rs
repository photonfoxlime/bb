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
//! The navigation system enables users to "drill down" into block subtrees.
//! Instead of showing all blocks flat in one tree, users can focus on a
//! specific branch by navigating into it.
//!
//! # Navigation Stack Design
//!
//! The navigation stack is initially **empty** at startup, representing the
//! main document root view. When the user drills down into a block, a layer
//! is pushed onto the stack.
//!
//! ## Stack States
//!
//! - **Empty stack**: Viewing the main document roots (default state)
//! - **Non-empty stack**: Viewing the subtree of the top layer's block
//!
//! Each [`NavigationLayer`] tracks:
//! - The block whose children are currently visible
//! - The optional file path (for mount points)
//!
//! ## Path Tracking
//!
//! Each layer optionally stores a file path. This serves two purposes:
//! 1. **Breadcrumb display**: Shows the source file name for context
//! 2. **Mount point detection**: When navigating into a mount, the path is
//!    derived from the mount metadata automatically
//!
//! # Invariants
//!
//! - All block IDs in the stack must exist in the store
//! - Navigation state is part of the undo snapshot for consistency

use crate::app::{AppState, Message};
use crate::store::{BlockId, BlockStore};
use iced::Task;
use std::path::PathBuf;

/// Messages for navigation operations.
///
/// These messages drive state transitions for the navigation stack,
/// including drill-down and breadcrumb navigation.
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
    /// - `Some(path)`: Block belongs to a mount point-backed subtree
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
/// # Stack States
///
/// - **Empty**: Viewing the main document roots (default startup state)
/// - **Non-empty**: Viewing the subtree of the top layer's block
///
/// # Invariants
///
/// - All block IDs reference valid blocks in the store
/// - Mount file paths are preserved for breadcrumb display
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

    /// Clear the navigation stack, returning to the root view.
    ///
    /// Removes all layers from the stack. When empty, the view shows
    /// the main document roots.
    pub fn clear(&mut self) {
        self.layers.clear();
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

    /// Rebuild the stack so the parent of `target` becomes the current view.
    ///
    /// This method is used by global find navigation: selecting a result should
    /// reveal the target block in context without mutating fold state.
    ///
    /// # Behavior
    /// - If `target` is a root, the stack is cleared (root view).
    /// - Otherwise, stack layers become the ordered ancestor chain
    ///   `root -> ... -> parent(target)`.
    /// - Existing path hints are preserved when a matching ancestor already
    ///   exists in the current stack.
    pub fn reveal_parent_path(&mut self, store: &BlockStore, target: &BlockId) {
        if store.node(target).is_none() {
            tracing::error!(target = ?target, "cannot reveal parent path for missing block");
            return;
        }

        let old_layers = self.layers.clone();
        let mut ancestors = Vec::new();
        let mut cursor = store.parent(target);
        while let Some(parent) = cursor {
            ancestors.push(parent);
            cursor = store.parent(&parent);
        }
        ancestors.reverse();

        self.layers = ancestors
            .into_iter()
            .map(|block_id| {
                let path = old_layers
                    .iter()
                    .find(|layer| layer.block_id == block_id)
                    .and_then(|layer| layer.path.clone());
                NavigationLayer { block_id, path }
            })
            .collect();

        tracing::debug!(target = ?target, depth = self.layers.len(), "revealed parent path");
    }

    /// Check if a block is within the current navigation view.
    ///
    /// A block is in view if:
    /// - The navigation stack is empty (viewing root) and the block is in the main document
    /// - The block is a descendant of the current navigation layer's block
    ///
    /// This check is structural (based on parent-child relationships) and does
    /// not consider fold state. Use
    /// [`crate::store::BlockStoreNavigateExt::is_visible`] to check fold state.
    ///
    /// # Arguments
    ///
    /// * `store` - The block store to query for structure
    /// * `block_id` - The block to check
    ///
    /// # Returns
    ///
    /// `true` if the block is within the current navigation view, `false` otherwise.
    pub fn is_in_current_view(&self, store: &BlockStore, block_id: &BlockId) -> bool {
        // Empty stack means viewing all roots
        let Some(current_layer) = self.current() else {
            return store.node(block_id).is_some();
        };

        // Check if block_id is a descendant of current_layer.block_id
        // or is the current layer's block itself
        if block_id == &current_layer.block_id {
            return true;
        }

        // Walk up the parent chain to see if we reach the current layer's block
        let mut current = *block_id;
        while let Some(parent) = store.parent(&current) {
            if parent == current_layer.block_id {
                return true;
            }
            current = parent;
        }
        false
    }
}

/// Process one navigation message and return a follow-up task (if any).
///
/// This is the main entry point for navigation state transitions.
/// It validates inputs and updates the navigation stack.
///
/// # Arguments
///
/// * `state` - Mutable reference to the application state
/// * `message` - The navigation message to process
///
/// # Returns
///
/// `Task::none()` after synchronous state updates.
pub fn handle(state: &mut AppState, message: NavigationMessage) -> Task<Message> {
    // Clear any reference-panel friend highlight on navigation.
    state.ui_mut().reference_panel.highlighted_friend_block = None;

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
            state.navigation.clear();
            Task::none()
        }
    }
}
