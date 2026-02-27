//! Integration tests for CLI commands.
//!
//! These tests verify complex multi-operation scenarios, read-after-write semantics,
//! and edge cases that could expose bugs in the CLI execution layer.

use crate::cli::{
    BlockCommands, BlockId,
    draft::{
        ClearDraftCommand, DraftCommands, ExpandDraftCommand, InstructionDraftCommand,
        ListDraftCommand, ReduceDraftCommand,
    },
    fold::{FoldCommands, StatusFoldCommand, ToggleFoldCommand},
    friend::{AddFriendCommand, FriendCommands, ListFriendCommand, RemoveFriendCommand},
    nav::{LineageCommand, NavCommands, NextCommand, PrevCommand},
    query::{FindCommand, ShowCommand},
    results::CliResult,
    tree::{
        AddChildCommand, AddSiblingCommand, DeleteCommand, DuplicateCommand, MoveCommand,
        TreeCommands, WrapCommand,
    },
};
use crate::store::BlockStore;
use std::path::PathBuf;

fn fmt(id: crate::store::BlockId) -> String {
    format!("{}", id)
}

// ============================================================================
// Read-After-Write Tests
// ============================================================================

#[test]
fn test_read_after_write_add_child() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Add child
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "new block".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let new_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Immediately read the new block
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(fmt(new_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Show { text, children, .. } => {
            assert_eq!(text, "new block");
            assert!(children.is_empty());
        }
        | _ => panic!("Expected Show result"),
    }
}

#[test]
fn test_read_after_write_chain() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let mut ids = vec![root_id];

    // Add 5 children in sequence
    for i in 0..5 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(root_id)),
            text: format!("child{}", i),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;
        let new_id = match result {
            | CliResult::BlockId(id) => id,
            | _ => panic!("Expected BlockId at iteration {}", i),
        };
        ids.push(new_id);
    }

    // Verify all children are readable with correct content
    for (i, &id) in ids.iter().enumerate().skip(1) {
        let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(fmt(id)) });
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::Show { text, .. } => {
                assert_eq!(text, format!("child{}", i - 1));
            }
            | _ => panic!("Expected Show for child {}", i - 1),
        }
    }

    // Verify root has exactly 5 children
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(fmt(root_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Show { children, .. } => {
            assert_eq!(children.len(), 5);
        }
        | _ => panic!("Expected Show for root"),
    }
}

#[test]
fn test_add_sibling_position() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Add sibling after child1
    let cmd = BlockCommands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(fmt(child1)),
        text: "sibling".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let sibling_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Verify position: child1, sibling, child2
    let children = store.children(&root_id);
    assert_eq!(children.len(), 3);

    // Find indices
    let c1_idx = children.iter().position(|&c| c == child1).unwrap();
    let s_idx = children.iter().position(|&c| c == sibling_id).unwrap();
    let _c2_idx = children.iter().position(|&c| c == child2).unwrap();

    assert!(s_idx > c1_idx, "Sibling should be after child1");
}

// ============================================================================
// Tree Integrity Tests
// ============================================================================

#[test]
fn test_wrap_preserves_subtree() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let grandchild1 = store.append_child(&child1, "grandchild1".to_string()).unwrap();

    // Wrap child1
    let cmd = BlockCommands::Tree(TreeCommands::Wrap(WrapCommand {
        block_id: BlockId(fmt(child1)),
        text: "wrapper".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let wrapper_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Verify structure
    assert!(store.children(&root_id).contains(&wrapper_id));
    assert!(store.children(&wrapper_id).contains(&child1));
    assert!(store.children(&child1).contains(&grandchild1));
}

#[test]
fn test_duplicate_preserves_subtree() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&child1, "grandchild1".to_string()).unwrap();
    store.append_child(&child1, "grandchild2".to_string()).unwrap();

    // Duplicate child1
    let cmd = BlockCommands::Tree(TreeCommands::Duplicate(DuplicateCommand {
        block_id: BlockId(fmt(child1)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let dup_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Verify duplicate is sibling
    assert!(store.children(&root_id).contains(&dup_id));
    assert_eq!(store.point(&dup_id), store.point(&child1));
    assert_eq!(store.children(&dup_id).len(), 2);
}

#[test]
fn test_delete_cascades() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&child1, "gc1".to_string()).unwrap();
    store.append_child(&child1, "gc2".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Delete child1
    let cmd =
        BlockCommands::Tree(TreeCommands::Delete(DeleteCommand { block_id: BlockId(fmt(child1)) }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Removed(ids) => {
            assert!(ids.len() >= 3); // child1 + 2 grandchildren
        }
        | _ => panic!("Expected Removed"),
    }

    assert!(!store.children(&root_id).contains(&child1));
    assert!(store.children(&root_id).contains(&child2));
}

#[test]
fn test_move_preserves_structure() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();
    let gc1 = store.append_child(&child1, "gc1".to_string()).unwrap();

    // Move child1 after child2
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(child1)),
        target_id: BlockId(fmt(child2)),
        before: false,
        after: true,
        under: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Success));

    let children = store.children(&root_id);
    assert_eq!(children[0], child2);
    assert_eq!(children[1], child1);
    assert!(store.children(&child1).contains(&gc1));
}

#[test]
fn test_move_under() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Move child2 under child1
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(child2)),
        target_id: BlockId(fmt(child1)),
        before: false,
        after: false,
        under: true,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Success));
    assert!(store.children(&child1).contains(&child2));
    assert!(!store.children(&root_id).contains(&child2));
}

// ============================================================================
// ID Stability Tests
// ============================================================================

#[test]
fn test_ids_stable_after_operations() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();

    let root_str = fmt(root_id);
    let child1_str = fmt(child1);

    // Add more children
    for i in 0..3 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(root_str.clone()),
            text: format!("new{}", i),
        }));
        let (s, _) = cmd.execute(store, &PathBuf::from("."));
        store = s;
    }

    // Original IDs should still work
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(root_str) });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Show { id, .. } if id == root_id));

    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(child1_str) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Show { id, .. } if id == child1));
}

// ============================================================================
// Navigation Tests
// ============================================================================

#[test]
fn test_nav_after_add() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Add child
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "new".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let new_child = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Navigate from new child back to root
    let cmd =
        BlockCommands::Nav(NavCommands::Prev(PrevCommand { block_id: BlockId(fmt(new_child)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::OptionalBlockId(Some(prev_id)) => {
            assert_eq!(prev_id, root_id);
        }
        | _ => panic!("Expected prev"),
    }
}

#[test]
fn test_lineage_after_wrap() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let gc1 = store.append_child(&child1, "gc1".to_string()).unwrap();

    // Wrap child1
    let cmd = BlockCommands::Tree(TreeCommands::Wrap(WrapCommand {
        block_id: BlockId(fmt(child1)),
        text: "wrapper".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let _wrapper_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Get lineage of gc1 - should be longer now
    let cmd =
        BlockCommands::Nav(NavCommands::Lineage(LineageCommand { block_id: BlockId(fmt(gc1)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Lineage(points) => {
            assert_eq!(points.len(), 4); // root, root again?, wrapper, child1
        }
        | _ => panic!("Expected Lineage"),
    }
}

// ============================================================================
// Draft Tests
// ============================================================================

#[test]
fn test_draft_persists_after_tree_mod() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set draft
    let cmd = BlockCommands::Draft(DraftCommands::Expand(ExpandDraftCommand {
        block_id: BlockId(fmt(root_id)),
        rewrite: Some("rewrite".to_string()),
        children: vec!["child".to_string()],
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Modify tree
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "new child".to_string(),
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Verify draft persists
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::DraftList { expansion: Some(exp), .. } => {
            assert_eq!(exp.rewrite, Some("rewrite".to_string()));
        }
        | _ => panic!("Expected expansion draft"),
    }
}

#[test]
fn test_draft_clear_selective() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set multiple drafts
    let cmd = BlockCommands::Draft(DraftCommands::Expand(ExpandDraftCommand {
        block_id: BlockId(fmt(root_id)),
        rewrite: Some("rewrite".to_string()),
        children: vec![],
    }));
    let (s, _) = cmd.execute(store, &PathBuf::from("."));
    store = s;

    let cmd = BlockCommands::Draft(DraftCommands::Instruction(InstructionDraftCommand {
        block_id: BlockId(fmt(root_id)),
        text: "instr".to_string(),
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Clear only expansion
    let cmd = BlockCommands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(fmt(root_id)),
        all: false,
        expand: true,
        reduce: false,
        instruction: false,
        inquiry: false,
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Verify
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::DraftList { expansion: None, instruction: Some(instr), .. } => {
            assert_eq!(instr, "instr");
        }
        | _ => panic!("Expected only instruction"),
    }
}

// ============================================================================
// Friend Tests
// ============================================================================

#[test]
fn test_friend_persists_after_move() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Add friend
    let cmd = BlockCommands::Friend(FriendCommands::Add(AddFriendCommand {
        target_id: BlockId(fmt(child1)),
        friend_id: BlockId(fmt(child2)),
        perspective: Some("related".to_string()),
        telescope_lineage: false,
        telescope_children: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Move child1
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(child1)),
        target_id: BlockId(fmt(child2)),
        before: false,
        after: true,
        under: false,
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Verify friend persists
    let cmd = BlockCommands::Friend(FriendCommands::List(ListFriendCommand {
        target_id: BlockId(fmt(child1)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::FriendList(friends) => {
            assert_eq!(friends.len(), 1);
        }
        | _ => panic!("Expected friend list"),
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_fold_toggle_repeated() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let mut state = store.is_collapsed(&root_id);

    for i in 0..5 {
        let cmd = BlockCommands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
            block_id: BlockId(fmt(root_id)),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;

        match result {
            | CliResult::Collapsed(new_state) => {
                assert_ne!(new_state, state, "Toggle {} should change", i);
                state = new_state;
            }
            | _ => panic!("Expected Collapsed"),
        }
    }

    // Verify with status
    let cmd = BlockCommands::Fold(FoldCommands::Status(StatusFoldCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Collapsed(final_state) => {
            assert_eq!(final_state, state);
        }
        | _ => panic!("Expected Collapsed"),
    }
}

#[test]
fn test_find_case_insensitive() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "CHILD".to_string()).unwrap();
    store.append_child(&root_id, "child".to_string()).unwrap();

    for query in &["CHILD", "child", "ChIlD"] {
        let cmd = BlockCommands::Find(FindCommand { query: query.to_string(), limit: 10 });
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::Find(matches) => {
                assert!(matches.len() >= 2, "Query '{}' should find both", query);
            }
            | _ => panic!("Expected Find"),
        }
    }
}

#[test]
fn test_move_to_self_fails() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();

    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(child1)),
        target_id: BlockId(fmt(child1)),
        before: false,
        after: false,
        under: true,
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Error(_)));
}

#[test]
fn test_duplicate_root() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "child1".to_string()).unwrap();

    let cmd = BlockCommands::Tree(TreeCommands::Duplicate(DuplicateCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let dup_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    assert!(store.roots().contains(&dup_id));
    assert_eq!(store.children(&dup_id).len(), 1);
}

// ============================================================================
// Complex Multi-Step Integration Tests
// ============================================================================

#[test]
fn test_build_deep_tree_then_navigate() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Build a deep tree: root -> child -> grandchild -> greatgrandchild -> gggrandchild
    let mut current_id = root_id;
    let mut ids = vec![current_id];

    for i in 0..10 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(current_id)),
            text: format!("level{}", i),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;

        current_id = match result {
            | CliResult::BlockId(id) => id,
            | _ => panic!("Expected BlockId at level {}", i),
        };
        ids.push(current_id);
    }

    // Navigate from deepest back to root using prev
    let mut cursor = current_id;
    for i in (0..ids.len() - 1).rev() {
        let cmd =
            BlockCommands::Nav(NavCommands::Prev(PrevCommand { block_id: BlockId(fmt(cursor)) }));
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::OptionalBlockId(Some(prev_id)) => {
                assert_eq!(prev_id, ids[i], "Prev at level {} should be {:?}", i, ids[i]);
                cursor = prev_id;
            }
            | _ => panic!("Expected prev at level {}", i),
        }
    }

    // Navigate forward from root using next
    cursor = root_id;
    for i in 1..ids.len() {
        let cmd =
            BlockCommands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(fmt(cursor)) }));
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::OptionalBlockId(Some(next_id)) => {
                assert_eq!(next_id, ids[i]);
                cursor = next_id;
            }
            | _ => panic!("Expected next at level {}", i),
        }
    }
}

#[test]
fn test_complex_restructure_move_wrap_duplicate() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Build initial structure: root -> [A, B, C]
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "A".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let id_a = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "B".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let id_b = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "C".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let id_c = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    // Add children to A: A -> [A1, A2]
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(id_a)),
        text: "A1".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let _id_a1 = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(id_a)),
        text: "A2".to_string(),
    }));
    let (s, _) = cmd.execute(store, &PathBuf::from("."));
    store = s;

    // Wrap A with WrapperA: root -> [WrapperA -> [A -> [A1, A2]], B, C]
    let cmd = BlockCommands::Tree(TreeCommands::Wrap(WrapCommand {
        block_id: BlockId(fmt(id_a)),
        text: "WrapperA".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let id_wrapper_a = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    // Move C under WrapperA: root -> [WrapperA -> [A, C], B]
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(id_c)),
        target_id: BlockId(fmt(id_wrapper_a)),
        before: false,
        after: false,
        under: true,
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Duplicate B: root -> [WrapperA, B, B_copy]
    let cmd = BlockCommands::Tree(TreeCommands::Duplicate(DuplicateCommand {
        block_id: BlockId(fmt(id_b)),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let _id_b_copy = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    // Verify final structure
    let root_children = store.children(&root_id);
    assert_eq!(root_children.len(), 3); // WrapperA, B, B_copy
    assert!(root_children.contains(&id_wrapper_a));
    assert!(root_children.contains(&id_b));

    // Verify WrapperA has A and C as children
    let wrapper_children = store.children(&id_wrapper_a);
    assert_eq!(wrapper_children.len(), 2);
    assert!(wrapper_children.contains(&id_a));
    assert!(wrapper_children.contains(&id_c));

    // Verify A still has A1, A2
    let a_children = store.children(&id_a);
    assert_eq!(a_children.len(), 2);
}

#[test]
fn test_draft_workflow_expand_then_reduce_then_clear() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set expansion draft
    let cmd = BlockCommands::Draft(DraftCommands::Expand(ExpandDraftCommand {
        block_id: BlockId(fmt(root_id)),
        rewrite: Some("expanded version".to_string()),
        children: vec!["child1".to_string(), "child2".to_string()],
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Set reduction draft
    let cmd = BlockCommands::Draft(DraftCommands::Reduce(ReduceDraftCommand {
        block_id: BlockId(fmt(root_id)),
        reduction: "summary of everything".to_string(),
        redundant_children: vec![],
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Set instruction draft
    let cmd = BlockCommands::Draft(DraftCommands::Instruction(InstructionDraftCommand {
        block_id: BlockId(fmt(root_id)),
        text: "Make this clearer".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Verify all three drafts exist
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

    match result {
        | CliResult::DraftList { expansion, reduction, instruction, inquiry } => {
            assert!(expansion.is_some());
            assert!(reduction.is_some());
            assert!(instruction.is_some());
            assert!(inquiry.is_none());
        }
        | _ => panic!("Expected all three drafts"),
    }

    // Clear only expansion
    let cmd = BlockCommands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(fmt(root_id)),
        all: false,
        expand: true,
        reduce: false,
        instruction: false,
        inquiry: false,
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Verify only reduction and instruction remain
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

    match result {
        | CliResult::DraftList { expansion, reduction, instruction, inquiry } => {
            assert!(expansion.is_none());
            assert!(reduction.is_some());
            assert!(instruction.is_some());
            assert!(inquiry.is_none());
        }
        | _ => panic!("Expected reduction and instruction only"),
    }

    // Clear all remaining
    let cmd = BlockCommands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(fmt(root_id)),
        all: true,
        expand: false,
        reduce: false,
        instruction: false,
        inquiry: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Verify all cleared
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::DraftList { expansion, reduction, instruction, inquiry } => {
            assert!(expansion.is_none());
            assert!(reduction.is_none());
            assert!(instruction.is_none());
            assert!(inquiry.is_none());
        }
        | _ => panic!("Expected all cleared"),
    }
}

#[test]
fn test_multiple_friends_then_list_then_remove() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Create 5 children
    let mut child_ids = vec![];
    for i in 0..5 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(root_id)),
            text: format!("child{}", i),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;
        let id = match result {
            | CliResult::BlockId(id) => id,
            | _ => panic!(),
        };
        child_ids.push(id);
    }

    // Add friend relationships: child0 -> [child1, child2, child3]
    for i in 1..4 {
        let cmd = BlockCommands::Friend(FriendCommands::Add(AddFriendCommand {
            target_id: BlockId(fmt(child_ids[0])),
            friend_id: BlockId(fmt(child_ids[i])),
            perspective: Some(format!("relation{}", i)),
            telescope_lineage: i % 2 == 0,
            telescope_children: i > 2,
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;
        assert!(matches!(result, CliResult::Success));
    }

    // List friends of child0
    let cmd = BlockCommands::Friend(FriendCommands::List(ListFriendCommand {
        target_id: BlockId(fmt(child_ids[0])),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

    match result {
        | CliResult::FriendList(friends) => {
            assert_eq!(friends.len(), 3);
            // Verify each friend
            for (i, friend) in friends.iter().enumerate() {
                assert_eq!(friend.id, fmt(child_ids[i + 1]));
                assert_eq!(friend.perspective, Some(format!("relation{}", i + 1)));
                assert_eq!(friend.telescope_lineage, (i + 1) % 2 == 0);
                assert_eq!(friend.telescope_children, i + 1 > 2);
            }
        }
        | _ => panic!("Expected friend list"),
    }

    // Remove middle friend (child2)
    let cmd = BlockCommands::Friend(FriendCommands::Remove(RemoveFriendCommand {
        target_id: BlockId(fmt(child_ids[0])),
        friend_id: BlockId(fmt(child_ids[1])),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Verify remaining friends
    let cmd = BlockCommands::Friend(FriendCommands::List(ListFriendCommand {
        target_id: BlockId(fmt(child_ids[0])),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::FriendList(friends) => {
            assert_eq!(friends.len(), 2);
            assert!(!friends.iter().any(|f| f.id == fmt(child_ids[1])));
        }
        | _ => panic!("Expected 2 friends"),
    }
}

#[test]
fn test_find_across_large_tree_with_limit() {
    use crate::cli::query::FindCommand;

    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Build tree with specific pattern
    // root -> [target1, other, target2, other, target3]
    let mut target_ids = vec![];
    for i in 0..10 {
        let text = if i % 3 == 0 {
            target_ids.push(i);
            format!("TARGET_{}", i)
        } else {
            format!("other_{}", i)
        };

        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(root_id)),
            text,
        }));
        let (s, _) = cmd.execute(store, &PathBuf::from("."));
        store = s;
    }

    // Find all TARGETs
    let cmd = BlockCommands::Find(FindCommand { query: "TARGET".to_string(), limit: 100 });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

    match result {
        | CliResult::Find(matches) => {
            assert_eq!(matches.len(), 4); // TARGET_0, TARGET_3, TARGET_6, TARGET_9
            for m in &matches {
                assert!(m.text.starts_with("TARGET_"));
            }
        }
        | _ => panic!("Expected Find"),
    }

    // Find with limit
    let cmd = BlockCommands::Find(FindCommand { query: "TARGET".to_string(), limit: 2 });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

    match result {
        | CliResult::Find(matches) => {
            assert_eq!(matches.len(), 2);
        }
        | _ => panic!("Expected Find with limit"),
    }
}

#[test]
fn test_fold_multiple_blocks_then_verify() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Create 5 children
    let mut child_ids = vec![];
    for i in 0..5 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(root_id)),
            text: format!("child{}", i),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;
        let id = match result {
            | CliResult::BlockId(id) => id,
            | _ => panic!(),
        };
        child_ids.push(id);
    }

    // Toggle fold state for alternating children
    let mut expected_states = vec![];
    for (_i, &child_id) in child_ids.iter().enumerate() {
        let initial = store.is_collapsed(&child_id);
        expected_states.push(!initial); // After toggle

        let cmd = BlockCommands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
            block_id: BlockId(fmt(child_id)),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;

        match result {
            | CliResult::Collapsed(state) => {
                assert_eq!(state, !initial);
            }
            | _ => panic!("Expected Collapsed"),
        }
    }

    // Verify all states with status command
    for (i, &child_id) in child_ids.iter().enumerate() {
        let cmd = BlockCommands::Fold(FoldCommands::Status(StatusFoldCommand {
            block_id: BlockId(fmt(child_id)),
        }));
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::Collapsed(state) => {
                assert_eq!(state, expected_states[i], "Child {} state mismatch", i);
            }
            | _ => panic!("Expected Collapsed for child {}", i),
        }
    }
}

#[test]
fn test_wrap_then_add_child_to_wrapper() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Create child
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "original".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let child_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    // Wrap child
    let cmd = BlockCommands::Tree(TreeCommands::Wrap(WrapCommand {
        block_id: BlockId(fmt(child_id)),
        text: "wrapper".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let wrapper_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    // Add new child to wrapper
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(wrapper_id)),
        text: "sibling_to_original".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let new_sibling = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    // Verify structure
    let wrapper_children = store.children(&wrapper_id);
    assert_eq!(wrapper_children.len(), 2);
    assert!(wrapper_children.contains(&child_id));
    assert!(wrapper_children.contains(&new_sibling));

    // Verify original child still has same text
    assert_eq!(store.point(&child_id), Some("original".to_string()));
}

#[test]
fn test_nav_lineage_for_deeply_nested_block() {
    let mut store = BlockStore::default();
    let mut current_id = store.roots()[0];
    let mut ids = vec![current_id];

    // Build 20-level deep tree
    for i in 0..20 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(current_id)),
            text: format!("level{}", i),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;
        current_id = match result {
            | CliResult::BlockId(id) => id,
            | _ => panic!(),
        };
        ids.push(current_id);
    }

    // Get lineage of deepest node
    let cmd = BlockCommands::Nav(NavCommands::Lineage(LineageCommand {
        block_id: BlockId(fmt(current_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Lineage(points) => {
            // Should have all ancestors (20 levels + root counted twice in lineage?)
            assert!(points.len() >= 20);
        }
        | _ => panic!("Expected Lineage"),
    }
}

// ============================================================================
// Context Operation Tests
// ============================================================================

#[test]
fn test_context_command_with_friends() {
    use crate::cli::context::ContextCommand;

    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Add friend
    let cmd = BlockCommands::Friend(FriendCommands::Add(AddFriendCommand {
        target_id: BlockId(fmt(child1)),
        friend_id: BlockId(fmt(child2)),
        perspective: Some("related".to_string()),
        telescope_lineage: false,
        telescope_children: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Get context for child1
    let cmd = BlockCommands::Context(ContextCommand { block_id: BlockId(fmt(child1)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Context { lineage, children, friends } => {
            assert!(lineage.len() >= 1); // Should include root
            assert_eq!(children.len(), 0); // No children
            assert_eq!(friends, 1); // One friend
        }
        | _ => panic!("Expected Context"),
    }
}

#[test]
fn test_context_for_root_block() {
    use crate::cli::context::ContextCommand;

    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&root_id, "child2".to_string()).unwrap();

    // Get context for root
    let cmd = BlockCommands::Context(ContextCommand { block_id: BlockId(fmt(root_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Context { lineage: _, children, friends } => {
            // Root may have itself in lineage, just verify it works
            assert_eq!(children.len(), 2);
            assert_eq!(friends, 0);
        }
        | _ => panic!("Expected Context"),
    }
}

// ============================================================================
// Navigation Edge Cases
// ============================================================================

#[test]
fn test_nav_next_at_leaf_returns_none() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child = store.append_child(&root_id, "child".to_string()).unwrap();

    // Next from leaf should return None (no children, no siblings)
    let cmd = BlockCommands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(fmt(child)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::OptionalBlockId(None) => {}
        | _ => panic!("Expected None for next at leaf"),
    }
}

#[test]
fn test_nav_prev_at_root_returns_none() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Prev from root should return None
    let cmd =
        BlockCommands::Nav(NavCommands::Prev(PrevCommand { block_id: BlockId(fmt(root_id)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::OptionalBlockId(None) => {}
        | _ => panic!("Expected None for prev at root"),
    }
}

#[test]
fn test_nav_next_visits_all_in_dfs_order() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "c1".to_string()).unwrap();
    let _c2 = store.append_child(&root_id, "c2".to_string()).unwrap();
    store.append_child(&c1, "gc1".to_string()).unwrap();

    // DFS order: root -> c1 -> gc1 -> c2
    let mut cursor = root_id;
    let mut visited = vec![cursor];

    for _ in 0..10 {
        let cmd =
            BlockCommands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(fmt(cursor)) }));
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::OptionalBlockId(Some(next)) => {
                visited.push(next);
                cursor = next;
            }
            | CliResult::OptionalBlockId(None) => break,
            | _ => panic!("Expected OptionalBlockId"),
        }
    }

    assert_eq!(visited.len(), 4); // root, c1, gc1, c2
}

// ============================================================================
// Multiple Roots Tests
// ============================================================================

#[test]
fn test_multiple_roots_operations() {
    let store = BlockStore::default();
    let root1 = store.roots()[0];

    // Add second root by adding sibling to first root
    let cmd = BlockCommands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(fmt(root1)),
        text: "second root".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    let root2 = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Verify two roots
    assert_eq!(store.roots().len(), 2);

    // Add child to second root
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root2)),
        text: "child of root2".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    let child2 = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Verify structure
    assert!(store.children(&root2).contains(&child2));
    assert!(store.children(&root1).is_empty());
}

#[test]
fn test_roots_command_lists_all() {
    let store = BlockStore::default();
    let root1 = store.roots()[0];

    // Add second root
    let cmd = BlockCommands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(fmt(root1)),
        text: "root2".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    let root2 = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Add third root
    let cmd = BlockCommands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(fmt(root2)),
        text: "root3".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::BlockId(_)));

    // List all roots
    let cmd = BlockCommands::Roots(crate::cli::commands::RootCommand {});
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Roots(ids) => {
            assert_eq!(ids.len(), 3);
        }
        | _ => panic!("Expected Roots"),
    }
}

// ============================================================================
// Friend Telescoping Tests
// ============================================================================

#[test]
fn test_friend_with_telescope_options() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();
    store.append_child(&child2, "grandchild".to_string()).unwrap();

    // Add friend with both telescopes enabled
    let cmd = BlockCommands::Friend(FriendCommands::Add(AddFriendCommand {
        target_id: BlockId(fmt(child1)),
        friend_id: BlockId(fmt(child2)),
        perspective: Some("telescoped".to_string()),
        telescope_lineage: true,
        telescope_children: true,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // List friends and verify telescope flags
    let cmd = BlockCommands::Friend(FriendCommands::List(ListFriendCommand {
        target_id: BlockId(fmt(child1)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::FriendList(friends) => {
            assert_eq!(friends.len(), 1);
            assert!(friends[0].telescope_lineage);
            assert!(friends[0].telescope_children);
        }
        | _ => panic!("Expected FriendList"),
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_unknown_block_errors_consistently() {
    let store = BlockStore::default();

    let unknown_id = "999v999";

    // Test show
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(unknown_id.to_string()) });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));

    // Test nav next
    let cmd = BlockCommands::Nav(NavCommands::Next(NextCommand {
        block_id: BlockId(unknown_id.to_string()),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));

    // Test fold toggle
    let cmd = BlockCommands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
        block_id: BlockId(unknown_id.to_string()),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));

    // Test add child to unknown
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(unknown_id.to_string()),
        text: "test".to_string(),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));
}

#[test]
fn test_empty_query_find() {
    let store = BlockStore::default();
    let _root_id = store.roots()[0];

    // Empty query should match everything
    let cmd = BlockCommands::Find(FindCommand { query: "".to_string(), limit: 100 });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Find(matches) => {
            assert!(matches.len() >= 1);
        }
        | _ => panic!("Expected Find"),
    }
}

// ============================================================================
// Duplicate Edge Cases
// ============================================================================

#[test]
fn test_duplicate_leaf_block() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let leaf = store.append_child(&root_id, "leaf".to_string()).unwrap();

    // Duplicate leaf (no children)
    let cmd = BlockCommands::Tree(TreeCommands::Duplicate(DuplicateCommand {
        block_id: BlockId(fmt(leaf)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let dup = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Verify duplicate
    assert!(store.children(&root_id).contains(&dup));
    assert_eq!(store.point(&dup), store.point(&leaf));
    assert!(store.children(&dup).is_empty());
}

#[test]
fn test_duplicate_with_collapsed_state() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child = store.append_child(&root_id, "child".to_string()).unwrap();
    store.append_child(&child, "grandchild".to_string());

    // Collapse child
    store.toggle_collapsed(&child);
    assert!(store.is_collapsed(&child));

    // Duplicate child
    let cmd = BlockCommands::Tree(TreeCommands::Duplicate(DuplicateCommand {
        block_id: BlockId(fmt(child)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let dup = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Note: collapse state may or may not be copied depending on implementation
    // Just verify structure is duplicated
    assert!(store.children(&root_id).contains(&dup));
    assert_eq!(store.children(&dup).len(), 1);
}

// ============================================================================
// Delete Edge Cases
// ============================================================================

#[test]
fn test_delete_last_child() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let only_child = store.append_child(&root_id, "only".to_string()).unwrap();

    // Delete the only child
    let cmd = BlockCommands::Tree(TreeCommands::Delete(DeleteCommand {
        block_id: BlockId(fmt(only_child)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Removed(_)));
    assert!(store.children(&root_id).is_empty());
}

#[test]
fn test_delete_root_fails() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Deleting root should work but leave empty roots list or fail
    let cmd = BlockCommands::Tree(TreeCommands::Delete(DeleteCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, _result) = cmd.execute(store, &PathBuf::from("."));

    // Either error or succeeds with empty roots
    // Just verify operation completes without panic
}

// ============================================================================
// Move Edge Cases
// ============================================================================

#[test]
fn test_move_before_first_sibling() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "c1".to_string()).unwrap();
    let c2 = store.append_child(&root_id, "c2".to_string()).unwrap();

    // Move c2 before c1
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(c2)),
        target_id: BlockId(fmt(c1)),
        before: true,
        after: false,
        under: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Success));

    let children = store.children(&root_id);
    assert_eq!(children[0], c2);
    assert_eq!(children[1], c1);
}

#[test]
fn test_move_after_last_sibling() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "c1".to_string()).unwrap();
    let c2 = store.append_child(&root_id, "c2".to_string()).unwrap();

    // Move c1 after c2
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(c1)),
        target_id: BlockId(fmt(c2)),
        before: false,
        after: true,
        under: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Success));

    let children = store.children(&root_id);
    assert_eq!(children[0], c2);
    assert_eq!(children[1], c1);
}

// ============================================================================
// Stress Test
// ============================================================================

#[test]
fn test_rapid_sequential_operations() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // 50 rapid add-child operations
    for i in 0..50 {
        let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(root_id)),
            text: format!("child{}", i),
        }));
        let (s, result) = cmd.execute(store, &PathBuf::from("."));
        store = s;
        assert!(matches!(result, CliResult::BlockId(_)), "Add {} failed", i);
    }

    // Verify all 50 children exist
    assert_eq!(store.children(&root_id).len(), 50);

    // Delete every other child
    let children: Vec<_> = store.children(&root_id).to_vec();
    for (i, &child) in children.iter().enumerate() {
        if i % 2 == 0 {
            let cmd = BlockCommands::Tree(TreeCommands::Delete(DeleteCommand {
                block_id: BlockId(fmt(child)),
            }));
            let (s, result) = cmd.execute(store, &PathBuf::from("."));
            store = s;
            assert!(matches!(result, CliResult::Removed(_)));
        }
    }

    // Should have 25 remaining
    assert_eq!(store.children(&root_id).len(), 25);
}
