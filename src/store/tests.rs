use super::{BlockId, BlockNode, BlockStore, BlockStoreNavigateExt as _, FriendBlock};
use crate::llm;
use rustc_hash::FxHashMap;

fn alloc_test_id<T>(map: &FxHashMap<BlockId, T>) -> BlockId {
    loop {
        let id = BlockId::new_v7();
        if !map.contains_key(&id) {
            return id;
        }
    }
}

fn insert_node(nodes: &mut FxHashMap<BlockId, BlockNode>, node: BlockNode) -> BlockId {
    let id = alloc_test_id(nodes);
    std::collections::HashMap::insert(nodes, id, node);
    id
}

fn simple_store() -> (BlockStore, BlockId, BlockId, BlockId) {
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();

    let child_a = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    points.insert(child_a, "child_a".to_string());
    let child_b = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    points.insert(child_b, "child_b".to_string());
    let root = insert_node(&mut nodes, BlockNode::with_children(vec![child_a, child_b]));
    points.insert(root, "root".to_string());

    let store = BlockStore::new(vec![root], nodes, points);
    (store, root, child_a, child_b)
}

#[test]
fn find_block_point_is_case_insensitive_and_dfs_ordered() {
    let (mut store, root, child_a, child_b) = simple_store();
    store.update_point(&root, "ROOT child marker".to_string());

    let matches = store.find_block_point("ChIlD");
    assert_eq!(matches, vec![root, child_a, child_b]);
}

#[test]
fn find_block_point_uses_phrase_tokenization_for_mixed_query() {
    let (mut store, _root, child_a, child_b) = simple_store();
    store.update_point(&child_a, "想买 显卡 RTX-4090 on macOS".to_string());
    store.update_point(&child_b, "普通办公本".to_string());

    let matches = store.find_block_point("RTX-4090,macOS");
    assert_eq!(matches, vec![child_a]);
}

#[test]
fn lineage_root_to_deep_child() {
    let (mut store, _, child_a, _) = simple_store();
    let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();
    let lineage = store.lineage_points_for_id(&grandchild);
    let expected = llm::LineageContext::from_points(vec![
        "root".to_string(),
        "child_a".to_string(),
        "gc".to_string(),
    ]);
    assert_eq!(lineage, expected);
}

#[test]
fn lineage_for_unknown_is_empty() {
    let (store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    let lineage = store.lineage_points_for_id(&unknown);
    assert_eq!(lineage, llm::LineageContext::from_points(vec![]));
}

#[test]
fn block_context_with_friend_blocks_skips_unknown_ids() {
    let (store, root, child_a, _) = simple_store();
    let unknown = BlockId::default();
    let context = store.block_context_for_id_with_friend_blocks(
        &root,
        &[
            FriendBlock {
                block_id: unknown,
                perspective: None,
                parent_lineage_telescope: true,
                children_telescope: true,
            },
            FriendBlock {
                block_id: child_a,
                perspective: Some("supporting lens".to_string()),
                ..Default::default()
            },
        ],
    );
    let friend_blocks = context.friend_blocks();
    assert_eq!(friend_blocks.len(), 1);
    assert_eq!(friend_blocks[0].point(), "child_a");
    assert_eq!(friend_blocks[0].perspective(), Some("supporting lens"));
}

#[test]
fn block_context_uses_persisted_friend_blocks_for_target() {
    let (mut store, root, child_a, child_b) = simple_store();
    store.set_friend_blocks_for(
        &root,
        vec![
            FriendBlock {
                block_id: child_a,
                perspective: Some("historical precedent".to_string()),
                ..Default::default()
            },
            FriendBlock { block_id: child_b, perspective: None, ..Default::default() },
        ],
    );

    let context = store.block_context_for_id(&root);
    let friend_blocks = context.friend_blocks();
    assert_eq!(friend_blocks.len(), 2);
    assert_eq!(friend_blocks[0].point(), "child_a");
    assert_eq!(friend_blocks[0].perspective(), Some("historical precedent"));
    assert_eq!(friend_blocks[1].point(), "child_b");
    assert_eq!(friend_blocks[1].perspective(), None);
}

#[test]
fn next_visible_in_dfs_skips_collapsed_subtrees() {
    let (mut store, root, child_a, child_b) = simple_store();
    let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();
    store.toggle_collapsed(&child_a);

    assert_eq!(store.next_visible_in_dfs(&root), Some(child_a));
    assert_eq!(store.next_visible_in_dfs(&child_a), Some(child_b));
    assert_eq!(store.next_visible_in_dfs(&grandchild), Some(child_b));
}

#[test]
fn prev_visible_in_dfs_skips_collapsed_subtrees() {
    let (mut store, _root, child_a, child_b) = simple_store();
    let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();
    store.toggle_collapsed(&child_a);

    assert_eq!(store.prev_visible_in_dfs(&child_b), Some(child_a));
    assert_eq!(store.prev_visible_in_dfs(&grandchild), Some(child_a));
}

#[test]
fn is_visible_requires_all_ancestors_expanded() {
    let (mut store, _root, child_a, _child_b) = simple_store();
    let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();

    assert!(store.is_visible(&grandchild));
    store.toggle_collapsed(&child_a);
    assert!(!store.is_visible(&grandchild));
}
