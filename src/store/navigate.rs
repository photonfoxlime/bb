//! DFS navigation and LLM context assembly.
//!
//! Provides visible-order traversal (respecting collapsed blocks) and
//! context builders that gather lineage, children, and friend-block text
//! for LLM requests.

use super::{BlockId, BlockStore, FriendBlock};
use crate::{llm, text::extract_search_phrases};

impl BlockStore {
    /// Find blocks whose point text matches a user query.
    ///
    /// Matching strategy:
    /// - case-insensitive full-query substring match, or
    /// - case-insensitive phrase-token match using [`extract_search_phrases`].
    ///
    /// Result order is deterministic DFS order across all roots.
    /// Empty queries match all blocks.
    pub fn find_block_point(&self, query: &str) -> Vec<BlockId> {
        let normalized_query = query.trim();

        let query_lower = normalized_query.to_lowercase();
        let mut query_terms = extract_search_phrases(normalized_query, &[])
            .into_iter()
            .map(|phrase| phrase.to_lowercase())
            .filter(|phrase| !phrase.is_empty())
            .collect::<Vec<_>>();
        if query_terms.is_empty() {
            query_terms.push(query_lower.clone());
        }

        let mut matched = Vec::new();
        for root in &self.roots {
            self.find_block_point_in_subtree(root, &query_lower, &query_terms, &mut matched);
        }

        tracing::debug!(
            query = %normalized_query,
            token_count = query_terms.len(),
            match_count = matched.len(),
            "searched block points"
        );
        matched
    }

    fn find_block_point_in_subtree(
        &self, current: &BlockId, query_lower: &str, query_terms: &[String], out: &mut Vec<BlockId>,
    ) {
        let point = self.points.get(*current).map(String::as_str).unwrap_or_default();
        let point_lower = point.to_lowercase();
        let is_match = point_lower.contains(query_lower)
            || query_terms.iter().any(|phrase| point_lower.contains(phrase));
        if is_match {
            out.push(*current);
        }

        for child in self.children(current) {
            self.find_block_point_in_subtree(child, query_lower, query_terms, out);
        }
    }

    /// Return lineage points from one root to the target id (DFS).
    ///
    /// # Requires
    /// - `target` must exist in the store.
    ///
    /// # Ensures
    /// - Returns a `Lineage` containing all ancestor point texts from root to target.
    pub fn lineage_points_for_id(&self, target: &BlockId) -> llm::LineageContext {
        for root in &self.roots {
            let mut collected = Vec::new();
            if self.collect_lineage_points(root, target, &mut collected) {
                return llm::LineageContext::from_points(collected);
            }
        }
        llm::LineageContext::from_points(vec![])
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
        let existing_children = llm::ChildrenContext::from_points(
            self.children(target)
                .iter()
                .filter_map(|child_id| self.point(child_id))
                .collect::<Vec<_>>(),
        );
        let friend_blocks = friend_block_ids
            .iter()
            .filter_map(|friend| {
                let point = self.point(&friend.block_id)?;
                let friend_lineage = if friend.parent_lineage_telescope {
                    Some(self.lineage_points_for_id(&friend.block_id))
                } else {
                    None
                };
                let friend_children = if friend.children_telescope {
                    Some(llm::ChildrenContext::from_points(
                        self.children(&friend.block_id)
                            .iter()
                            .filter_map(|child_id| self.point(child_id))
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };
                Some(llm::FriendContext::with_context(
                    point,
                    friend.perspective.clone(),
                    friend.parent_lineage_telescope,
                    friend.children_telescope,
                    friend_lineage,
                    friend_children,
                ))
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

    /// Check if a block is visible in the current view.
    ///
    /// A block is visible if:
    /// - All its ancestors (up to a root) are not collapsed
    /// - The block itself exists in the store
    ///
    /// This does not check navigation layer visibility; it only checks
    /// the fold state. Use this in combination with navigation checks
    /// to determine if a block should be highlighted on hover.
    ///
    /// # Returns
    /// - `true` if the block exists and all ancestors are expanded
    /// - `false` if the block does not exist or any ancestor is collapsed
    pub fn is_visible(&self, block_id: &BlockId) -> bool {
        if self.node(block_id).is_none() {
            return false;
        }
        // Walk up the ancestor chain; if any ancestor is collapsed, return false
        let mut current = *block_id;
        while let Some(parent) = self.parent(&current) {
            if self.view_collapsed.contains_key(parent) {
                return false;
            }
            current = parent;
        }
        true
    }
}
