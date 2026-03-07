//! Tree mutation and subtree traversal helpers.
//!
//! Structural operations that add, remove, or rearrange blocks within the
//! forest.  Also contains internal helpers for collecting subtree ids and
//! walking the parent chain (lineage).

use super::{BlockId, BlockNode, BlockStore, Direction, PointContent, mount::BlockOrigin};

impl BlockStore {
    /// Add one child block under the parent and return the new child id.
    ///
    /// # Requires
    /// - `parent_id` must exist in the store.
    /// - The parent must not be a mount node.
    ///
    /// # Ensures
    /// - The returned `Some(BlockId)` references the newly created child block.
    /// - The child inherits the mount origin of its parent if applicable.
    pub fn append_child(&mut self, parent_id: &BlockId, point: String) -> Option<BlockId> {
        if !self.nodes.contains_key(parent_id) {
            return None;
        }

        let child_id = Self::insert_node(&mut self.nodes, BlockNode::with_children(vec![]));
        self.points.insert(child_id, PointContent::from(point));
        if let Some(mount_point) = self.inherited_mount_point_for_anchor(parent_id) {
            self.mount_table.set_origin(child_id, BlockOrigin::Mounted { mount_point });
        }
        if let Some(parent) = self.nodes.get_mut(parent_id)
            && let Some(children) = parent.children_mut()
        {
            children.push(child_id);
        }
        self.rebuild_parent_index();
        Some(child_id)
    }

    /// Wrap a block with a new parent inserted at the block's current position.
    ///
    /// Preserves sibling/root ordering by replacing the original slot with the
    /// new parent and attaching the target block as its first child.
    ///
    /// # Requires
    /// - `block_id` must exist in the store.
    ///
    /// # Ensures
    /// - Returns `Some(BlockId)` of the new parent.
    /// - The new parent inherits the mount origin of `block_id` if applicable.
    pub fn insert_parent(&mut self, block_id: &BlockId, point: String) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;

        let parent_block_id =
            Self::insert_node(&mut self.nodes, BlockNode::with_children(vec![*block_id]));
        self.points.insert(parent_block_id, PointContent::from(point));

        if let Some(mount_point) = self.inherited_mount_point_for_anchor(block_id) {
            self.mount_table.set_origin(parent_block_id, BlockOrigin::Mounted { mount_point });
        }

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            if let Some(children) = parent.children_mut() {
                children[index] = parent_block_id;
            }
        } else {
            self.roots[index] = parent_block_id;
        }

        self.rebuild_parent_index();
        Some(parent_block_id)
    }

    /// Insert a sibling block immediately after `block_id` in its parent's
    /// child list (or in roots if `block_id` is a root).
    ///
    /// # Requires
    /// - `block_id` must exist in the store.
    ///
    /// # Ensures
    /// - Returns `Some(BlockId)` of the newly created sibling.
    /// - The sibling inherits the mount origin of `block_id` if applicable.
    pub fn append_sibling(&mut self, block_id: &BlockId, point: String) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let sibling_id = Self::insert_node(&mut self.nodes, BlockNode::with_children(vec![]));
        self.points.insert(sibling_id, PointContent::from(point));
        if let Some(mount_point) = self.inherited_mount_point_for_anchor(block_id) {
            self.mount_table.set_origin(sibling_id, BlockOrigin::Mounted { mount_point });
        }

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            if let Some(children) = parent.children_mut() {
                children.insert(index + 1, sibling_id);
            }
        } else {
            self.roots.insert(index + 1, sibling_id);
        }
        self.rebuild_parent_index();
        Some(sibling_id)
    }

    /// Deep-clone a block and its entire subtree with fresh ids, inserting the
    /// copy immediately after the original.
    ///
    /// # Requires
    /// - `block_id` must exist in the store.
    ///
    /// # Ensures
    /// - Returns `Some(BlockId)` of the cloned root.
    /// - The duplicate inherits the mount origin of the original if applicable.
    pub fn duplicate_subtree_after(&mut self, block_id: &BlockId) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let duplicate_id = self.clone_subtree_with_new_ids(block_id)?;

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            if let Some(children) = parent.children_mut() {
                children.insert(index + 1, duplicate_id);
            }
        } else {
            self.roots.insert(index + 1, duplicate_id);
        }
        self.rebuild_parent_index();
        Some(duplicate_id)
    }

    /// Remove a block and its entire subtree.
    ///
    /// # Requires
    /// - `block_id` must exist in the store.
    ///
    /// # Ensures
    /// - Returns `Some(Vec<BlockId>)` of all removed ids (including the target and all descendants).
    /// - If removal empties the root list, a fresh empty root is inserted.
    /// - All draft records, friend blocks, and panel state for the removed subtree are cleaned up.
    pub fn remove_block_subtree(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(&parent_id)
                && let Some(children) = parent.children_mut()
            {
                children.remove(index);
            }
        } else {
            self.roots.remove(index);
        }

        let mut removed_ids = Vec::new();
        self.collect_subtree_ids(block_id, &mut removed_ids);
        for id in &removed_ids {
            self.remove_block_metadata(id);
        }
        self.remove_friend_block_references(&removed_ids);

        if self.roots.is_empty() {
            let root_id = Self::insert_node(&mut self.nodes, BlockNode::with_children(vec![]));
            self.points.insert(root_id, PointContent::default());
            self.roots.push(root_id);
        }

        self.rebuild_parent_index();
        Some(removed_ids)
    }

    /// Permanently delete a block that was previously archived.
    ///
    /// Removes the block from `self.archive` and destroys its entire subtree
    /// from `nodes`, `points`, and all draft maps.
    ///
    /// # Requires
    /// - `block_id` must be present in `self.archive`.
    ///
    /// # Ensures
    /// - Returns `Some(Vec<BlockId>)` of all destroyed ids (root first) on success.
    /// - Returns `None` if `block_id` is not found in `self.archive`.
    pub fn delete_archived_block(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>> {
        let pos = self.archive.iter().position(|id| id == block_id)?;
        self.archive.remove(pos);

        let mut removed_ids = Vec::new();
        self.collect_subtree_ids(block_id, &mut removed_ids);
        for id in &removed_ids {
            self.remove_block_metadata(id);
        }
        self.remove_friend_block_references(&removed_ids);

        self.rebuild_parent_index();
        Some(removed_ids)
    }

    /// Restore an archived block as the last child of `parent_id`.
    ///
    /// The archived subtree keeps its original ids and metadata; only the root
    /// is removed from [`BlockStore::archive`] and reattached under the live
    /// target parent.
    ///
    /// # Requires
    /// - `block_id` must be present in `self.archive`.
    /// - `parent_id` must be attached to the live tree.
    /// - `parent_id` must not be a mount node.
    ///
    /// # Ensures
    /// - Returns `Some(())` when the archived subtree is reattached.
    /// - Removes `block_id` from `self.archive`.
    pub fn restore_archived_block_as_child(
        &mut self, block_id: &BlockId, parent_id: &BlockId,
    ) -> Option<()> {
        let archive_index = self.archive.iter().position(|id| id == block_id)?;
        let _ = self.parent_and_index_of(parent_id)?;
        let parent = self.nodes.get_mut(parent_id)?;
        let children = parent.children_mut()?;
        self.archive.remove(archive_index);
        children.push(*block_id);
        self.rebuild_parent_index();
        Some(())
    }

    /// Restore an archived block as the next sibling after `target_id`.
    ///
    /// The archived subtree keeps its original ids and metadata; only the root
    /// is removed from [`BlockStore::archive`] and reattached into the live
    /// sibling/root order immediately after `target_id`.
    ///
    /// # Requires
    /// - `block_id` must be present in `self.archive`.
    /// - `target_id` must be attached to the live tree.
    ///
    /// # Ensures
    /// - Returns `Some(())` when the archived subtree is reattached.
    /// - Removes `block_id` from `self.archive`.
    pub fn restore_archived_block_as_sibling(
        &mut self, block_id: &BlockId, target_id: &BlockId,
    ) -> Option<()> {
        let archive_index = self.archive.iter().position(|id| id == block_id)?;
        let (parent_id, target_index) = self.parent_and_index_of(target_id)?;
        self.archive.remove(archive_index);

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            let children = parent.children_mut()?;
            children.insert(target_index + 1, *block_id);
        } else {
            self.roots.insert(target_index + 1, *block_id);
        }

        self.rebuild_parent_index();
        Some(())
    }

    /// Detach a block from its parent (or roots) and append its id to `archive`.
    ///
    /// The block and its entire subtree remain in the store (`nodes`, `points`,
    /// and all draft maps are untouched). Only the topmost detached id is pushed
    /// to `self.archive`.
    ///
    /// # Requires
    /// - `block_id` must exist in the store.
    ///
    /// # Ensures
    /// - Returns `Some(Vec<BlockId>)` containing the detached root id followed by
    ///   all descendant ids, so callers can clean up ancillary state (editor
    ///   buffers, LLM requests, focus). The block data itself is **not** removed.
    /// - `block_id` is appended to `self.archive`.
    /// - If detachment empties the root list, a fresh empty root is inserted.
    /// - Returns `None` if `block_id` is not found.
    pub fn archive_block(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(&parent_id)
                && let Some(children) = parent.children_mut()
            {
                children.remove(index);
            }
        } else {
            self.roots.remove(index);
        }

        self.archive.push(*block_id);

        if self.roots.is_empty() {
            let root_id = Self::insert_node(&mut self.nodes, BlockNode::with_children(vec![]));
            self.points.insert(root_id, PointContent::default());
            self.roots.push(root_id);
        }

        let mut subtree_ids = Vec::new();
        self.collect_subtree_ids(block_id, &mut subtree_ids);
        self.rebuild_parent_index();
        Some(subtree_ids)
    }

    /// Move a block to before, after, or under a target block.
    ///
    /// # Requires
    /// - `source_id` and `target_id` must both exist in the store.
    /// - `source_id` must not equal `target_id`.
    /// - `source_id` must not be an ancestor of `target_id` (cannot move parent into its own child).
    /// - For `Direction::Under`, the target must not be a mount node.
    ///
    /// # Ensures
    /// - Returns `Some(())` on success.
    /// - The source block (and its subtree) is repositioned relative to the target.
    pub fn move_block(
        &mut self, source_id: &BlockId, target_id: &BlockId, dir: Direction,
    ) -> Option<()> {
        if source_id == target_id {
            return None;
        }

        // Check that source is not an ancestor of target
        if self.is_ancestor(source_id, target_id) {
            return None;
        }

        // Find and remove source from its current position
        let (source_parent_id, source_index) = self.parent_and_index_of(source_id)?;
        if let Some(parent_id) = source_parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            if let Some(children) = parent.children_mut() {
                children.remove(source_index);
            }
        } else {
            self.roots.remove(source_index);
        }

        match dir {
            | Direction::Before | Direction::After => {
                // Find target position and insert source
                let (target_parent_id, target_index) = self.parent_and_index_of(target_id)?;

                // Adjust insertion index: if moving Before, insert at target_index; if After, insert at target_index + 1
                let insert_index = match dir {
                    | Direction::Before => target_index,
                    | Direction::After => target_index + 1,
                    | Direction::Under => unreachable!(),
                };

                if let Some(parent_id) = target_parent_id {
                    let parent = self.nodes.get_mut(&parent_id)?;
                    if let Some(children) = parent.children_mut() {
                        children.insert(insert_index, *source_id);
                    }
                } else {
                    self.roots.insert(insert_index, *source_id);
                }
            }
            | Direction::Under => {
                // Add source as the last child of target
                let target_node = self.nodes.get_mut(target_id)?;
                let children = target_node.children_mut()?;
                children.push(*source_id);
            }
        }

        self.rebuild_parent_index();
        Some(())
    }

    /// Check if `ancestor` is an ancestor of `descendant` in the block tree.
    /// Searches bottom-up by walking from `descendant` to its parent chain.
    fn is_ancestor(&self, ancestor: &BlockId, descendant: &BlockId) -> bool {
        let mut current = Some(*descendant);
        while let Some(id) = current {
            if id == *ancestor {
                return true;
            }
            current = self.parent(&id);
        }
        false
    }

    /// Find the parent id and position index of a block.
    ///
    /// Returns `(None, index)` if the block is a root, or
    /// `(Some(parent_id), index)` if it is a child.
    ///
    /// Uses the cached `parent_index` for O(1) parent lookup,
    /// then O(C) position scan within the parent's children.
    pub(crate) fn parent_and_index_of(&self, target: &BlockId) -> Option<(Option<BlockId>, usize)> {
        if let Some(index) = self.roots.iter().position(|id| id == target) {
            return Some((None, index));
        }

        let parent_id = self.parent_index.get(target)?;
        let node = self.nodes.get(parent_id)?;
        let index = node.children().iter().position(|id| id == target)?;
        Some((Some(*parent_id), index))
    }

    fn clone_subtree_with_new_ids(&mut self, source_id: &BlockId) -> Option<BlockId> {
        let source_node = self.node(source_id)?.clone();
        let source_content = self.points.get(source_id).cloned().unwrap_or_default();
        let source_children: Vec<BlockId> = source_node.children().to_vec();
        let mut child_ids = Vec::with_capacity(source_children.len());
        for child in &source_children {
            child_ids.push(self.clone_subtree_with_new_ids(child)?);
        }

        let next_id = Self::insert_node(&mut self.nodes, BlockNode::with_children(child_ids));
        self.points.insert(next_id, source_content);
        if let Some(mount_point) = self.inherited_mount_point_for_anchor(source_id) {
            self.mount_table.set_origin(next_id, BlockOrigin::Mounted { mount_point });
        }
        Some(next_id)
    }

    /// Find the mount point that owns `anchor_id`, either because the anchor
    /// itself is a mount point or because it was loaded from a mounted file.
    pub(crate) fn inherited_mount_point_for_anchor(&self, anchor_id: &BlockId) -> Option<BlockId> {
        if self.mount_table.entry(*anchor_id).is_some() {
            return Some(*anchor_id);
        }

        match self.mount_table.origin(*anchor_id) {
            | Some(BlockOrigin::Mounted { mount_point }) => Some(*mount_point),
            | None => None,
        }
    }

    /// Recursively collect all block ids in the subtree rooted at `current`.
    pub(crate) fn collect_subtree_ids(&self, current: &BlockId, out: &mut Vec<BlockId>) {
        let Some(node) = self.node(current) else {
            return;
        };
        out.push(*current);
        for child in node.children() {
            self.collect_subtree_ids(child, out);
        }
    }

    /// Collect subtree IDs owned by this store, stopping at expanded mount
    /// boundaries.
    ///
    /// `own_ids` receives every block id in the subtree that is not from a
    /// nested mounted file. `mount_points` receives ids of expanded mount
    /// points encountered during traversal (they are also included in
    /// `own_ids` since the mount-point node itself belongs to this store).
    pub(crate) fn collect_own_subtree_ids(
        &self, current: &BlockId, own_ids: &mut Vec<BlockId>, mount_points: &mut Vec<BlockId>,
    ) {
        let Some(node) = self.node(current) else {
            return;
        };
        own_ids.push(*current);

        // If this node is an expanded mount, its children belong to the
        // mounted file. Record it and do not recurse.
        if self.mount_table.entry(*current).is_some() {
            mount_points.push(*current);
            return;
        }

        for child in node.children() {
            self.collect_own_subtree_ids(child, own_ids, mount_points);
        }
    }

    /// Walk the tree from `current` toward `target`, collecting each node's
    /// display text into `out`. Returns `true` if `target` was found.
    pub(crate) fn collect_lineage_points(
        &self, current: &BlockId, target: &BlockId, out: &mut Vec<String>,
    ) -> bool {
        if !self.nodes.contains_key(current) {
            return false;
        }

        let point =
            self.points.get(current).map(|pc| pc.display_text().to_owned()).unwrap_or_default();
        out.push(point);
        if current == target {
            return true;
        }

        let children = self.node(current).map(|n| n.children().to_vec()).unwrap_or_default();
        for child in &children {
            if self.collect_lineage_points(child, target, out) {
                return true;
            }
        }

        out.pop();
        false
    }

    /// Remove all friend-block entries that reference any id in `removed_ids`.
    pub(crate) fn remove_friend_block_references(&mut self, removed_ids: &[BlockId]) {
        if removed_ids.is_empty() || self.friend_blocks.is_empty() {
            return;
        }
        let removed = removed_ids.iter().copied().collect::<std::collections::HashSet<_>>();
        let target_ids = self.friend_blocks.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let mut empty_targets = Vec::new();
        for target_id in target_ids {
            if let Some(friend_ids) = self.friend_blocks.get_mut(&target_id) {
                friend_ids.retain(|friend| !removed.contains(&friend.block_id));
                if friend_ids.is_empty() {
                    empty_targets.push(target_id);
                }
            }
        }
        for target_id in empty_targets {
            self.friend_blocks.remove(&target_id);
        }
    }
}
