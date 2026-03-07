use super::mount::MountError;
use super::*;
use crate::llm;
use rustc_hash::FxHashMap;
use std::fs;

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

fn insert_unit(values: &mut FxHashMap<BlockId, ()>) -> BlockId {
    let id = alloc_test_id(values);
    std::collections::HashMap::insert(values, id, ());
    id
}

/// Build a simple store: one root with two children.
///
/// ```text
/// root("root")
/// ├── child_a("child_a")
/// └── child_b("child_b")
/// ```
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

/// Allocate `count` distinct `BlockId`s for unit tests that need ids
/// without building an entire `BlockStore`.
fn make_ids(count: usize) -> Vec<BlockId> {
    let mut sm: FxHashMap<BlockId, ()> = FxHashMap::default();
    (0..count).map(|_| insert_unit(&mut sm)).collect()
}

// -- BlockId --

#[test]
fn block_id_new_produces_distinct_ids() {
    let mut nodes: FxHashMap<BlockId, BlockNode> = FxHashMap::default();
    let a = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    let b = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    assert_ne!(a, b);
}

// -- BlockNode --

#[test]
fn block_node_stores_children() {
    let child = BlockId::default();
    let node = BlockNode::with_children(vec![child]);
    assert_eq!(node.children(), &[child]);
}

// -- Store accessors --

#[test]
fn node_returns_some_for_existing_id() {
    let (store, root, _, _) = simple_store();
    assert!(store.node(&root).is_some());
}

#[test]
fn node_returns_none_for_unknown_id() {
    let (store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    assert!(store.node(&unknown).is_none());
}

#[test]
fn point_returns_text_for_known_id() {
    let (store, root, _, _) = simple_store();
    assert_eq!(store.point(&root), Some("root".to_string()));
}

#[test]
fn roots_returns_root_list() {
    let (store, root, _, _) = simple_store();
    assert_eq!(store.roots(), &[root]);
}

#[test]
fn find_block_point_empty_query_matches_all_in_dfs_order() {
    let (store, root, child_a, child_b) = simple_store();
    assert_eq!(store.find_block_point("   "), vec![root, child_a, child_b]);
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
fn find_block_point_returns_empty_when_no_match_exists() {
    let (store, _, _, _) = simple_store();
    assert!(store.find_block_point("nonexistent-keyword").is_empty());
}

#[test]
fn find_block_point_empty_query_includes_deep_descendants_in_dfs_order() {
    let (mut store, root, child_a, child_b) = simple_store();
    let grandchild = store.append_child(&child_a, "nested".to_string()).unwrap();

    assert_eq!(store.find_block_point(""), vec![root, child_a, grandchild, child_b]);
}

#[test]
fn find_block_point_handles_multiple_roots_in_order() {
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();

    let child_a = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    points.insert(child_a, "alpha child".to_string());
    let root_a = insert_node(&mut nodes, BlockNode::with_children(vec![child_a]));
    points.insert(root_a, "alpha root".to_string());

    let child_b = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    points.insert(child_b, "beta child".to_string());
    let root_b = insert_node(&mut nodes, BlockNode::with_children(vec![child_b]));
    points.insert(root_b, "beta root".to_string());

    let store = BlockStore::new(vec![root_a, root_b], nodes, points);
    assert_eq!(store.find_block_point(""), vec![root_a, child_a, root_b, child_b]);
}

#[test]
fn find_block_point_uses_full_query_fallback_when_no_phrase_tokens() {
    let (mut store, _root, child_a, _child_b) = simple_store();
    store.update_point(&child_a, "hello😀world".to_string());

    assert_eq!(store.find_block_point("😀"), vec![child_a]);
}

#[test]
fn find_block_point_does_not_duplicate_when_multiple_rules_match() {
    let (mut store, root, _child_a, _child_b) = simple_store();
    store.update_point(&root, "alpha,beta".to_string());

    assert_eq!(store.find_block_point("alpha,beta"), vec![root]);
}

#[test]
fn find_block_point_matches_chinese_substring() {
    let (mut store, _root, child_a, child_b) = simple_store();
    store.update_point(&child_a, "我想买显卡".to_string());
    store.update_point(&child_b, "我想买键盘".to_string());

    assert_eq!(store.find_block_point("显卡"), vec![child_a]);
}

#[test]
fn find_block_point_handles_chinese_boundary_query() {
    let (mut store, _root, child_a, child_b) = simple_store();
    store.update_point(&child_a, "升级显卡".to_string());
    store.update_point(&child_b, "升级电源".to_string());

    assert_eq!(store.find_block_point("显卡，电源"), vec![child_a, child_b]);
}

#[test]
fn find_block_point_matches_chinese_and_latin_mixed_query() {
    let (mut store, _root, child_a, child_b) = simple_store();
    store.update_point(&child_a, "购买 RTX-4090 显卡".to_string());
    store.update_point(&child_b, "购买普通显示器".to_string());

    assert_eq!(store.find_block_point("RTX-4090，显卡"), vec![child_a]);
}

#[test]
fn find_block_point_matches_japanese_hiragana_by_full_query_fallback() {
    let (mut store, _root, child_a, _child_b) = simple_store();
    store.update_point(&child_a, "きょうはねこを見た".to_string());

    assert_eq!(store.find_block_point("ねこ"), vec![child_a]);
}

#[test]
fn find_block_point_matches_japanese_katakana_by_full_query_fallback() {
    let (mut store, _root, child_a, _child_b) = simple_store();
    store.update_point(&child_a, "ノートPCが欲しい".to_string());

    assert_eq!(store.find_block_point("ノート"), vec![child_a]);
}

#[test]
fn find_block_point_matches_japanese_punctuation_query_by_full_query_fallback() {
    let (mut store, _root, child_a, child_b) = simple_store();
    store.update_point(&child_a, "ねこ、いぬ".to_string());
    store.update_point(&child_b, "ねこといぬ".to_string());

    assert_eq!(store.find_block_point("ねこ、いぬ"), vec![child_a]);
}

// -- update_point --

#[test]
fn update_point_changes_existing_node() {
    let (mut store, root, _, _) = simple_store();
    store.update_point(&root, "updated".to_string());
    assert_eq!(store.point(&root), Some("updated".to_string()));
}

#[test]
fn update_point_noop_for_unknown_id() {
    let (mut store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    store.update_point(&unknown, "nope".to_string());
}

// -- append_child --

#[test]
fn append_child_returns_new_id() {
    let (mut store, root, _, _) = simple_store();
    let child_id = store.append_child(&root, "new_child".to_string());
    assert!(child_id.is_some());
}

#[test]
fn append_child_node_exists_with_point() {
    let (mut store, root, _, _) = simple_store();
    let child_id = store.append_child(&root, "new_child".to_string()).unwrap();
    assert_eq!(store.point(&child_id), Some("new_child".to_string()));
}

#[test]
fn append_child_appears_in_parent_children() {
    let (mut store, root, child_a, child_b) = simple_store();
    let child_id = store.append_child(&root, "new_child".to_string()).unwrap();
    let parent = store.node(&root).unwrap();
    assert_eq!(parent.children(), &[child_a, child_b, child_id]);
}

#[test]
fn append_child_returns_none_for_unknown_parent() {
    let (mut store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    assert_eq!(store.append_child(&unknown, "x".to_string()), None);
}

// -- append_sibling --

#[test]
fn append_sibling_after_root() {
    let (mut store, root, _, _) = simple_store();
    let sibling = store.append_sibling(&root, "sibling".to_string()).unwrap();
    assert_eq!(store.roots(), &[root, sibling]);
}

#[test]
fn append_sibling_after_non_root() {
    let (mut store, root, child_a, child_b) = simple_store();
    let sibling = store.append_sibling(&child_a, "mid".to_string()).unwrap();
    let parent = store.node(&root).unwrap();
    assert_eq!(parent.children(), &[child_a, sibling, child_b]);
}

#[test]
fn append_sibling_returns_none_for_unknown() {
    let (mut store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    assert_eq!(store.append_sibling(&unknown, "x".to_string()), None);
}

#[test]
fn insert_parent_wraps_non_root_block() {
    let (mut store, root, child_a, child_b) = simple_store();

    let inserted = store.insert_parent(&child_a, "new_parent".to_string()).unwrap();

    assert_eq!(store.point(&inserted), Some("new_parent".to_string()));
    let root_node = store.node(&root).unwrap();
    assert_eq!(root_node.children(), &[inserted, child_b]);
    let inserted_node = store.node(&inserted).unwrap();
    assert_eq!(inserted_node.children(), &[child_a]);
}

#[test]
fn insert_parent_wraps_root_block() {
    let (mut store, root, _child_a, _child_b) = simple_store();

    let inserted = store.insert_parent(&root, "new_root_parent".to_string()).unwrap();

    assert_eq!(store.roots(), &[inserted]);
    let inserted_node = store.node(&inserted).unwrap();
    assert_eq!(inserted_node.children(), &[root]);
}

#[test]
fn insert_parent_returns_none_for_unknown_block() {
    let (mut store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    assert_eq!(store.insert_parent(&unknown, "x".to_string()), None);
}

// -- duplicate_subtree_after --

#[test]
fn duplicate_leaf_appears_after_original() {
    let (mut store, root, child_a, child_b) = simple_store();
    let dup = store.duplicate_subtree_after(&child_a).unwrap();
    let parent = store.node(&root).unwrap();
    assert_eq!(parent.children(), &[child_a, dup, child_b]);
    assert_eq!(store.point(&dup), Some("child_a".to_string()));
}

#[test]
fn duplicate_subtree_clones_descendants() {
    let (mut store, _root, child_a, _) = simple_store();
    let grandchild = store.append_child(&child_a, "grandchild".to_string()).unwrap();

    let dup = store.duplicate_subtree_after(&child_a).unwrap();
    let dup_node = store.node(&dup).unwrap();
    assert_eq!(dup_node.children().len(), 1);
    let cloned_grandchild = &dup_node.children()[0];
    assert_ne!(cloned_grandchild, &grandchild);
    assert_eq!(store.point(cloned_grandchild), Some("grandchild".to_string()));

    let orig = store.node(&child_a).unwrap();
    assert_eq!(orig.children(), &[grandchild]);
}

#[test]
fn duplicate_returns_none_for_unknown() {
    let (mut store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    assert_eq!(store.duplicate_subtree_after(&unknown), None);
}

// -- remove_block_subtree --

#[test]
fn remove_leaf_child_shrinks_parent() {
    let (mut store, root, child_a, child_b) = simple_store();
    let removed = store.remove_block_subtree(&child_a).unwrap();
    assert_eq!(removed, vec![child_a]);
    let parent = store.node(&root).unwrap();
    assert_eq!(parent.children(), &[child_b]);
}

#[test]
fn remove_subtree_removes_all_descendants() {
    let (mut store, _, child_a, _) = simple_store();
    let grandchild = store.append_child(&child_a, "gc".to_string()).unwrap();
    let removed = store.remove_block_subtree(&child_a).unwrap();
    assert!(removed.contains(&child_a));
    assert!(removed.contains(&grandchild));
    assert!(store.node(&child_a).is_none());
    assert!(store.node(&grandchild).is_none());
}

#[test]
fn remove_last_root_inserts_fresh_root() {
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let id = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    points.insert(id, "only".to_string());
    let mut store = BlockStore::new(vec![id], nodes, points);

    store.remove_block_subtree(&id).unwrap();
    assert_eq!(store.roots().len(), 1);
    let new_root = store.roots()[0];
    assert_ne!(new_root, id);
    assert_eq!(store.point(&new_root), Some(String::new()));
}

#[test]
fn remove_returns_none_for_unknown() {
    let (mut store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    assert_eq!(store.remove_block_subtree(&unknown), None);
}

// -- lineage_points_for_id --

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
fn lineage_for_root_is_single_element() {
    let (store, root, _, _) = simple_store();
    let lineage = store.lineage_points_for_id(&root);
    let expected = llm::LineageContext::from_points(vec!["root".to_string()]);
    assert_eq!(lineage, expected);
}

#[test]
fn lineage_for_unknown_is_empty() {
    let (store, _, _, _) = simple_store();
    let unknown = BlockId::default();
    let lineage = store.lineage_points_for_id(&unknown);
    let expected = llm::LineageContext::from_points(vec![]);
    assert_eq!(lineage, expected);
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

// -- Serialization round-trip --

#[test]
fn serde_round_trip_preserves_store() {
    let (store, _, _, _) = simple_store();
    let json = serde_json::to_string(&store).unwrap();
    let restored: BlockStore = serde_json::from_str(&json).unwrap();
    assert_eq!(store, restored);
}

#[test]
fn serde_round_trip_preserves_persisted_drafts() {
    let (mut store, root, child_a, _) = simple_store();
    store.amplification_drafts.insert(
        root,
        AmplificationDraftRecord {
            rewrite: Some("rewrite".to_string()),
            children: vec!["child suggestion".to_string()],
        },
    );
    store.distillation_drafts.insert(
        child_a,
        DistillationDraftRecord {
            reduction: Some("reduction".to_string()),
            redundant_children: vec![],
        },
    );
    store.set_instruction_draft(root, "instruction".to_string());
    store.set_probe_response(child_a, "inquiry".to_string());

    let json = serde_json::to_string(&store).unwrap();
    let restored: BlockStore = serde_json::from_str(&json).unwrap();

    assert_eq!(store, restored);
    assert!(restored.amplification_draft(&root).is_some());
    assert!(restored.distillation_draft(&child_a).is_some());
    assert_eq!(
        restored.instruction_draft(&root).map(|draft| draft.instruction.as_str()),
        Some("instruction")
    );
    assert_eq!(
        restored.probe_draft(&child_a).map(|draft| draft.response.as_str()),
        Some("inquiry")
    );
}

#[test]
fn set_probe_question_replaces_question_and_clears_old_response() {
    let (mut store, root, _child_a, _child_b) = simple_store();
    store.set_probe_response(root, "old response".to_string());

    store.set_probe_question(root, "new question".to_string());

    let draft = store.probe_draft(&root).expect("inquiry draft exists");
    assert_eq!(draft.inquiry, "new question");
    assert!(draft.response.is_empty());
}

#[test]
fn append_inquiry_response_chunk_builds_incremental_response() {
    let (mut store, root, _child_a, _child_b) = simple_store();
    store.set_probe_question(root, "question".to_string());

    store.append_inquiry_response_chunk(root, "hello ");
    store.append_inquiry_response_chunk(root, "world");

    let draft = store.probe_draft(&root).expect("inquiry draft exists");
    assert_eq!(draft.inquiry, "question");
    assert_eq!(draft.response, "hello world");
}

#[test]
fn remove_subtree_cleans_persisted_drafts() {
    let (mut store, _root, child_a, child_b) = simple_store();
    store.amplification_drafts.insert(
        child_a,
        AmplificationDraftRecord { rewrite: None, children: vec!["draft".to_string()] },
    );
    store.distillation_drafts.insert(
        child_b,
        DistillationDraftRecord {
            reduction: Some("draft".to_string()),
            redundant_children: vec![],
        },
    );
    store.set_instruction_draft(child_a, "instruction draft".to_string());
    store.set_probe_response(child_b, "inquiry draft".to_string());

    store.remove_block_subtree(&child_a).unwrap();
    store.remove_block_subtree(&child_b).unwrap();

    assert!(store.amplification_draft(&child_a).is_none());
    assert!(store.distillation_draft(&child_b).is_none());
    assert!(store.instruction_draft(&child_a).is_none());
    assert!(store.probe_draft(&child_b).is_none());
}

#[test]
fn backward_compat_missing_draft_fields_defaults_empty() {
    let (store, _, _, _) = simple_store();
    let mut value = serde_json::to_value(&store).unwrap();
    value.as_object_mut().unwrap().remove("amplification_drafts");
    value.as_object_mut().unwrap().remove("reduction_drafts");
    value.as_object_mut().unwrap().remove("instruction_drafts");
    value.as_object_mut().unwrap().remove("inquiry_drafts");

    let restored: BlockStore = serde_json::from_value(value).unwrap();
    assert_eq!(restored.amplification_drafts.len(), 0);
    assert_eq!(restored.distillation_drafts.len(), 0);
    assert_eq!(restored.instruction_drafts.len(), 0);
    assert_eq!(restored.probe_drafts.len(), 0);
}

#[test]
fn backward_compat_mount_without_format_defaults_to_json() {
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let mount_id =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from("legacy.json")));
    points.insert(mount_id, "legacy mount".to_string());
    let store = BlockStore::new(vec![mount_id], nodes, points);

    let mut value = serde_json::to_value(&store).unwrap();
    if let Some(nodes_obj) = value["nodes"].as_object_mut() {
        for node in nodes_obj.values_mut() {
            if node.get("path").is_some() {
                node.as_object_mut().expect("mount node object").remove("format");
            }
        }
    } else if let Some(nodes_arr) = value["nodes"].as_array_mut() {
        for node in nodes_arr {
            if node.get("path").is_some() {
                node.as_object_mut().expect("mount node object").remove("format");
            }
        }
    } else {
        panic!("unexpected nodes serialization shape");
    }

    let restored: BlockStore = serde_json::from_value(value).unwrap();
    assert_eq!(
        restored.node(&restored.roots()[0]).and_then(|node| node.mount_format()),
        Some(MountFormat::Json)
    );
}

#[test]
fn load_from_path_returns_parse_error_on_malformed_json() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("broken.json");
    fs::write(&path, "{ not valid json").unwrap();

    let err = BlockStore::load_from_path(&path).unwrap_err();
    assert!(matches!(err, StoreLoadError::Parse { .. }));
}

#[test]
fn load_from_path_with_dangling_child_is_operable_and_normalized_on_save_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("invalid-graph.json");

    let mut nodes = FxHashMap::default();
    let dangling_child = BlockId::default();
    let root = insert_node(&mut nodes, BlockNode::with_children(vec![dangling_child]));
    let mut points = FxHashMap::default();
    points.insert(root, "root".to_string());
    let invalid_store = BlockStore::new(vec![root], nodes, points);
    fs::write(&path, serde_json::to_string_pretty(&invalid_store).unwrap()).unwrap();

    let loaded = BlockStore::load_from_path(&path).unwrap();
    assert!(loaded.node(&root).is_some());
    assert!(loaded.node(&dangling_child).is_none());
    let lineage = loaded.lineage_points_for_id(&root);
    assert_eq!(lineage.points().last(), Some("root"));

    let normalized = loaded.snapshot_for_save();
    let normalized_root = normalized.roots()[0];
    assert_eq!(normalized.node(&normalized_root).unwrap().children().len(), 0);
}

// -- expand_mount / collapse_mount --

fn write_sub_store(dir: &std::path::Path, filename: &str) -> (std::path::PathBuf, BlockStore) {
    let sub = simple_store().0;
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(&sub).unwrap();
    fs::write(&path, json).unwrap();
    (path, sub)
}

fn write_markdown_sub_store(
    dir: &std::path::Path, filename: &str,
) -> (std::path::PathBuf, BlockStore) {
    let sub = simple_store().0;
    let path = dir.join(filename);
    let markdown = BlockStore::render_markdown_mount_store(&sub);
    fs::write(&path, markdown).unwrap();
    (path, sub)
}

/// Build a [`BlockStore`] whose sole root is a JSON-format mount node.
fn mount_store(filename: &str) -> (BlockStore, BlockId) {
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let mount_id =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from(filename)));
    points.insert(mount_id, String::new());
    let store = BlockStore::new(vec![mount_id], nodes, points);
    (store, mount_id)
}

/// Build a [`BlockStore`] whose sole root is a Markdown-format mount node.
fn mount_store_md(filename: &str) -> (BlockStore, BlockId) {
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let mount_id = insert_node(
        &mut nodes,
        BlockNode::with_path_and_format(
            std::path::PathBuf::from(filename),
            super::mount::MountFormat::Markdown,
        ),
    );
    points.insert(mount_id, String::new());
    let store = BlockStore::new(vec![mount_id], nodes, points);
    (store, mount_id)
}

#[test]
fn expand_mount_loads_and_rekeys() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sub) = write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();

    assert_eq!(new_roots.len(), sub.roots().len());
    assert!(store.node(&mount_id).unwrap().children().len() == new_roots.len());

    for &r in &new_roots {
        assert!(store.node(&r).is_some());
    }
    let entry = store.mount_table().entry(mount_id).unwrap();
    for &r in &new_roots {
        assert!(entry.block_ids.contains(&r));
    }
}

#[test]
fn expand_mount_preserves_points() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let root_point = store.point(&new_roots[0]);
    assert_eq!(root_point, Some("root".to_string()));
}

#[test]
fn expand_markdown_mount_loads_and_rekeys() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sub) = write_markdown_sub_store(tmp.path(), "sub.md");

    let (mut store, mount_id) = mount_store_md("sub.md");

    let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    assert_eq!(new_roots.len(), sub.roots().len());
    assert_eq!(store.point(&new_roots[0]), Some("root".to_string()));
}

#[test]
fn expand_markdown_mount_clears_collapsed_state_for_mount_point() {
    let tmp = tempfile::tempdir().unwrap();
    write_markdown_sub_store(tmp.path(), "sub.md");

    let (mut store, mount_id) = mount_store_md("sub.md");
    store.view_collapsed.insert(mount_id, true);

    let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    assert!(!new_roots.is_empty());
    assert!(!store.is_collapsed(&mount_id));
    assert_eq!(store.children(&mount_id), new_roots.as_slice());
}

#[test]
fn expand_markdown_mount_errors_on_invalid_text() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("sub.md"), "- \"missing preamble\"\n").unwrap();

    let (mut store, mount_id) = mount_store_md("sub.md");

    let result = store.expand_mount(&mount_id, tmp.path());
    assert!(matches!(result, Err(MountError::MarkdownParse { .. })));
}

#[test]
fn expand_mount_errors_on_children_node() {
    let (mut store, root, _, _) = simple_store();
    let result = store.expand_mount(&root, std::path::Path::new("."));
    assert!(result.is_err());
}

#[test]
fn expand_mount_errors_on_missing_file() {
    let (mut store, mount_id) = mount_store("nonexistent.json");

    let result = store.expand_mount(&mount_id, std::path::Path::new("."));
    assert!(result.is_err());
}

#[test]
fn collapse_mount_restores_mount_node() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    assert!(!new_roots.is_empty());

    store.collapse_mount(&mount_id).unwrap();

    assert!(store.node(&mount_id).unwrap().mount_path().is_some());
    for &r in &new_roots {
        assert!(store.node(&r).is_none());
    }
}

#[test]
fn collapse_mount_returns_none_for_unmounted() {
    let (mut store, root, _, _) = simple_store();
    assert!(store.collapse_mount(&root).is_none());
}

#[test]
fn move_mount_file_updates_unexpanded_mount_path_and_moves_file() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let moved_path = tmp.path().join("moved").join("renamed.json");
    store.move_mount_file(&mount_id, &moved_path, tmp.path()).unwrap();

    assert!(!tmp.path().join("sub.json").exists());
    assert!(moved_path.exists());
    assert_eq!(
        store.node(&mount_id).unwrap().mount_path(),
        Some(std::path::Path::new("moved/renamed.json"))
    );
}

#[test]
fn move_mount_file_rewrites_expanded_mount_and_updates_entry() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    store.update_point(&roots[0], "rewritten root".to_string());

    let moved_path = tmp.path().join("next.json");
    store.move_mount_file(&mount_id, &moved_path, tmp.path()).unwrap();

    assert!(!tmp.path().join("sub.json").exists());
    assert!(moved_path.exists());

    let entry = store.mount_table().entry(mount_id).unwrap();
    assert_eq!(entry.rel_path, std::path::PathBuf::from("next.json"));
    let reloaded: BlockStore =
        serde_json::from_str(&fs::read_to_string(&moved_path).unwrap()).unwrap();
    assert_eq!(reloaded.point(&reloaded.roots()[0]), Some("rewritten root".to_string()));
}

#[test]
fn inline_mount_shallow_keeps_nested_mount_nodes() {
    let tmp = tempfile::tempdir().unwrap();

    let (inner_store, _, _, _) = simple_store();
    write_store(tmp.path(), "inner.json", &inner_store);

    let mut outer_nodes = FxHashMap::default();
    let mut outer_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut outer_nodes, BlockNode::with_path(std::path::PathBuf::from("inner.json")));
    outer_points.insert(inner_mount, String::new());
    let outer_root = insert_node(&mut outer_nodes, BlockNode::with_children(vec![inner_mount]));
    outer_points.insert(outer_root, "outer root".to_string());
    let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
    write_store(tmp.path(), "outer.json", &outer_store);

    let (mut store, outer_mount) = mount_store("outer.json");

    store.inline_mount(&outer_mount, tmp.path()).unwrap();

    assert!(store.mount_table().entry(outer_mount).is_none());
    assert!(store.node(&outer_mount).unwrap().mount_path().is_none());
    let outer_children = store.children(&outer_mount);
    assert_eq!(outer_children.len(), 1);

    let nested_mount = *store
        .children(&outer_children[0])
        .iter()
        .find(|id| store.node(id).is_some_and(|node| node.mount_path().is_some()))
        .unwrap();
    assert_eq!(
        store.node(&nested_mount).and_then(|node| node.mount_path()),
        Some(std::path::Path::new("inner.json"))
    );
}

#[test]
fn inline_mount_recursive_inlines_nested_mounts_into_main_store() {
    let tmp = tempfile::tempdir().unwrap();

    let (inner_store, _, _, _) = simple_store();
    write_store(tmp.path(), "inner.json", &inner_store);

    let mut outer_nodes = FxHashMap::default();
    let mut outer_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut outer_nodes, BlockNode::with_path(std::path::PathBuf::from("inner.json")));
    outer_points.insert(inner_mount, String::new());
    let outer_root = insert_node(&mut outer_nodes, BlockNode::with_children(vec![inner_mount]));
    outer_points.insert(outer_root, "outer root".to_string());
    let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
    write_store(tmp.path(), "outer.json", &outer_store);

    let (mut store, outer_mount) = mount_store("outer.json");

    let inlined = store.inline_mount_recursive(&outer_mount, tmp.path()).unwrap();
    assert_eq!(inlined, 2);
    assert!(store.mount_table().entry(outer_mount).is_none());
    assert!(store.node(&outer_mount).unwrap().mount_path().is_none());
    assert!(!store.children(&outer_mount).is_empty());

    let mut subtree = Vec::new();
    for child in store.children(&outer_mount) {
        store.collect_subtree_ids(child, &mut subtree);
    }
    assert!(
        subtree.iter().all(|id| store.node(id).is_some_and(|node| node.mount_path().is_none()))
    );
    assert!(store.mount_table().entries().next().is_none());
}

// -- save-back --

#[test]
fn snapshot_excludes_mounted_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    store.expand_mount(&mount_id, tmp.path()).unwrap();

    let snap = store.snapshot_for_save();
    assert_eq!(snap.roots().len(), 1);
    let node = snap.node(&snap.roots()[0]).unwrap();
    assert!(node.mount_path().is_some());
    assert_eq!(snap.nodes.len(), 1);
}

#[test]
fn save_mounts_writes_updated_points() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let new_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    store.update_point(&new_roots[0], "modified root".to_string());
    store.save_mounts().unwrap();

    let saved_json = fs::read_to_string(tmp.path().join("sub.json")).unwrap();
    let saved: BlockStore = serde_json::from_str(&saved_json).unwrap();
    let saved_root_point = saved.point(&saved.roots()[0]);
    assert_eq!(saved_root_point, Some("modified root".to_string()));
}

#[test]
fn save_subtree_to_markdown_sets_mount_format_and_writes_markdown() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut store, root, _child_a, _child_b) = simple_store();
    let path = tmp.path().join("subtree.md");

    store.save_subtree_to_file(&root, &path, tmp.path()).unwrap();

    let mount_node = store.node(&root).unwrap();
    assert_eq!(mount_node.mount_path(), Some(std::path::Path::new("subtree.md")));
    assert_eq!(mount_node.mount_format(), Some(MountFormat::Markdown));

    let markdown = fs::read_to_string(&path).unwrap();
    assert!(markdown.starts_with("<!-- bb-mount format=markdown v1 -->\n"));
    assert!(markdown.contains("- \"child_a\"\n"));
    assert!(markdown.contains("- \"child_b\"\n"));
}

#[test]
fn save_subtree_to_markdown_escapes_special_characters() {
    let tmp = tempfile::tempdir().unwrap();
    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let child = insert_node(&mut nodes, BlockNode::with_children(vec![]));
    points.insert(child, "line1\n\"quoted\"\\tail".to_string());
    let root = insert_node(&mut nodes, BlockNode::with_children(vec![child]));
    points.insert(root, "root".to_string());
    let mut store = BlockStore::new(vec![root], nodes, points);

    let path = tmp.path().join("escaped.md");
    store.save_subtree_to_file(&root, &path, tmp.path()).unwrap();

    let markdown = fs::read_to_string(&path).unwrap();
    assert!(markdown.contains("- \"line1\\n\\\"quoted\\\"\\\\tail\"\n"));
}

#[test]
fn expand_mount_allows_duplicate_path() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let mount_a =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from("sub.json")));
    points.insert(mount_a, String::new());
    let mount_b =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from("sub.json")));
    points.insert(mount_b, String::new());
    let mut store = BlockStore::new(vec![mount_a, mount_b], nodes, points);

    store.expand_mount(&mount_a, tmp.path()).unwrap();
    let second = store.expand_mount(&mount_b, tmp.path()).unwrap();
    assert!(!second.is_empty());
    assert!(!store.children(&mount_b).is_empty());
}

#[test]
fn expand_mount_allows_after_collapse() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let mount_a =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from("sub.json")));
    points.insert(mount_a, String::new());
    let mount_b =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from("sub.json")));
    points.insert(mount_b, String::new());
    let mut store = BlockStore::new(vec![mount_a, mount_b], nodes, points);

    store.expand_mount(&mount_a, tmp.path()).unwrap();
    store.collapse_mount(&mount_a).unwrap();
    store.expand_mount(&mount_b, tmp.path()).unwrap();
    assert!(!store.children(&mount_b).is_empty());
}

#[test]
fn collapse_mount_restores_relative_path() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let mut nodes = FxHashMap::default();
    let mut points = FxHashMap::default();
    let mount_id =
        insert_node(&mut nodes, BlockNode::with_path(std::path::PathBuf::from("sub.json")));
    points.insert(mount_id, String::new());
    let mut store = BlockStore::new(vec![mount_id], nodes, points);

    store.expand_mount(&mount_id, tmp.path()).unwrap();
    store.collapse_mount(&mount_id).unwrap();

    let path = store.node(&mount_id).unwrap().mount_path().unwrap();
    assert_eq!(path, std::path::Path::new("sub.json"));
}

#[test]
fn clone_preserves_mount_table_for_undo() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let snapshot = store.clone();
    assert!(snapshot.node(&mount_id).unwrap().mount_path().is_some());

    store.expand_mount(&mount_id, tmp.path()).unwrap();
    assert!(store.node(&mount_id).unwrap().mount_path().is_none());
    assert!(!store.children(&mount_id).is_empty());

    // Restoring the snapshot should give back the unexpanded mount.
    let restored = snapshot;
    assert!(restored.node(&mount_id).unwrap().mount_path().is_some());
    assert!(restored.children(&mount_id).is_empty());
    assert!(restored.mount_table().entry(mount_id).is_none());
}

// -- integration: nested mounts --

fn write_store(dir: &std::path::Path, filename: &str, store: &BlockStore) {
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(store).unwrap();
    fs::write(&path, json).unwrap();
}

#[test]
fn nested_mount_expands_recursively() {
    let tmp = tempfile::tempdir().unwrap();

    let (inner_store, _, _, _) = simple_store();
    write_store(tmp.path(), "inner.json", &inner_store);

    let mut outer_nodes = FxHashMap::default();
    let mut outer_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut outer_nodes, BlockNode::with_path(std::path::PathBuf::from("inner.json")));
    outer_points.insert(inner_mount, String::new());
    let outer_root = insert_node(&mut outer_nodes, BlockNode::with_children(vec![inner_mount]));
    outer_points.insert(outer_root, "outer root".to_string());
    let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
    write_store(tmp.path(), "outer.json", &outer_store);

    let (mut store, outer_mount) = mount_store("outer.json");

    let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
    assert_eq!(outer_children.len(), 1);

    let rekeyed_outer_root = outer_children[0];
    let nested_mount_candidates: Vec<BlockId> = store
        .children(&rekeyed_outer_root)
        .iter()
        .filter(|id| store.node(id).unwrap().mount_path().is_some())
        .copied()
        .collect();
    assert_eq!(nested_mount_candidates.len(), 1);

    let nested_mount_id = nested_mount_candidates[0];
    let inner_children = store.expand_mount(&nested_mount_id, tmp.path()).unwrap();
    assert_eq!(inner_children.len(), 1);
    assert_eq!(store.point(&inner_children[0]), Some("root".to_string()));
}

#[test]
fn nested_mount_path_resolves_relative_to_parent_mount_file() {
    let tmp = tempfile::tempdir().unwrap();
    let nested_dir = tmp.path().join("nested");
    fs::create_dir_all(&nested_dir).unwrap();

    let (inner_store, _, _, _) = simple_store();
    write_store(&nested_dir, "inner.json", &inner_store);

    let mut outer_nodes = FxHashMap::default();
    let mut outer_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut outer_nodes, BlockNode::with_path(std::path::PathBuf::from("inner.json")));
    outer_points.insert(inner_mount, String::new());
    let outer_root = insert_node(&mut outer_nodes, BlockNode::with_children(vec![inner_mount]));
    outer_points.insert(outer_root, "outer root".to_string());
    let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
    write_store(&nested_dir, "outer.json", &outer_store);

    let (mut store, outer_mount) = mount_store("nested/outer.json");

    let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
    let rekeyed_outer_root = outer_children[0];
    let nested_mount = *store
        .children(&rekeyed_outer_root)
        .iter()
        .find(|id| store.node(id).unwrap().mount_path().is_some())
        .unwrap();

    let inner_children = store.expand_mount(&nested_mount, tmp.path()).unwrap();
    assert_eq!(inner_children.len(), 1);
    assert_eq!(store.point(&inner_children[0]), Some("root".to_string()));
}

#[test]
fn save_mounts_preserves_nested_mount_nodes() {
    let tmp = tempfile::tempdir().unwrap();
    let nested_dir = tmp.path().join("nested");
    fs::create_dir_all(&nested_dir).unwrap();

    let (inner_store, _, _, _) = simple_store();
    write_store(&nested_dir, "inner.json", &inner_store);

    let mut outer_nodes = FxHashMap::default();
    let mut outer_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut outer_nodes, BlockNode::with_path(std::path::PathBuf::from("inner.json")));
    outer_points.insert(inner_mount, String::new());
    let outer_root = insert_node(&mut outer_nodes, BlockNode::with_children(vec![inner_mount]));
    outer_points.insert(outer_root, "outer root".to_string());
    let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
    write_store(&nested_dir, "outer.json", &outer_store);

    let (mut store, outer_mount) = mount_store("nested/outer.json");

    let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
    let rekeyed_outer_root = outer_children[0];
    let nested_mount = *store
        .children(&rekeyed_outer_root)
        .iter()
        .find(|id| store.node(id).unwrap().mount_path().is_some())
        .unwrap();
    let inner_children = store.expand_mount(&nested_mount, tmp.path()).unwrap();
    store.update_point(&inner_children[0], "edited nested root".to_string());

    store.save_mounts().unwrap();

    let outer_json = fs::read_to_string(nested_dir.join("outer.json")).unwrap();
    let saved_outer: BlockStore = serde_json::from_str(&outer_json).unwrap();
    let saved_outer_root = saved_outer.roots()[0];
    let saved_nested_mount = saved_outer.children(&saved_outer_root)[0];
    let saved_nested_path = saved_outer.node(&saved_nested_mount).unwrap().mount_path();
    assert_eq!(saved_nested_path, Some(std::path::Path::new("inner.json")));

    let inner_json = fs::read_to_string(nested_dir.join("inner.json")).unwrap();
    let saved_inner: BlockStore = serde_json::from_str(&inner_json).unwrap();
    assert_eq!(saved_inner.point(&saved_inner.roots()[0]), Some("edited nested root".to_string()));
}

// -- integration: round-trip persistence --

#[test]
fn mount_edit_save_collapse_remount_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots_1 = store.expand_mount(&mount_id, tmp.path()).unwrap();
    store.update_point(&roots_1[0], "edited root".to_string());
    store.save_mounts().unwrap();

    store.collapse_mount(&mount_id).unwrap();
    assert!(store.node(&mount_id).unwrap().mount_path().is_some());

    let roots_2 = store.expand_mount(&mount_id, tmp.path()).unwrap();
    assert_eq!(store.point(&roots_2[0]), Some("edited root".to_string()));
}

#[test]
fn mount_save_persists_new_deep_non_mounted_nodes() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots_1 = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let root = roots_1[0];
    let child_a = store.children(&root)[0];
    let deep_child = store.append_child(&child_a, "deep child".to_string()).unwrap();
    store.append_child(&deep_child, "deep grandchild".to_string()).unwrap();

    store.save_mounts().unwrap();
    store.collapse_mount(&mount_id).unwrap();

    let roots_2 = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let reloaded_root = roots_2[0];
    let reloaded_child_a = store.children(&reloaded_root)[0];
    let reloaded_deep_child = *store
        .children(&reloaded_child_a)
        .iter()
        .find(|id| store.point(id) == Some("deep child".to_string()))
        .unwrap();
    let reloaded_deep_grandchild = store.children(&reloaded_deep_child)[0];
    assert_eq!(store.point(&reloaded_deep_grandchild), Some("deep grandchild".to_string()));
}

#[test]
fn mount_save_persists_new_sibling_under_mounted_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let root = roots[0];
    let first_child = store.children(&root)[0];
    store.append_sibling(&first_child, "sibling created in mounted file".to_string()).unwrap();

    store.save_mounts().unwrap();
    store.collapse_mount(&mount_id).unwrap();
    let reloaded_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let reloaded_root = reloaded_roots[0];
    let has_new_sibling = store
        .children(&reloaded_root)
        .iter()
        .any(|id| store.point(id) == Some("sibling created in mounted file".to_string()));
    assert!(has_new_sibling);
}

#[test]
fn mount_save_persists_duplicated_subtree_under_mounted_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let root = roots[0];
    let first_child = store.children(&root)[0];
    let duplicated = store.duplicate_subtree_after(&first_child).unwrap();
    store.update_point(&duplicated, "duplicated mounted node".to_string());

    store.save_mounts().unwrap();
    store.collapse_mount(&mount_id).unwrap();
    let reloaded_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let reloaded_root = reloaded_roots[0];
    let has_duplicate = store
        .children(&reloaded_root)
        .iter()
        .any(|id| store.point(id) == Some("duplicated mounted node".to_string()));
    assert!(has_duplicate);
}

#[test]
fn collapse_mount_discards_unsaved_new_descendants() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let root = roots[0];
    let transient = store.append_child(&root, "transient unsaved child".to_string()).unwrap();

    store.collapse_mount(&mount_id).unwrap();
    assert!(store.node(&transient).is_none());
    assert!(store.mount_table.origin(transient).is_none());

    let reloaded_roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let reloaded_root = reloaded_roots[0];
    let still_has_transient = store
        .children(&reloaded_root)
        .iter()
        .any(|id| store.point(id) == Some("transient unsaved child".to_string()));
    assert!(!still_has_transient);
}

#[test]
fn nested_mount_save_persists_new_descendants_in_inner_file() {
    let tmp = tempfile::tempdir().unwrap();
    let nested_dir = tmp.path().join("nested");
    fs::create_dir_all(&nested_dir).unwrap();

    let (inner_store, _, _, _) = simple_store();
    write_store(&nested_dir, "inner.json", &inner_store);

    let mut outer_nodes = FxHashMap::default();
    let mut outer_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut outer_nodes, BlockNode::with_path(std::path::PathBuf::from("inner.json")));
    outer_points.insert(inner_mount, String::new());
    let outer_root = insert_node(&mut outer_nodes, BlockNode::with_children(vec![inner_mount]));
    outer_points.insert(outer_root, "outer root".to_string());
    let outer_store = BlockStore::new(vec![outer_root], outer_nodes, outer_points);
    write_store(&nested_dir, "outer.json", &outer_store);

    let (mut store, outer_mount) = mount_store("nested/outer.json");

    let outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
    let rekeyed_outer_root = outer_children[0];
    let nested_mount = *store
        .children(&rekeyed_outer_root)
        .iter()
        .find(|id| store.node(id).unwrap().mount_path().is_some())
        .unwrap();
    let inner_children = store.expand_mount(&nested_mount, tmp.path()).unwrap();
    let inner_root = inner_children[0];
    let added = store.append_child(&inner_root, "new inner child".to_string()).unwrap();
    store.append_child(&added, "new inner grandchild".to_string()).unwrap();

    store.save_mounts().unwrap();
    store.collapse_mount(&outer_mount).unwrap();

    let reloaded_outer_children = store.expand_mount(&outer_mount, tmp.path()).unwrap();
    let reloaded_outer_root = reloaded_outer_children[0];
    let reloaded_nested_mount = *store
        .children(&reloaded_outer_root)
        .iter()
        .find(|id| store.node(id).unwrap().mount_path().is_some())
        .unwrap();
    let reloaded_inner_children = store.expand_mount(&reloaded_nested_mount, tmp.path()).unwrap();
    let reloaded_inner_root = reloaded_inner_children[0];
    let reloaded_added = *store
        .children(&reloaded_inner_root)
        .iter()
        .find(|id| store.point(id) == Some("new inner child".to_string()))
        .unwrap();
    let reloaded_grandchild = store.children(&reloaded_added)[0];
    assert_eq!(store.point(&reloaded_grandchild), Some("new inner grandchild".to_string()));
}

#[test]
fn snapshot_excludes_new_nodes_under_expanded_mount() {
    let tmp = tempfile::tempdir().unwrap();
    write_sub_store(tmp.path(), "sub.json");

    let (mut store, mount_id) = mount_store("sub.json");

    let roots = store.expand_mount(&mount_id, tmp.path()).unwrap();
    let root = roots[0];
    store.append_child(&root, "unsaved-in-main".to_string()).unwrap();

    let snapshot = store.snapshot_for_save();
    let has_mount = snapshot.nodes.iter().any(
        |(_, node)| matches!(node, BlockNode::Mount { path, .. } if path == std::path::Path::new("sub.json")),
    );
    assert!(has_mount);
    let leaks_new_mounted_node =
        snapshot.points.iter().any(|(_, point)| point.display_text() == "unsaved-in-main");
    assert!(!leaks_new_mounted_node);
}

#[test]
fn nested_self_reference_can_expand_lazily() {
    let tmp = tempfile::tempdir().unwrap();

    let mut self_nodes = FxHashMap::default();
    let mut self_points = FxHashMap::default();
    let inner_mount =
        insert_node(&mut self_nodes, BlockNode::with_path(std::path::PathBuf::from("self.json")));
    self_points.insert(inner_mount, String::new());
    let self_root = insert_node(&mut self_nodes, BlockNode::with_children(vec![inner_mount]));
    self_points.insert(self_root, "self-ref root".to_string());
    let self_store = BlockStore::new(vec![self_root], self_nodes, self_points);
    write_store(tmp.path(), "self.json", &self_store);

    let (mut store, main_mount) = mount_store("self.json");

    let roots = store.expand_mount(&main_mount, tmp.path()).unwrap();
    let rekeyed_root = roots[0];
    let nested: Vec<BlockId> = store
        .children(&rekeyed_root)
        .iter()
        .filter(|id| store.node(id).unwrap().mount_path().is_some())
        .copied()
        .collect();
    assert_eq!(nested.len(), 1);

    let inner_roots = store.expand_mount(&nested[0], tmp.path()).unwrap();
    assert_eq!(inner_roots.len(), 1);
    assert_eq!(store.point(&inner_roots[0]), Some("self-ref root".to_string()));
}

// -- view_collapsed persistence --

#[test]
fn serde_round_trip_preserves_view_collapsed() {
    let (mut store, _root, child_a, _child_b) = simple_store();
    store.view_collapsed.insert(child_a, true);

    let json = serde_json::to_string(&store).unwrap();
    let restored: BlockStore = serde_json::from_str(&json).unwrap();

    assert_eq!(store, restored);
    assert!(restored.view_collapsed.contains_key(&child_a));
}

#[test]
fn backward_compat_missing_view_collapsed_defaults_empty() {
    let (store, _, _, _) = simple_store();
    let mut value = serde_json::to_value(&store).unwrap();
    value.as_object_mut().unwrap().remove("view_collapsed");

    let restored: BlockStore = serde_json::from_value(value).unwrap();
    assert_eq!(restored.view_collapsed.len(), 0);
}

#[test]
fn remove_subtree_cleans_view_collapsed() {
    let (mut store, _root, child_a, _child_b) = simple_store();
    store.view_collapsed.insert(child_a, true);

    store.remove_block_subtree(&child_a).unwrap();
    assert!(!store.view_collapsed.contains_key(&child_a));
}

#[test]
fn block_context_with_friend_blocks_preserves_order_and_perspective() {
    let (store, root, child_a, child_b) = simple_store();
    let context = store.block_context_for_id_with_friend_blocks(
        &root,
        &[
            FriendBlock {
                block_id: child_b,
                perspective: Some("contrast".to_string()),
                ..Default::default()
            },
            FriendBlock { block_id: child_a, perspective: None, ..Default::default() },
        ],
    );
    let friend_blocks = context.friend_blocks();
    assert_eq!(friend_blocks.len(), 2);
    assert_eq!(friend_blocks[0].point(), "child_b");
    assert_eq!(friend_blocks[0].perspective(), Some("contrast"));
    assert_eq!(friend_blocks[1].point(), "child_a");
    assert_eq!(friend_blocks[1].perspective(), None);
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
fn serde_round_trip_preserves_friend_blocks() {
    let (mut store, root, child_a, child_b) = simple_store();
    store.set_friend_blocks_for(
        &root,
        vec![
            FriendBlock { block_id: child_a, perspective: None, ..Default::default() },
            FriendBlock {
                block_id: child_b,
                perspective: Some("counter-example".to_string()),
                ..Default::default()
            },
        ],
    );

    let json = serde_json::to_string(&store).unwrap();
    let restored: BlockStore = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.friend_blocks_for(&root),
        &[
            FriendBlock { block_id: child_a, perspective: None, ..Default::default() },
            FriendBlock {
                block_id: child_b,
                perspective: Some("counter-example".to_string()),
                ..Default::default()
            },
        ]
    );
}

#[test]
fn backward_compat_missing_friend_blocks_defaults_empty() {
    let (store, _, _, _) = simple_store();
    let mut value = serde_json::to_value(&store).unwrap();
    value.as_object_mut().unwrap().remove("friend_blocks");

    let restored: BlockStore = serde_json::from_value(value).unwrap();
    assert_eq!(restored.friend_blocks.len(), 0);
}

#[test]
fn remove_subtree_cleans_friend_blocks_keys_and_values() {
    let (mut store, root, child_a, child_b) = simple_store();
    store.set_friend_blocks_for(
        &root,
        vec![
            FriendBlock { block_id: child_a, perspective: None, ..Default::default() },
            FriendBlock { block_id: child_b, perspective: None, ..Default::default() },
        ],
    );
    store.set_friend_blocks_for(
        &child_a,
        vec![FriendBlock {
            block_id: root,
            perspective: Some("parent framing".to_string()),
            ..Default::default()
        }],
    );

    store.remove_block_subtree(&child_a).unwrap();

    assert_eq!(
        store.friend_blocks_for(&root),
        &[FriendBlock { block_id: child_b, perspective: None, ..Default::default() }]
    );
    assert!(store.friend_blocks_for(&child_a).is_empty());
}

// ---------------------------------------------------------------------------
// MountTable unit tests (moved from top-level mount module)
// ---------------------------------------------------------------------------

#[test]
fn mount_table_insert_and_query_entry() {
    use super::mount::{MountEntry, MountFormat, MountTable};

    let mut table = MountTable::new();
    let ids = make_ids(3);
    let entry = MountEntry::new(
        std::path::PathBuf::from("sub.json"),
        std::path::PathBuf::from("sub.json"),
        MountFormat::Json,
        vec![ids[1]],
        vec![ids[1], ids[2]],
    );
    table.insert_entry(ids[0], entry);
    let got = table.entry(ids[0]).unwrap();
    assert_eq!(got.path, std::path::PathBuf::from("sub.json"));
    assert_eq!(got.root_ids, vec![ids[1]]);
    assert_eq!(got.block_ids, vec![ids[1], ids[2]]);
}

#[test]
fn mount_table_remove_entry_clears_origins() {
    use super::mount::{BlockOrigin, MountEntry, MountFormat, MountTable};

    let mut table = MountTable::new();
    let ids = make_ids(3);

    let origin = BlockOrigin::Mounted { mount_point: ids[0] };
    table.set_origin(ids[1], origin.clone());
    table.set_origin(ids[2], origin);
    table.insert_entry(
        ids[0],
        MountEntry::new(
            std::path::PathBuf::from("x.json"),
            std::path::PathBuf::from("x.json"),
            MountFormat::Json,
            vec![ids[1]],
            vec![ids[1], ids[2]],
        ),
    );

    let removed = table.remove_entry(ids[0]).unwrap();
    assert_eq!(removed.block_ids.len(), 2);
    assert!(table.entry(ids[0]).is_none());
}

// ── Friend Perspective Tests ───────────────────────────────────────────────

#[test]
fn set_friend_perspective_updates_existing_friend() {
    let (mut store, root, child_a, _child_b) = simple_store();
    // Add friend
    store.set_friend_blocks_for(
        &root,
        vec![FriendBlock { block_id: child_a, perspective: None, ..Default::default() }],
    );
    // Set perspective
    store.set_friend_blocks_for(
        &root,
        vec![FriendBlock {
            block_id: child_a,
            perspective: Some("supporting evidence".to_string()),
            ..Default::default()
        }],
    );
    let friends = store.friend_blocks_for(&root);
    assert_eq!(friends.len(), 1);
    assert_eq!(friends[0].perspective, Some("supporting evidence".to_string()));
}

#[test]
fn set_friend_perspective_clears_existing_perspective() {
    let (mut store, root, child_a, _child_b) = simple_store();
    // Add friend with existing perspective
    store.set_friend_blocks_for(
        &root,
        vec![FriendBlock {
            block_id: child_a,
            perspective: Some("original perspective".to_string()),
            ..Default::default()
        }],
    );
    // Clear perspective by setting to None
    store.set_friend_blocks_for(
        &root,
        vec![FriendBlock { block_id: child_a, perspective: None, ..Default::default() }],
    );
    let friends = store.friend_blocks_for(&root);
    assert_eq!(friends.len(), 1);
    assert_eq!(friends[0].perspective, None);
}

#[test]
fn friend_perspective_empty_string_vs_none() {
    let (mut store, root, child_a, _child_b) = simple_store();
    // Set perspective to empty string
    store.set_friend_blocks_for(
        &root,
        vec![FriendBlock {
            block_id: child_a,
            perspective: Some("".to_string()),
            ..Default::default()
        }],
    );
    let friends = store.friend_blocks_for(&root);
    // Empty string is preserved (different from None)
    assert_eq!(friends[0].perspective, Some("".to_string()));
}

#[test]
fn friend_perspective_survives_serde_roundtrip() {
    let (mut store, root, child_a, child_b) = simple_store();
    store.set_friend_blocks_for(
        &root,
        vec![
            FriendBlock {
                block_id: child_a,
                perspective: Some("historical lens".to_string()),
                ..Default::default()
            },
            FriendBlock { block_id: child_b, perspective: None, ..Default::default() },
        ],
    );
    // Serialize and deserialize
    let serialized = serde_json::to_string(&store).unwrap();
    let restored: super::BlockStore = serde_json::from_str(&serialized).unwrap();
    let friends = restored.friend_blocks_for(&root);
    assert_eq!(friends.len(), 2);
    assert_eq!(friends[0].perspective, Some("historical lens".to_string()));
    assert_eq!(friends[1].perspective, None);
}

#[test]
fn remove_friend_block_also_removes_perspective() {
    let (mut store, root, child_a, child_b) = simple_store();
    store.set_friend_blocks_for(
        &root,
        vec![
            FriendBlock {
                block_id: child_a,
                perspective: Some("primary".to_string()),
                ..Default::default()
            },
            FriendBlock {
                block_id: child_b,
                perspective: Some("secondary".to_string()),
                ..Default::default()
            },
        ],
    );
    // Remove one friend
    let mut friends = store.friend_blocks_for(&root).to_vec();
    friends.retain(|f| f.block_id != child_a);
    store.set_friend_blocks_for(&root, friends);
    let remaining = store.friend_blocks_for(&root);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].block_id, child_b);
    assert_eq!(remaining[0].perspective, Some("secondary".to_string()));
}
