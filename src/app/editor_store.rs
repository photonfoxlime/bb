//! Parallel text editor buffer storage keyed by block id.

use crate::store::{BlockId, BlockNode, BlockStore};
use iced::widget::text_editor;
use std::collections::HashMap;

/// Maps each block to its iced `text_editor::Content` buffer.
///
/// Invariant: every block id present in the store has a corresponding
/// buffer. Rebuilt from scratch on undo/redo; incrementally updated
/// on add/remove/edit operations.
#[derive(Clone, Default)]
pub(crate) struct EditorStore {
    buffers: HashMap<BlockId, text_editor::Content>,
}

impl EditorStore {
    pub(crate) fn from_store(block_store: &BlockStore) -> Self {
        let mut store = Self::default();
        store.populate(block_store, block_store.roots());
        store
    }

    pub(crate) fn populate(&mut self, block_store: &BlockStore, ids: &[BlockId]) {
        for id in ids {
            let Some(node): Option<&BlockNode> = block_store.node(id) else {
                continue;
            };
            self.buffers.insert(id.clone(), text_editor::Content::with_text(&node.point));
            self.populate(block_store, &node.children);
        }
    }

    pub(crate) fn ensure_block(&mut self, block_store: &BlockStore, block_id: &BlockId) {
        if self.buffers.contains_key(block_id) {
            return;
        }
        let point = block_store.point(block_id).unwrap_or_default();
        self.buffers.insert(block_id.clone(), text_editor::Content::with_text(&point));
    }

    pub(crate) fn get(&self, block_id: &BlockId) -> Option<&text_editor::Content> {
        self.buffers.get(block_id)
    }

    pub(crate) fn get_mut(&mut self, block_id: &BlockId) -> Option<&mut text_editor::Content> {
        self.buffers.get_mut(block_id)
    }

    pub(crate) fn set_text(&mut self, block_id: &BlockId, value: &str) {
        self.buffers.insert(block_id.clone(), text_editor::Content::with_text(value));
    }

    pub(crate) fn ensure_subtree(&mut self, block_store: &BlockStore, block_id: &BlockId) {
        let Some(node): Option<&BlockNode> = block_store.node(block_id) else {
            return;
        };
        self.buffers
            .entry(block_id.clone())
            .or_insert_with(|| text_editor::Content::with_text(&node.point));
        for child in &node.children {
            self.ensure_subtree(block_store, child);
        }
    }

    pub(crate) fn remove_blocks(&mut self, block_ids: &[BlockId]) {
        for id in block_ids {
            self.buffers.remove(id);
        }
    }
}
