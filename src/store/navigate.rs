//! DFS navigation and LLM context assembly.
//!
//! Provides visible-order traversal (respecting collapsed blocks) and
//! context builders that gather lineage, children, and friend-block text
//! for LLM requests.

use super::{BlockId, BlockStore, FriendBlock};
use crate::llm;

impl BlockStore {
    /// Return lineage points from one root to the target id (DFS).
    ///
    /// # Requires
    /// - `target` must exist in the store.
    ///
    /// # Ensures
    /// - Returns a `Lineage` containing all ancestor point texts from root to target.
    pub fn lineage_points_for_id(&self, target: &BlockId) -> llm::Lineage {
        for root in &self.roots {
            let mut collected = Vec::new();
            if self.collect_lineage_points(root, target, &mut collected) {
                return llm::Lineage::from_points(collected);
            }
        }
        llm::Lineage::from_points(vec![])
    }

    /// Build a [`llm::BlockContext`] for the given block from all visible context.
    ///
    /// Visibility model:
    /// - target point (as the final lineage item),
    /// - parent chain (earlier lineage items),
    /// - direct children point texts,
    /// - user-selected friend blocks.
    ///
    /// Used by inquire/reduce/expand handlers so all three operations read the
    /// same context envelope.
    pub fn block_context_for_id(&self, target: &BlockId) -> llm::BlockContext {
        let friend_ids = self.friend_blocks.get(*target).cloned().unwrap_or_default();
        self.block_context_for_id_with_friend_blocks(target, &friend_ids)
    }

    /// Build a [`llm::BlockContext`] with user-selected friend blocks.
    ///
    /// Friend blocks are extra readable blocks outside the target's direct
    /// children and may include an optional per-friend perspective.
    pub fn block_context_for_id_with_friend_blocks(
        &self, target: &BlockId, friend_block_ids: &[FriendBlock],
    ) -> llm::BlockContext {
        let lineage = self.lineage_points_for_id(target);
        let existing_children = self
            .children(target)
            .iter()
            .filter_map(|child_id| self.point(child_id))
            .collect::<Vec<_>>();
        let friend_blocks = friend_block_ids
            .iter()
            .filter_map(|friend| {
                self.point(&friend.block_id)
                    .map(|point| llm::FriendContext::new(point, friend.perspective.clone()))
            })
            .collect::<Vec<_>>();
        llm::BlockContext::new(lineage, existing_children, friend_blocks)
    }

    /// Return the next block in visible DFS order, skipping collapsed subtrees.
    ///
    /// Uses [`Self::view_collapsed`] to determine which blocks are folded.
    /// Returns `None` when `current` is the last visible block.
    pub fn next_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId> {
        // If current has visible children, descend into the first child.
        if !self.view_collapsed.contains_key(*current) {
            let children = self.children(current);
            if let Some(&first) = children.first() {
                return Some(first);
            }
        }
        // Otherwise walk up ancestors looking for a next sibling.
        let mut target = *current;
        loop {
            let (parent, index) = self.parent_and_index_of(&target)?;
            let siblings = match parent {
                | Some(pid) => self.children(&pid),
                | None => self.roots(),
            };
            if index + 1 < siblings.len() {
                return Some(siblings[index + 1]);
            }
            // No next sibling: move up to parent and retry.
            match parent {
                | Some(pid) => target = pid,
                | None => return None,
            }
        }
    }

    /// Return the previous block in visible DFS order, skipping collapsed subtrees.
    ///
    /// Uses [`Self::view_collapsed`] to determine which blocks are folded.
    /// Returns `None` when `current` is the first visible block.
    pub fn prev_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId> {
        let (parent, index) = self.parent_and_index_of(current)?;
        if index == 0 {
            // No previous sibling; go to parent (None for root-0 means we are first).
            return parent;
        }
        let siblings = match parent {
            | Some(pid) => self.children(&pid),
            | None => self.roots(),
        };
        // Previous sibling's deepest visible descendant.
        let mut target = siblings[index - 1];
        loop {
            if self.view_collapsed.contains_key(target) {
                return Some(target);
            }
            let children = self.children(&target);
            if children.is_empty() {
                return Some(target);
            }
            if let Some(&last) = children.last() {
                target = last;
            } else {
                return Some(target);
            }
        }
    }
}
