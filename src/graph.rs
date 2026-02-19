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
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// One node in the block graph: a text point and ordered child ids.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockNode {
    pub point: String,
    pub children: Vec<BlockId>,
}

impl BlockNode {
    pub fn new(point: impl ToString, children: Vec<BlockId>) -> Self {
        Self { point: point.to_string(), children }
    }
}

/// Forest of blocks: root ids and a map from block id to node.
///
/// Invariant: every id in `roots` and in any node's `children` must exist as
/// a key in `nodes`. The graph always has at least one root.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockGraph {
    roots: Vec<BlockId>,
    nodes: HashMap<BlockId, BlockNode>,
}

impl BlockGraph {
    pub fn new(roots: Vec<BlockId>, nodes: HashMap<BlockId, BlockNode>) -> Self {
        Self { roots, nodes }
    }

    pub fn roots(&self) -> &[BlockId] {
        &self.roots
    }

    pub fn load() -> Self {
        let Some(path) = AppPaths::data_file() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            | Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            | Err(_) => Self::default(),
        }
    }

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

    pub fn node(&self, id: &BlockId) -> Option<&BlockNode> {
        self.nodes.get(id)
    }

    pub fn point(&self, id: &BlockId) -> Option<String> {
        self.node(id).map(|node| node.point.clone())
    }

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

    fn collect_subtree_ids(&self, current: &BlockId, out: &mut Vec<BlockId>) {
        let Some(node) = self.node(current) else {
            return;
        };
        out.push(current.clone());
        for child in &node.children {
            self.collect_subtree_ids(child, out);
        }
    }

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
