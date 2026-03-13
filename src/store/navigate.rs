//! DFS navigation and LLM context assembly.
//!
//! Provides visible-order traversal (respecting collapsed blocks) and
//! context builders that gather lineage, children, and friend-block text
//! for LLM requests.

use crate::{llm, text::extract_search_phrases};
use blooming_blockery_store::{BlockId, BlockStore, FriendBlock};

/// App-local navigation and context helpers layered on top of the persisted
/// [`BlockStore`] model.
///
/// Import this trait anywhere method-style access is needed:
/// `use crate::store::BlockStoreNavigateExt as _;`
pub trait BlockStoreNavigateExt {
    /// Find blocks whose point text matches a user query.
    ///
    /// Matching strategy:
    /// - case-insensitive full-query substring match, or
    /// - case-insensitive phrase-token match using [`extract_search_phrases`].
    ///
    /// Result order is deterministic DFS order across all roots.
    /// Empty queries match all blocks.
    fn find_block_point(&self, query: &str) -> Vec<BlockId>;

    /// Return lineage points from one root to the target id.
    fn lineage_points_for_id(&self, target: &BlockId) -> llm::LineageContext;

    /// Build a [`llm::BlockContext`] for the given block from all visible context.
    fn block_context_for_id(&self, target: &BlockId) -> llm::BlockContext;

    /// Build a [`llm::BlockContext`] with user-selected friend blocks.
    fn block_context_for_id_with_friend_blocks(
        &self, target: &BlockId, friend_block_ids: &[FriendBlock],
    ) -> llm::BlockContext;

    /// Return the next block in visible DFS order, skipping collapsed subtrees.
    fn next_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId>;

    /// Return the previous block in visible DFS order, skipping collapsed subtrees.
    fn prev_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId>;

    /// Check if a block is visible in the current view.
    fn is_visible(&self, block_id: &BlockId) -> bool;
}

impl BlockStoreNavigateExt for BlockStore {
    fn find_block_point(&self, query: &str) -> Vec<BlockId> {
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
            find_block_point_in_subtree(self, root, &query_lower, &query_terms, &mut matched);
        }

        tracing::debug!(
            query = %normalized_query,
            token_count = query_terms.len(),
            match_count = matched.len(),
            "searched block points"
        );
        matched
    }

    /// Return lineage points from one root to the target id.
    ///
    /// # Requires
    /// - `target` must exist in the store.
    ///
    /// # Ensures
    /// - Returns a `Lineage` containing all ancestor point texts from root to target.
    fn lineage_points_for_id(&self, target: &BlockId) -> llm::LineageContext {
        let mut current = match self.node(target) {
            | Some(_) => *target,
            | None => return llm::LineageContext::from_points(vec![]),
        };

        let mut collected = vec![self.point(&current).unwrap_or_default()];
        while let Some(parent) = self.parent(&current) {
            collected.push(self.point(&parent).unwrap_or_default());
            current = parent;
        }
        collected.reverse();
        llm::LineageContext::from_points(collected)
    }

    /// Build a [`llm::BlockContext`] for the given block from all visible context.
    ///
    /// Visibility model:
    /// - target point (as the final lineage item),
    /// - parent chain (earlier lineage items),
    /// - direct children point texts,
    /// - user-selected friend blocks.
    ///
    /// Used by amplify/distill/atomize/probe handlers so all four operations read the
    /// same context envelope.
    fn block_context_for_id(&self, target: &BlockId) -> llm::BlockContext {
        let friend_ids = self.friend_blocks_for(target).to_vec();
        self.block_context_for_id_with_friend_blocks(target, &friend_ids)
    }

    /// Build a [`llm::BlockContext`] with user-selected friend blocks.
    ///
    /// Friend blocks are extra readable blocks outside the target's direct
    /// children and may include an optional per-friend perspective.
    fn block_context_for_id_with_friend_blocks(
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
    /// Uses [`BlockStore::is_collapsed`] to determine which blocks are folded.
    /// Returns `None` when `current` is the last visible block.
    fn next_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId> {
        // If current has visible children, descend into the first child.
        if !self.is_collapsed(current) {
            let children = self.children(current);
            if let Some(&first) = children.first() {
                return Some(first);
            }
        }
        // Otherwise walk up ancestors looking for a next sibling.
        let mut target = *current;
        loop {
            let (parent, index) = parent_and_index_of(self, &target)?;
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
    /// Uses [`BlockStore::is_collapsed`] to determine which blocks are folded.
    /// Returns `None` when `current` is the first visible block.
    fn prev_visible_in_dfs(&self, current: &BlockId) -> Option<BlockId> {
        let (parent, index) = parent_and_index_of(self, current)?;
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
            if self.is_collapsed(&target) {
                return Some(target);
            }
            let children = self.children(&target);
            match children.last() {
                | None => return Some(target),
                | Some(&last) => target = last,
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
    fn is_visible(&self, block_id: &BlockId) -> bool {
        if self.node(block_id).is_none() {
            return false;
        }
        // Walk up the ancestor chain; if any ancestor is collapsed, return false.
        let mut current = *block_id;
        while let Some(parent) = self.parent(&current) {
            if self.is_collapsed(&parent) {
                return false;
            }
            current = parent;
        }
        true
    }
}

fn find_block_point_in_subtree(
    store: &BlockStore, current: &BlockId, query_lower: &str, query_terms: &[String],
    out: &mut Vec<BlockId>,
) {
    let point = store.points.get(current).map(|pc| pc.display_text()).unwrap_or_default();
    let point_lower = point.to_lowercase();
    let is_match = point_lower.contains(query_lower)
        || query_terms.iter().any(|phrase| point_lower.contains(phrase));
    if is_match {
        out.push(*current);
    }

    for child in store.children(current) {
        find_block_point_in_subtree(store, child, query_lower, query_terms, out);
    }
}

/// Compute a block's parent and index using only public `BlockStore` queries.
///
/// Note: the core store crate keeps this helper private because it is an
/// implementation detail of app-local traversal logic.
fn parent_and_index_of(store: &BlockStore, target: &BlockId) -> Option<(Option<BlockId>, usize)> {
    if let Some(parent) = store.parent(target) {
        let index = store.children(&parent).iter().position(|child| child == target)?;
        return Some((Some(parent), index));
    }
    let index = store.roots().iter().position(|root| root == target)?;
    Some((None, index))
}
