//! Parallel text editor buffer storage keyed by block id.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Manages text editor buffers for each block in the tree, plus a shared
//! buffer for the instruction panel in the overlay panel bar.

use crate::store::{BlockId, BlockStore};
use crate::text::{WordTokenSpan, WordTokenizationCache};
use iced::widget::{self, text_editor};
use slotmap::SecondaryMap;

/// Maps each block to its iced `text_editor::Content` buffer and widget id.
///
/// Invariant: every block id present in the store has a corresponding
/// buffer and widget id. Rebuilt from scratch on undo/redo; incrementally
/// updated on add/remove/edit operations.
#[derive(Clone)]
pub(crate) struct EditorBuffers {
    buffers: SecondaryMap<BlockId, text_editor::Content>,
    /// Stable `widget::Id` per block, used for programmatic focus.
    widget_ids: SecondaryMap<BlockId, widget::Id>,
    /// Per-block cache of line tokenization for word-motion cursor shortcuts.
    word_token_cache: SecondaryMap<BlockId, WordTokenizationCache>,
    /// Text editor content for the instruction panel draft.
    ///
    /// This buffer is independent from per-block point editors and is consumed
    /// by inquire / expand / reduce instruction submissions.
    instruction_content: text_editor::Content,
}

impl Default for EditorBuffers {
    fn default() -> Self {
        Self {
            buffers: SecondaryMap::new(),
            widget_ids: SecondaryMap::new(),
            word_token_cache: SecondaryMap::new(),
            instruction_content: text_editor::Content::new(),
        }
    }
}

impl EditorBuffers {
    pub(crate) fn from_store(block_store: &BlockStore) -> Self {
        let mut store = Self::default();
        store.populate(block_store, block_store.roots());
        store
    }

    pub(crate) fn populate(&mut self, block_store: &BlockStore, ids: &[BlockId]) {
        for id in ids {
            let point = match block_store.point(id) {
                | Some(p) => p,
                | None => continue,
            };
            self.buffers.insert(*id, text_editor::Content::with_text(&point));
            self.widget_ids.insert(*id, widget::Id::unique());
            self.word_token_cache.insert(*id, WordTokenizationCache::default());
            let children: Vec<BlockId> = block_store.children(id).to_vec();
            self.populate(block_store, &children);
        }
    }

    pub(crate) fn ensure_block(&mut self, block_store: &BlockStore, block_id: &BlockId) {
        if self.buffers.contains_key(*block_id) {
            return;
        }
        let point = block_store.point(block_id).unwrap_or_default();
        self.buffers.insert(*block_id, text_editor::Content::with_text(&point));
        self.widget_ids.insert(*block_id, widget::Id::unique());
        self.word_token_cache.insert(*block_id, WordTokenizationCache::default());
    }

    pub(crate) fn get(&self, block_id: &BlockId) -> Option<&text_editor::Content> {
        self.buffers.get(*block_id)
    }

    pub(crate) fn get_mut(&mut self, block_id: &BlockId) -> Option<&mut text_editor::Content> {
        self.buffers.get_mut(*block_id)
    }

    pub(crate) fn set_text(&mut self, block_id: &BlockId, value: &str) {
        self.buffers.insert(*block_id, text_editor::Content::with_text(value));
        self.widget_ids.insert(*block_id, widget::Id::unique());
        self.word_token_cache.insert(*block_id, WordTokenizationCache::default());
    }

    /// Invalidate the word token cache for a block after its content changed.
    ///
    /// This forces re-tokenization on the next `word_token_spans_for_line` call.
    /// Call this after `text_editor::Content::perform` to keep the cache in sync
    /// with the editor buffer.
    pub(crate) fn invalidate_token_cache(&mut self, block_id: &BlockId) {
        if let Some(cache) = self.word_token_cache.get_mut(*block_id) {
            *cache = WordTokenizationCache::default();
        }
    }

    pub(crate) fn ensure_subtree(&mut self, block_store: &BlockStore, block_id: &BlockId) {
        if block_store.point(block_id).is_none() {
            return;
        }
        if !self.buffers.contains_key(*block_id) {
            let point = block_store.point(block_id).unwrap_or_default();
            self.buffers.insert(*block_id, text_editor::Content::with_text(&point));
            self.widget_ids.insert(*block_id, widget::Id::unique());
            self.word_token_cache.insert(*block_id, WordTokenizationCache::default());
        }
        let children: Vec<BlockId> = block_store.children(block_id).to_vec();
        for child in &children {
            self.ensure_subtree(block_store, child);
        }
    }

    pub(crate) fn remove_blocks(&mut self, block_ids: &[BlockId]) {
        for id in block_ids {
            self.buffers.remove(*id);
            self.widget_ids.remove(*id);
            self.word_token_cache.remove(*id);
        }
    }

    /// Return cached word token spans for one line in one block editor.
    pub(crate) fn word_token_spans_for_line(
        &mut self, block_id: &BlockId, line: &str,
    ) -> Vec<WordTokenSpan> {
        let cache = match self.word_token_cache.entry(*block_id) {
            | Some(entry) => entry.or_insert_with(WordTokenizationCache::default),
            | None => {
                tracing::warn!(block_id = ?block_id, "word_token_spans_for_line called with invalid block_id");
                return Vec::new();
            }
        };
        cache.spans_for_line(line).to_vec()
    }

    /// Get the `widget::Id` for a block's text editor (for programmatic focus).
    pub(crate) fn widget_id(&self, block_id: &BlockId) -> Option<&widget::Id> {
        self.widget_ids.get(*block_id)
    }

    /// Get the instruction panel text editor content.
    pub(crate) fn instruction_content(&self) -> &text_editor::Content {
        &self.instruction_content
    }

    /// Get mutable reference to instruction panel text editor content.
    pub(crate) fn instruction_content_mut(&mut self) -> &mut text_editor::Content {
        &mut self.instruction_content
    }

    /// Set instruction panel draft text.
    pub(crate) fn set_instruction_text(&mut self, text: &str) {
        self.instruction_content = text_editor::Content::with_text(text);
    }
}
