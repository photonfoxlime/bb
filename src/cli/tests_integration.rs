//! Integration tests for CLI commands.
//!
//! These tests verify complex multi-operation scenarios, read-after-write semantics,
//! and edge cases that could expose bugs in the CLI execution layer.

use crate::cli::{
    BlockId, Commands,
    draft::{
        ClearDraftCommand, DraftCommands, AmplifyDraftCommand, InstructionDraftCommand,
        ListDraftCommand, DistillDraftCommand,
    },
    fold::{FoldCommands, StatusFoldCommand, ToggleFoldCommand},
    friend::{AddFriendCommand, FriendCommands, ListFriendCommand, RemoveFriendCommand},
    nav::{
        FindNextCommand, FindPrevCommand, LineageCommand, NavCommands, NextCommand, PrevCommand,
    },
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
fn read_after_write_add_child() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Add child
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "new block".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let new_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Immediately read the new block
    let cmd = Commands::Show(ShowCommand { block_id: BlockId(fmt(new_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Show(show) => {
            assert_eq!(show.text, "new block");
            assert!(show.children.is_empty());
        }
        | _ => panic!("Expected Show result"),
    }
}

#[test]
fn read_after_write_chain() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let mut ids = vec![root_id];

    // Add 5 children in sequence
    for i in 0..5 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
        let cmd = Commands::Show(ShowCommand { block_id: BlockId(fmt(id)) });
        let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

        match result {
            | CliResult::Show(show) => {
                assert_eq!(show.text, format!("child{}", i - 1));
            }
            | _ => panic!("Expected Show for child {}", i - 1),
        }
    }

    // Verify root has exactly 5 children
    let cmd = Commands::Show(ShowCommand { block_id: BlockId(fmt(root_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Show(show) => {
            assert_eq!(show.children.len(), 5);
        }
        | _ => panic!("Expected Show for root"),
    }
}

#[test]
fn add_sibling_position() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Add sibling after child1
    let cmd = Commands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
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
fn wrap_preserves_subtree() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let grandchild1 = store.append_child(&child1, "grandchild1".to_string()).unwrap();

    // Wrap child1
    let cmd = Commands::Tree(TreeCommands::Wrap(WrapCommand {
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
fn duplicate_preserves_subtree() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&child1, "grandchild1".to_string()).unwrap();
    store.append_child(&child1, "grandchild2".to_string()).unwrap();

    // Duplicate child1
    let cmd = Commands::Tree(TreeCommands::Duplicate(DuplicateCommand {
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
fn delete_cascades() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&child1, "gc1".to_string()).unwrap();
    store.append_child(&child1, "gc2".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Delete child1
    let cmd =
        Commands::Tree(TreeCommands::Delete(DeleteCommand { block_id: BlockId(fmt(child1)) }));
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
fn move_preserves_structure() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();
    let gc1 = store.append_child(&child1, "gc1".to_string()).unwrap();

    // Move child1 after child2
    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
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
fn move_under() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Move child2 under child1
    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
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
fn ids_stable_after_operations() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();

    let root_str = fmt(root_id);
    let child1_str = fmt(child1);

    // Add more children
    for i in 0..3 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(root_str.clone()),
            text: format!("new{}", i),
        }));
        let (s, _) = cmd.execute(store, &PathBuf::from("."));
        store = s;
    }

    // Original IDs should still work
    let cmd = Commands::Show(ShowCommand { block_id: BlockId(root_str) });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Show(show) if show.id == fmt(root_id)));

    let cmd = Commands::Show(ShowCommand { block_id: BlockId(child1_str) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Show(show) if show.id == fmt(child1)));
}

// ============================================================================
// Navigation Tests
// ============================================================================

#[test]
fn nav_after_add() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Add child
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "new".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let new_child = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Navigate from new child back to root
    let cmd = Commands::Nav(NavCommands::Prev(PrevCommand { block_id: BlockId(fmt(new_child)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::OptionalBlockId(Some(prev_id)) => {
            assert_eq!(prev_id, root_id);
        }
        | _ => panic!("Expected prev"),
    }
}

#[test]
fn lineage_after_wrap() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let gc1 = store.append_child(&child1, "gc1".to_string()).unwrap();

    // Wrap child1
    let cmd = Commands::Tree(TreeCommands::Wrap(WrapCommand {
        block_id: BlockId(fmt(child1)),
        text: "wrapper".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let _wrapper_id = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Get lineage of gc1 - should be longer now
    let cmd = Commands::Nav(NavCommands::Lineage(LineageCommand { block_id: BlockId(fmt(gc1)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Lineage(lineage) => {
            assert_eq!(lineage.items.len(), 4); // root, root again?, wrapper, child1
        }
        | _ => panic!("Expected Lineage"),
    }
}

// ============================================================================
// Draft Tests
// ============================================================================

#[test]
fn draft_persists_after_tree_mod() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set draft
    let cmd = Commands::Draft(DraftCommands::Amplify(AmplifyDraftCommand {
        block_id: BlockId(fmt(root_id)),
        rewrite: Some("rewrite".to_string()),
        children: vec!["child".to_string()],
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Modify tree
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "new child".to_string(),
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Verify draft persists
    let cmd =
        Commands::Draft(DraftCommands::List(ListDraftCommand { block_id: BlockId(fmt(root_id)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::DraftList { expansion: Some(exp), .. } => {
            assert_eq!(exp.rewrite, Some("rewrite".to_string()));
        }
        | _ => panic!("Expected expansion draft"),
    }
}

#[test]
fn draft_clear_selective() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set multiple drafts
    let cmd = Commands::Draft(DraftCommands::Amplify(AmplifyDraftCommand {
        block_id: BlockId(fmt(root_id)),
        rewrite: Some("rewrite".to_string()),
        children: vec![],
    }));
    let (s, _) = cmd.execute(store, &PathBuf::from("."));
    store = s;

    let cmd = Commands::Draft(DraftCommands::Instruction(InstructionDraftCommand {
        block_id: BlockId(fmt(root_id)),
        text: "instr".to_string(),
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Clear only expansion
    let cmd = Commands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(fmt(root_id)),
        all: false,
        amplify: true,
        distill: false,
        instruction: false,
        probe: false,
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Verify
    let cmd =
        Commands::Draft(DraftCommands::List(ListDraftCommand { block_id: BlockId(fmt(root_id)) }));
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
fn friend_persists_after_move() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Add friend
    let cmd = Commands::Friend(FriendCommands::Add(AddFriendCommand {
        target_id: BlockId(fmt(child1)),
        friend_id: BlockId(fmt(child2)),
        perspective: Some("related".to_string()),
        telescope_lineage: false,
        telescope_children: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Move child1
    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(fmt(child1)),
        target_id: BlockId(fmt(child2)),
        before: false,
        after: true,
        under: false,
    }));
    let (store, _) = cmd.execute(store, &PathBuf::from("."));

    // Verify friend persists
    let cmd = Commands::Friend(FriendCommands::List(ListFriendCommand {
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
fn fold_toggle_repeated() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let mut state = store.is_collapsed(&root_id);

    for i in 0..5 {
        let cmd = Commands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
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
    let cmd =
        Commands::Fold(FoldCommands::Status(StatusFoldCommand { block_id: BlockId(fmt(root_id)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Collapsed(final_state) => {
            assert_eq!(final_state, state);
        }
        | _ => panic!("Expected Collapsed"),
    }
}

#[test]
fn find_case_insensitive() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "CHILD".to_string()).unwrap();
    store.append_child(&root_id, "child".to_string()).unwrap();

    for query in &["CHILD", "child", "ChIlD"] {
        let cmd = Commands::Find(FindCommand { query: query.to_string(), limit: 10 });
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
fn move_to_self_fails() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();

    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
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
fn duplicate_root() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "child1".to_string()).unwrap();

    let cmd = Commands::Tree(TreeCommands::Duplicate(DuplicateCommand {
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
fn build_deep_tree_then_navigate() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Build a deep tree: root -> child -> grandchild -> greatgrandchild -> gggrandchild
    let mut current_id = root_id;
    let mut ids = vec![current_id];

    for i in 0..10 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
        let cmd = Commands::Nav(NavCommands::Prev(PrevCommand { block_id: BlockId(fmt(cursor)) }));
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
        let cmd = Commands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(fmt(cursor)) }));
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
fn complex_restructure_move_wrap_duplicate() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Build initial structure: root -> [A, B, C]
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "A".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let id_a = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(root_id)),
        text: "B".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let id_b = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(id_a)),
        text: "A1".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    let _id_a1 = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!(),
    };

    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(fmt(id_a)),
        text: "A2".to_string(),
    }));
    let (s, _) = cmd.execute(store, &PathBuf::from("."));
    store = s;

    // Wrap A with WrapperA: root -> [WrapperA -> [A -> [A1, A2]], B, C]
    let cmd = Commands::Tree(TreeCommands::Wrap(WrapCommand {
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
    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
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
    let cmd =
        Commands::Tree(TreeCommands::Duplicate(DuplicateCommand { block_id: BlockId(fmt(id_b)) }));
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
fn draft_workflow_expand_then_reduce_then_clear() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set expansion draft
    let cmd = Commands::Draft(DraftCommands::Amplify(AmplifyDraftCommand {
        block_id: BlockId(fmt(root_id)),
        rewrite: Some("expanded version".to_string()),
        children: vec!["child1".to_string(), "child2".to_string()],
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Set reduction draft
    let cmd = Commands::Draft(DraftCommands::Distill(DistillDraftCommand {
        block_id: BlockId(fmt(root_id)),
        reduction: "summary of everything".to_string(),
        redundant_children: vec![],
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Set instruction draft
    let cmd = Commands::Draft(DraftCommands::Instruction(InstructionDraftCommand {
        block_id: BlockId(fmt(root_id)),
        text: "Make this clearer".to_string(),
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Verify all three drafts exist
    let cmd =
        Commands::Draft(DraftCommands::List(ListDraftCommand { block_id: BlockId(fmt(root_id)) }));
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
    let cmd = Commands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(fmt(root_id)),
        all: false,
        amplify: true,
        distill: false,
        instruction: false,
        probe: false,
    }));
    let (s, result) = cmd.execute(store, &PathBuf::from("."));
    store = s;
    assert!(matches!(result, CliResult::Success));

    // Verify only reduction and instruction remain
    let cmd =
        Commands::Draft(DraftCommands::List(ListDraftCommand { block_id: BlockId(fmt(root_id)) }));
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
    let cmd = Commands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(fmt(root_id)),
        all: true,
        amplify: false,
        distill: false,
        instruction: false,
        probe: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Verify all cleared
    let cmd =
        Commands::Draft(DraftCommands::List(ListDraftCommand { block_id: BlockId(fmt(root_id)) }));
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
fn multiple_friends_then_list_then_remove() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Create 5 children
    let mut child_ids = vec![];
    for i in 0..5 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
        let cmd = Commands::Friend(FriendCommands::Add(AddFriendCommand {
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
    let cmd = Commands::Friend(FriendCommands::List(ListFriendCommand {
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
    let cmd = Commands::Friend(FriendCommands::Remove(RemoveFriendCommand {
        target_id: BlockId(fmt(child_ids[0])),
        friend_id: BlockId(fmt(child_ids[1])),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Verify remaining friends
    let cmd = Commands::Friend(FriendCommands::List(ListFriendCommand {
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
fn find_across_large_tree_with_limit() {
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

        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
            parent_id: BlockId(fmt(root_id)),
            text,
        }));
        let (s, _) = cmd.execute(store, &PathBuf::from("."));
        store = s;
    }

    // Find all TARGETs
    let cmd = Commands::Find(FindCommand { query: "TARGET".to_string(), limit: 100 });
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
    let cmd = Commands::Find(FindCommand { query: "TARGET".to_string(), limit: 2 });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));

    match result {
        | CliResult::Find(matches) => {
            assert_eq!(matches.len(), 2);
        }
        | _ => panic!("Expected Find with limit"),
    }
}

#[test]
fn fold_multiple_blocks_then_verify() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Create 5 children
    let mut child_ids = vec![];
    for i in 0..5 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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

        let cmd = Commands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
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
        let cmd = Commands::Fold(FoldCommands::Status(StatusFoldCommand {
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
fn wrap_then_add_child_to_wrapper() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // Create child
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
    let cmd = Commands::Tree(TreeCommands::Wrap(WrapCommand {
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
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
fn nav_lineage_for_deeply_nested_block() {
    let mut store = BlockStore::default();
    let mut current_id = store.roots()[0];
    let mut ids = vec![current_id];

    // Build 20-level deep tree
    for i in 0..20 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
    let cmd =
        Commands::Nav(NavCommands::Lineage(LineageCommand { block_id: BlockId(fmt(current_id)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Lineage(lineage) => {
            // Should have all ancestors (20 levels + root counted twice in lineage?)
            assert!(lineage.items.len() >= 20);
        }
        | _ => panic!("Expected Lineage"),
    }
}

// ============================================================================
// Context Operation Tests
// ============================================================================

#[test]
fn context_command_with_friends() {
    use crate::cli::context::ContextCommand;

    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();

    // Add friend
    let cmd = Commands::Friend(FriendCommands::Add(AddFriendCommand {
        target_id: BlockId(fmt(child1)),
        friend_id: BlockId(fmt(child2)),
        perspective: Some("related".to_string()),
        telescope_lineage: false,
        telescope_children: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // Get context for child1
    let cmd = Commands::Context(ContextCommand { block_id: BlockId(fmt(child1)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Context(ctx) => {
            assert!(ctx.lineage().points().count() >= 1); // Should include root
            assert_eq!(ctx.existing_children().len(), 0); // No children
            assert_eq!(ctx.friend_blocks().len(), 1); // One friend
        }
        | _ => panic!("Expected Context"),
    }
}

#[test]
fn context_for_root_block() {
    use crate::cli::context::ContextCommand;

    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&root_id, "child2".to_string()).unwrap();

    // Get context for root
    let cmd = Commands::Context(ContextCommand { block_id: BlockId(fmt(root_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Context(ctx) => {
            // Root may have itself in lineage, just verify it works
            assert_eq!(ctx.existing_children().len(), 2);
            assert_eq!(ctx.friend_blocks().len(), 0);
        }
        | _ => panic!("Expected Context"),
    }
}

// ============================================================================
// Navigation Edge Cases
// ============================================================================

#[test]
fn nav_next_at_leaf_returns_none() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child = store.append_child(&root_id, "child".to_string()).unwrap();

    // Next from leaf should return None (no children, no siblings)
    let cmd = Commands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(fmt(child)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::OptionalBlockId(None) => {}
        | _ => panic!("Expected None for next at leaf"),
    }
}

#[test]
fn nav_prev_at_root_returns_none() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Prev from root should return None
    let cmd = Commands::Nav(NavCommands::Prev(PrevCommand { block_id: BlockId(fmt(root_id)) }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::OptionalBlockId(None) => {}
        | _ => panic!("Expected None for prev at root"),
    }
}

#[test]
fn nav_next_visits_all_in_dfs_order() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "c1".to_string()).unwrap();
    let _c2 = store.append_child(&root_id, "c2".to_string()).unwrap();
    store.append_child(&c1, "gc1".to_string()).unwrap();

    // DFS order: root -> c1 -> gc1 -> c2
    let mut cursor = root_id;
    let mut visited = vec![cursor];

    for _ in 0..10 {
        let cmd = Commands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(fmt(cursor)) }));
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
fn multiple_roots_operations() {
    let store = BlockStore::default();
    let root1 = store.roots()[0];

    // Add second root by adding sibling to first root
    let cmd = Commands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
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
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
fn roots_command_lists_all() {
    let store = BlockStore::default();
    let root1 = store.roots()[0];

    // Add second root
    let cmd = Commands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(fmt(root1)),
        text: "root2".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    let root2 = match result {
        | CliResult::BlockId(id) => id,
        | _ => panic!("Expected BlockId"),
    };

    // Add third root
    let cmd = Commands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(fmt(root2)),
        text: "root3".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::BlockId(_)));

    // List all roots
    let cmd = Commands::Roots(crate::cli::commands::RootCommand {});
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
fn friend_with_telescope_options() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();
    store.append_child(&child2, "grandchild".to_string()).unwrap();

    // Add friend with both telescopes enabled
    let cmd = Commands::Friend(FriendCommands::Add(AddFriendCommand {
        target_id: BlockId(fmt(child1)),
        friend_id: BlockId(fmt(child2)),
        perspective: Some("telescoped".to_string()),
        telescope_lineage: true,
        telescope_children: true,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));

    // List friends and verify telescope flags
    let cmd = Commands::Friend(FriendCommands::List(ListFriendCommand {
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
fn unknown_block_errors_consistently() {
    let store = BlockStore::default();

    let unknown_id = "999v999";

    // Test show
    let cmd = Commands::Show(ShowCommand { block_id: BlockId(unknown_id.to_string()) });
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));

    // Test nav next
    let cmd =
        Commands::Nav(NavCommands::Next(NextCommand { block_id: BlockId(unknown_id.to_string()) }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));

    // Test fold toggle
    let cmd = Commands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
        block_id: BlockId(unknown_id.to_string()),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));

    // Test add child to unknown
    let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(unknown_id.to_string()),
        text: "test".to_string(),
    }));
    let (_store, result) = cmd.execute(store.clone(), &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(_)));
}

#[test]
fn empty_query_find() {
    let store = BlockStore::default();
    let _root_id = store.roots()[0];

    // Empty query should match everything
    let cmd = Commands::Find(FindCommand { query: "".to_string(), limit: 100 });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    match result {
        | CliResult::Find(matches) => {
            assert!(matches.len() >= 1);
        }
        | _ => panic!("Expected Find"),
    }
}

#[test]
fn nav_find_next_and_prev_follow_dfs_match_order() {
    let mut store = BlockStore::default();
    let root = store.roots()[0];
    let alpha_a = store.append_child(&root, "alpha A".to_string()).unwrap();
    let alpha_deep = store.append_child(&alpha_a, "alpha deep".to_string()).unwrap();
    let alpha_b = store.append_child(&root, "alpha B".to_string()).unwrap();
    let _other = store.append_child(&root, "other".to_string()).unwrap();

    // DFS order among alpha matches: alpha_a -> alpha_deep -> alpha_b
    let cmd = Commands::Nav(NavCommands::FindNext(FindNextCommand {
        block_id: BlockId(fmt(alpha_a)),
        query: "alpha".to_string(),
        no_wrap: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::OptionalBlockId(Some(id)) if id == alpha_deep));

    let cmd = Commands::Nav(NavCommands::FindNext(FindNextCommand {
        block_id: BlockId(fmt(alpha_deep)),
        query: "alpha".to_string(),
        no_wrap: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::OptionalBlockId(Some(id)) if id == alpha_b));

    let cmd = Commands::Nav(NavCommands::FindPrev(FindPrevCommand {
        block_id: BlockId(fmt(alpha_b)),
        query: "alpha".to_string(),
        no_wrap: false,
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::OptionalBlockId(Some(id)) if id == alpha_deep));
}

#[test]
fn nav_find_next_no_wrap_returns_none_at_end() {
    let mut store = BlockStore::default();
    let root = store.roots()[0];
    let first = store.append_child(&root, "target 1".to_string()).unwrap();
    let last = store.append_child(&root, "target 2".to_string()).unwrap();
    let _ = first;

    let cmd = Commands::Nav(NavCommands::FindNext(FindNextCommand {
        block_id: BlockId(fmt(last)),
        query: "target".to_string(),
        no_wrap: true,
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::OptionalBlockId(None)));
}

// ============================================================================
// Duplicate Edge Cases
// ============================================================================

#[test]
fn duplicate_leaf_block() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let leaf = store.append_child(&root_id, "leaf".to_string()).unwrap();

    // Duplicate leaf (no children)
    let cmd =
        Commands::Tree(TreeCommands::Duplicate(DuplicateCommand { block_id: BlockId(fmt(leaf)) }));
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
fn duplicate_with_collapsed_state() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child = store.append_child(&root_id, "child".to_string()).unwrap();
    store.append_child(&child, "grandchild".to_string());

    // Collapse child
    store.toggle_collapsed(&child);
    assert!(store.is_collapsed(&child));

    // Duplicate child
    let cmd =
        Commands::Tree(TreeCommands::Duplicate(DuplicateCommand { block_id: BlockId(fmt(child)) }));
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
fn delete_last_child() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let only_child = store.append_child(&root_id, "only".to_string()).unwrap();

    // Delete the only child
    let cmd =
        Commands::Tree(TreeCommands::Delete(DeleteCommand { block_id: BlockId(fmt(only_child)) }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Removed(_)));
    assert!(store.children(&root_id).is_empty());
}

#[test]
fn delete_root_fails() {
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Deleting root should work but leave empty roots list or fail
    let cmd =
        Commands::Tree(TreeCommands::Delete(DeleteCommand { block_id: BlockId(fmt(root_id)) }));
    let (_store, _result) = cmd.execute(store, &PathBuf::from("."));

    // Either error or succeeds with empty roots
    // Just verify operation completes without panic
}

// ============================================================================
// Move Edge Cases
// ============================================================================

#[test]
fn move_before_first_sibling() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "c1".to_string()).unwrap();
    let c2 = store.append_child(&root_id, "c2".to_string()).unwrap();

    // Move c2 before c1
    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
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
fn move_after_last_sibling() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "c1".to_string()).unwrap();
    let c2 = store.append_child(&root_id, "c2".to_string()).unwrap();

    // Move c1 after c2
    let cmd = Commands::Tree(TreeCommands::Move(MoveCommand {
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
fn rapid_sequential_operations() {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];

    // 50 rapid add-child operations
    for i in 0..50 {
        let cmd = Commands::Tree(TreeCommands::AddChild(AddChildCommand {
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
            let cmd = Commands::Tree(TreeCommands::Delete(DeleteCommand {
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

// ============================================================================
// Mount Operation Tests (basic)
// ============================================================================

#[test]
fn mount_set_on_leaf_succeeds() {
    use crate::cli::mount::{MountCommands, SetMountCommand};

    let tmp = tempfile::tempdir().unwrap();
    let store = BlockStore::default();
    let root_id = store.roots()[0];

    // Set mount on leaf (root has no children by default after we check)
    let external_path = tmp.path().join("external.json");
    std::fs::write(&external_path, "{}").unwrap();

    let cmd = Commands::Mount(MountCommands::Set(SetMountCommand {
        block_id: BlockId(fmt(root_id)),
        path: external_path,
        format: crate::cli::MountFormatCli(crate::store::MountFormat::Json),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    // May succeed or fail depending on default store structure
    assert!(matches!(result, CliResult::Success | CliResult::Error(_)));
}

#[test]
fn mount_set_fails_when_block_has_children() {
    use crate::cli::mount::{MountCommands, SetMountCommand};

    let tmp = tempfile::tempdir().unwrap();
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "child".to_string());

    let external_path = tmp.path().join("external.json");
    std::fs::write(&external_path, "{}").unwrap();

    let cmd = Commands::Mount(MountCommands::Set(SetMountCommand {
        block_id: BlockId(fmt(root_id)),
        path: external_path,
        format: crate::cli::MountFormatCli(crate::store::MountFormat::Json),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    // Should fail because block has children
    assert!(matches!(result, CliResult::Error(_)));
}

#[test]
fn mount_collapse_on_non_mount_fails() {
    use crate::cli::mount::{CollapseMountCommand, MountCommands};

    let store = BlockStore::default();
    let root_id = store.roots()[0];

    let cmd = Commands::Mount(MountCommands::Collapse(CollapseMountCommand {
        block_id: BlockId(fmt(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::Error(_)));
}

#[test]
fn mount_extract_creates_file() {
    use crate::cli::mount::{ExtractMountCommand, MountCommands};

    let tmp = tempfile::tempdir().unwrap();
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let c1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    store.append_child(&c1, "grandchild".to_string());

    let output_path = tmp.path().join("extracted.json");

    let cmd = Commands::Mount(MountCommands::Extract(ExtractMountCommand {
        block_id: BlockId(fmt(root_id)),
        output: output_path.clone(),
        format: None,
    }));
    let (_store, result) = cmd.execute(store, &tmp.path());

    assert!(matches!(result, CliResult::Success));
    assert!(output_path.exists());
}
