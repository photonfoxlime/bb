//! Block graph: the core document data model.
//!
//! A document is a forest of blocks. Each block has a UUID identity, a text
//! point, and ordered children. The graph is serialized as JSON for persistence.

use crate::llm;
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io};
use uuid::Uuid;

/// Unique id for a block in the graph.
///
/// Invariant: always a valid UUID. Constructed only via `BlockId::new`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(Uuid);

impl BlockId {
    /// Generate a fresh, random block id (UUID v4).
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// One node in the block graph: a text point and ordered child ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockNode {
    pub point: String,
    pub children: Vec<BlockId>,
}

impl BlockNode {
    /// Create a node with the given text point and child ids.
    pub fn new(point: impl ToString, children: Vec<BlockId>) -> Self {
        Self { point: point.to_string(), children }
    }
}

/// Forest of blocks: root ids and a map from block id to node.
///
/// Invariant: every id in `roots` and in any node's `children` must exist as
/// a key in `nodes`. The graph always has at least one root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockGraph {
    roots: Vec<BlockId>,
    nodes: HashMap<BlockId, BlockNode>,
}

impl BlockGraph {
    /// Construct a graph from pre-built roots and nodes.
    ///
    /// Caller must ensure every id in `roots` and in each node's `children`
    /// exists as a key in `nodes`.
    pub fn new(roots: Vec<BlockId>, nodes: HashMap<BlockId, BlockNode>) -> Self {
        Self { roots, nodes }
    }

    /// The ordered root block ids.
    pub fn roots(&self) -> &[BlockId] {
        &self.roots
    }

    /// Load the graph from the app data file, falling back to the default demo graph.
    pub fn load() -> Self {
        let Some(path) = AppPaths::data_file() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            | Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            | Err(_) => Self::default(),
        }
    }

    /// Persist the graph as pretty-printed JSON to the app data file.
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = AppPaths::data_file() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, contents)
    }

    /// Look up a node by id.
    pub fn node(&self, id: &BlockId) -> Option<&BlockNode> {
        self.nodes.get(id)
    }

    /// Return the text point of a block, or `None` if the id is unknown.
    pub fn point(&self, id: &BlockId) -> Option<String> {
        self.node(id).map(|node| node.point.clone())
    }

    /// Overwrite the text point of an existing block. No-op if `id` is unknown.
    pub fn update_point(&mut self, id: &BlockId, value: String) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.point = value;
        }
    }

    /// Add one child block under the parent and return the new child id.
    pub fn append_child(&mut self, parent_id: &BlockId, point: String) -> Option<BlockId> {
        if !self.nodes.contains_key(parent_id) {
            return None;
        }

        let child_id = BlockId::new();
        self.nodes.insert(child_id.clone(), BlockNode::new(point, vec![]));
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            parent.children.push(child_id.clone());
        }
        Some(child_id)
    }

    /// Insert an empty sibling block immediately after `block_id` in its parent's
    /// child list (or in roots if `block_id` is a root). Returns the new id.
    pub fn append_sibling(&mut self, block_id: &BlockId, point: String) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let sibling_id = BlockId::new();
        self.nodes.insert(sibling_id.clone(), BlockNode::new(point, vec![]));

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            parent.children.insert(index + 1, sibling_id.clone());
        } else {
            self.roots.insert(index + 1, sibling_id.clone());
        }
        Some(sibling_id)
    }

    /// Deep-clone a block and its entire subtree with fresh ids, inserting the
    /// copy immediately after the original. Returns the cloned root id.
    pub fn duplicate_subtree_after(&mut self, block_id: &BlockId) -> Option<BlockId> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        let duplicate_id = self.clone_subtree_with_new_ids(block_id)?;

        if let Some(parent_id) = parent_id {
            let parent = self.nodes.get_mut(&parent_id)?;
            parent.children.insert(index + 1, duplicate_id.clone());
        } else {
            self.roots.insert(index + 1, duplicate_id.clone());
        }
        Some(duplicate_id)
    }

    /// Remove a block and its entire subtree. Returns the removed ids.
    ///
    /// If removal empties the root list, a fresh empty root is inserted.
    pub fn remove_block_subtree(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>> {
        let (parent_id, index) = self.parent_and_index_of(block_id)?;
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.children.remove(index);
            }
        } else {
            self.roots.remove(index);
        }

        let mut removed_ids = Vec::new();
        self.collect_subtree_ids(block_id, &mut removed_ids);
        for id in &removed_ids {
            self.nodes.remove(id);
        }

        if self.roots.is_empty() {
            let root_id = BlockId::new();
            self.nodes.insert(root_id.clone(), BlockNode::new(String::new(), vec![]));
            self.roots.push(root_id);
        }

        Some(removed_ids)
    }

    /// Return lineage points from one root to the target id (DFS).
    pub fn lineage_points_for_id(&self, target: &BlockId) -> llm::Lineage {
        for root in &self.roots {
            let mut points = Vec::new();
            if self.collect_lineage_points(root, target, &mut points) {
                return llm::Lineage::from_points(points);
            }
        }
        llm::Lineage::from_points(vec![])
    }

    /// Find the parent id (or `None` for roots) and the child-list index of `target`.
    fn parent_and_index_of(&self, target: &BlockId) -> Option<(Option<BlockId>, usize)> {
        if let Some(index) = self.roots.iter().position(|id| id == target) {
            return Some((None, index));
        }

        for (parent_id, node) in &self.nodes {
            if let Some(index) = node.children.iter().position(|id| id == target) {
                return Some((Some(parent_id.clone()), index));
            }
        }
        None
    }

    /// Recursively clone a subtree, assigning fresh ids to every node.
    fn clone_subtree_with_new_ids(&mut self, source_id: &BlockId) -> Option<BlockId> {
        let source_node = self.node(source_id)?.clone();
        let mut child_ids = Vec::with_capacity(source_node.children.len());
        for child in source_node.children {
            child_ids.push(self.clone_subtree_with_new_ids(&child)?);
        }

        let next_id = BlockId::new();
        self.nodes.insert(next_id.clone(), BlockNode::new(source_node.point, child_ids));
        Some(next_id)
    }

    /// Collect all ids reachable from `current` (inclusive) via DFS.
    fn collect_subtree_ids(&self, current: &BlockId, out: &mut Vec<BlockId>) {
        let Some(node) = self.node(current) else {
            return;
        };
        out.push(current.clone());
        for child in &node.children {
            self.collect_subtree_ids(child, out);
        }
    }

    /// DFS helper: accumulate ancestor points from `current` toward `target`.
    /// Returns `true` when the target is found and `points` contains the full
    /// root-to-target path.
    fn collect_lineage_points(
        &self, current: &BlockId, target: &BlockId, points: &mut Vec<String>,
    ) -> bool {
        let Some(node) = self.node(current) else {
            return false;
        };

        points.push(node.point.clone());
        if current == target {
            return true;
        }

        for child in &node.children {
            if self.collect_lineage_points(child, target, points) {
                return true;
            }
        }

        points.pop();
        false
    }

    /// Build the built-in demo graph used when no data file exists.
    fn default_graph() -> Self {
        let root_id = BlockId::new();
        let child_ids = [BlockId::new(), BlockId::new(), BlockId::new()];
        let mut nodes = HashMap::new();
        nodes.insert(child_ids[0].clone(), BlockNode::new("马克思：《资本论》", vec![]));
        nodes.insert(
            child_ids[1].clone(),
            BlockNode::new("马克思·韦伯：《新教伦理与资本主义精神》", vec![]),
        );
        nodes.insert(
            child_ids[2].clone(),
            BlockNode::new("Ivan Zhao: Steam, Steel, and Invisible Minds", vec![]),
        );
        nodes.insert(
            root_id.clone(),
            BlockNode::new("Notes on liberating productivity", child_ids.to_vec()),
        );
        BlockGraph::new(vec![root_id], nodes)
    }
}

impl Default for BlockGraph {
    fn default() -> Self {
        Self::default_graph()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a simple graph: one root with two children.
    ///
    /// ```text
    /// root("root")
    /// ├── child_a("child_a")
    /// └── child_b("child_b")
    /// ```
    fn simple_graph() -> (BlockGraph, BlockId, BlockId, BlockId) {
        let root = BlockId::new();
        let child_a = BlockId::new();
        let child_b = BlockId::new();
        let mut nodes = HashMap::new();
        nodes.insert(child_a.clone(), BlockNode::new("child_a", vec![]));
        nodes.insert(child_b.clone(), BlockNode::new("child_b", vec![]));
        nodes.insert(root.clone(), BlockNode::new("root", vec![child_a.clone(), child_b.clone()]));
        let graph = BlockGraph::new(vec![root.clone()], nodes);
        (graph, root, child_a, child_b)
    }

    // -- BlockId --

    #[test]
    fn block_id_new_produces_distinct_ids() {
        let a = BlockId::new();
        let b = BlockId::new();
        assert_ne!(a, b);
    }

    // -- BlockNode --

    #[test]
    fn block_node_stores_point_and_children() {
        let child = BlockId::new();
        let node = BlockNode::new("hello", vec![child.clone()]);
        assert_eq!(node.point, "hello");
        assert_eq!(node.children, vec![child]);
    }

    // -- Graph accessors --

    #[test]
    fn node_returns_some_for_existing_id() {
        let (graph, root, _, _) = simple_graph();
        assert!(graph.node(&root).is_some());
    }

    #[test]
    fn node_returns_none_for_unknown_id() {
        let (graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        assert!(graph.node(&unknown).is_none());
    }

    #[test]
    fn point_returns_text_for_known_id() {
        let (graph, root, _, _) = simple_graph();
        assert_eq!(graph.point(&root), Some("root".to_string()));
    }

    #[test]
    fn roots_returns_root_list() {
        let (graph, root, _, _) = simple_graph();
        assert_eq!(graph.roots(), &[root]);
    }

    // -- update_point --

    #[test]
    fn update_point_changes_existing_node() {
        let (mut graph, root, _, _) = simple_graph();
        graph.update_point(&root, "updated".to_string());
        assert_eq!(graph.point(&root), Some("updated".to_string()));
    }

    #[test]
    fn update_point_noop_for_unknown_id() {
        let (mut graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        graph.update_point(&unknown, "nope".to_string());
    }

    // -- append_child --

    #[test]
    fn append_child_returns_new_id() {
        let (mut graph, root, _, _) = simple_graph();
        let child_id = graph.append_child(&root, "new_child".to_string());
        assert!(child_id.is_some());
    }

    #[test]
    fn append_child_node_exists_with_point() {
        let (mut graph, root, _, _) = simple_graph();
        let child_id = graph.append_child(&root, "new_child".to_string()).unwrap();
        assert_eq!(graph.point(&child_id), Some("new_child".to_string()));
    }

    #[test]
    fn append_child_appears_in_parent_children() {
        let (mut graph, root, child_a, child_b) = simple_graph();
        let child_id = graph.append_child(&root, "new_child".to_string()).unwrap();
        let parent = graph.node(&root).unwrap();
        assert_eq!(parent.children, vec![child_a, child_b, child_id]);
    }

    #[test]
    fn append_child_returns_none_for_unknown_parent() {
        let (mut graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        assert_eq!(graph.append_child(&unknown, "x".to_string()), None);
    }

    // -- append_sibling --

    #[test]
    fn append_sibling_after_root() {
        let (mut graph, root, _, _) = simple_graph();
        let sibling = graph.append_sibling(&root, "sibling".to_string()).unwrap();
        assert_eq!(graph.roots(), &[root, sibling]);
    }

    #[test]
    fn append_sibling_after_non_root() {
        let (mut graph, root, child_a, child_b) = simple_graph();
        let sibling = graph.append_sibling(&child_a, "mid".to_string()).unwrap();
        let parent = graph.node(&root).unwrap();
        assert_eq!(parent.children, vec![child_a, sibling, child_b]);
    }

    #[test]
    fn append_sibling_returns_none_for_unknown() {
        let (mut graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        assert_eq!(graph.append_sibling(&unknown, "x".to_string()), None);
    }

    // -- duplicate_subtree_after --

    #[test]
    fn duplicate_leaf_appears_after_original() {
        let (mut graph, root, child_a, child_b) = simple_graph();
        let dup = graph.duplicate_subtree_after(&child_a).unwrap();
        let parent = graph.node(&root).unwrap();
        assert_eq!(parent.children, vec![child_a, dup.clone(), child_b]);
        assert_eq!(graph.point(&dup), Some("child_a".to_string()));
    }

    #[test]
    fn duplicate_subtree_clones_descendants() {
        let (mut graph, _root, child_a, _) = simple_graph();
        let grandchild = graph.append_child(&child_a, "grandchild".to_string()).unwrap();

        let dup = graph.duplicate_subtree_after(&child_a).unwrap();
        let dup_node = graph.node(&dup).unwrap();
        assert_eq!(dup_node.children.len(), 1);
        let cloned_grandchild = &dup_node.children[0];
        assert_ne!(cloned_grandchild, &grandchild);
        assert_eq!(graph.point(cloned_grandchild), Some("grandchild".to_string()));

        let orig = graph.node(&child_a).unwrap();
        assert_eq!(orig.children, vec![grandchild]);
    }

    #[test]
    fn duplicate_returns_none_for_unknown() {
        let (mut graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        assert_eq!(graph.duplicate_subtree_after(&unknown), None);
    }

    // -- remove_block_subtree --

    #[test]
    fn remove_leaf_child_shrinks_parent() {
        let (mut graph, root, child_a, child_b) = simple_graph();
        let removed = graph.remove_block_subtree(&child_a).unwrap();
        assert_eq!(removed, vec![child_a]);
        let parent = graph.node(&root).unwrap();
        assert_eq!(parent.children, vec![child_b]);
    }

    #[test]
    fn remove_subtree_removes_all_descendants() {
        let (mut graph, _, child_a, _) = simple_graph();
        let grandchild = graph.append_child(&child_a, "gc".to_string()).unwrap();
        let removed = graph.remove_block_subtree(&child_a).unwrap();
        assert!(removed.contains(&child_a));
        assert!(removed.contains(&grandchild));
        assert!(graph.node(&child_a).is_none());
        assert!(graph.node(&grandchild).is_none());
    }

    #[test]
    fn remove_last_root_inserts_fresh_root() {
        let id = BlockId::new();
        let mut nodes = HashMap::new();
        nodes.insert(id.clone(), BlockNode::new("only", vec![]));
        let mut graph = BlockGraph::new(vec![id.clone()], nodes);

        graph.remove_block_subtree(&id).unwrap();
        assert_eq!(graph.roots().len(), 1);
        let new_root = &graph.roots()[0];
        assert_ne!(new_root, &id);
        assert_eq!(graph.point(new_root), Some(String::new()));
    }

    #[test]
    fn remove_returns_none_for_unknown() {
        let (mut graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        assert_eq!(graph.remove_block_subtree(&unknown), None);
    }

    // -- lineage_points_for_id --

    #[test]
    fn lineage_root_to_deep_child() {
        let (mut graph, _, child_a, _) = simple_graph();
        let grandchild = graph.append_child(&child_a, "gc".to_string()).unwrap();
        let lineage = graph.lineage_points_for_id(&grandchild);
        let expected = llm::Lineage::from_points(vec![
            "root".to_string(),
            "child_a".to_string(),
            "gc".to_string(),
        ]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_for_root_is_single_element() {
        let (graph, root, _, _) = simple_graph();
        let lineage = graph.lineage_points_for_id(&root);
        let expected = llm::Lineage::from_points(vec!["root".to_string()]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_for_unknown_is_empty() {
        let (graph, _, _, _) = simple_graph();
        let unknown = BlockId::new();
        let lineage = graph.lineage_points_for_id(&unknown);
        let expected = llm::Lineage::from_points(vec![]);
        assert_eq!(lineage, expected);
    }

    // -- Serialization round-trip --

    #[test]
    fn serde_round_trip_preserves_graph() {
        let (graph, _, _, _) = simple_graph();
        let json = serde_json::to_string(&graph).unwrap();
        let restored: BlockGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(graph, restored);
    }
}
