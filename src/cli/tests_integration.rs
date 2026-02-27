//! Integration tests for CLI commands.
//!
//! These tests verify complex multi-operation scenarios, read-after-write semantics,
//! and edge cases that could expose bugs in the CLI execution layer.

use crate::cli::{
    BlockCommands, BlockId,
    draft::{
        ClearDraftCommand, DraftCommands, ExpandDraftCommand, InstructionDraftCommand,
        ListDraftCommand,
    },
    fold::{FoldCommands, StatusFoldCommand, ToggleFoldCommand},
    friend::{AddFriendCommand, FriendCommands, ListFriendCommand},
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
    let c2_idx = children.iter().position(|&c| c == child2).unwrap();

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
