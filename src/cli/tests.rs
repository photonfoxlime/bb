//! End-to-end CLI command tests.
//!
//! These tests verify the complete CLI command execution flow:
//! - Command parsing and validation
//! - Store operations through `BlockCommands::execute()`
//! - Output formatting through `print_result()`
//!
//! Tests use the in-memory store to avoid modifying the real block store.

use crate::cli::{
    BlockCommands, BlockId, OutputFormat, print_result,
    commands::RootCommand,
    draft::{DraftCommands, ExpandDraftCommand, InstructionDraftCommand, ReduceDraftCommand, InquiryDraftCommand, ListDraftCommand, ClearDraftCommand},
    fold::{FoldCommands, StatusFoldCommand, ToggleFoldCommand},
    nav::{NavCommands, NextCommand, PrevCommand, LineageCommand},
    query::{ShowCommand, FindCommand},
    results::CliResult,
    tree::{TreeCommands, AddChildCommand, AddSiblingCommand, WrapCommand, DuplicateCommand, DeleteCommand, MoveCommand},
};
use crate::store::BlockStore;
use std::path::PathBuf;

fn create_test_store() -> BlockStore {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let child2 = store.append_child(&root_id, "child2".to_string()).unwrap();
    let _grandchild1 = store.append_child(&child1, "grandchild1".to_string()).unwrap();
    store
}

fn format_block_id(id: crate::store::BlockId) -> String {
    format!("{}", id)
}

#[test]
fn test_roots_command() {
    let store = create_test_store();
    let cmd = BlockCommands::Roots(RootCommand {});
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Roots(ids) if ids.len() == 1));
}

#[test]
fn test_show_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(format_block_id(root_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Show { id, text, children } 
        if id == root_id && text.contains("Tree of Thoughts") && children.len() == 2));
}

#[test]
fn test_show_unknown_block() {
    let store = create_test_store();
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId("0v0".to_string()) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(msg) if msg.contains("Unknown block ID")));
}

#[test]
fn test_find_command() {
    let store = create_test_store();
    let cmd = BlockCommands::Find(FindCommand { query: "child".to_string(), limit: 10 });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Find(matches) if matches.len() >= 2));
}

#[test]
fn test_add_child_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(format_block_id(root_id)),
        text: "new child".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::BlockId(new_id) if store.children(&root_id).contains(&new_id)));
}

#[test]
fn test_add_sibling_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(format_block_id(child_id)),
        text: "sibling".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::BlockId(new_id) if store.children(&root_id).contains(&new_id)));
}

#[test]
fn test_wrap_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Tree(TreeCommands::Wrap(WrapCommand {
        block_id: BlockId(format_block_id(child_id)),
        text: "wrapper".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::BlockId(wrapper_id) 
        if store.children(&root_id).contains(&wrapper_id) && store.children(&wrapper_id).contains(&child_id)));
}

#[test]
fn test_duplicate_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Tree(TreeCommands::Duplicate(DuplicateCommand {
        block_id: BlockId(format_block_id(child_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::BlockId(dup_id) 
        if store.children(&root_id).contains(&dup_id) && store.point(&dup_id) == store.point(&child_id)));
}

#[test]
fn test_delete_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let initial_count = store.children(&root_id).len();
    let cmd = BlockCommands::Tree(TreeCommands::Delete(DeleteCommand {
        block_id: BlockId(format_block_id(child_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Removed(ids) 
        if !ids.is_empty() && store.children(&root_id).len() == initial_count - 1));
}

#[test]
fn test_move_command_after() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let children = store.children(&root_id);
    let child1 = children[0];
    let child2 = children[1];
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(format_block_id(child1)),
        target_id: BlockId(format_block_id(child2)),
        before: false, after: true, under: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.children(&root_id).contains(&child1));
}

#[test]
fn test_nav_next_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Nav(NavCommands::Next(NextCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::OptionalBlockId(Some(next_id)) 
        if store.children(&root_id).contains(&next_id)));
}

#[test]
fn test_nav_prev_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Nav(NavCommands::Prev(PrevCommand {
        block_id: BlockId(format_block_id(child_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::OptionalBlockId(Some(prev_id)) if prev_id == root_id));
}

#[test]
fn test_nav_lineage_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Nav(NavCommands::Lineage(LineageCommand {
        block_id: BlockId(format_block_id(child_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Lineage(points) if !points.is_empty()));
}

#[test]
fn test_draft_expand_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Draft(DraftCommands::Expand(ExpandDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
        rewrite: Some("rewritten".to_string()),
        children: vec!["child a".to_string(), "child b".to_string()],
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.expansion_draft(&root_id).is_some());
}

#[test]
fn test_draft_reduce_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Draft(DraftCommands::Reduce(ReduceDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
        reduction: "summary".to_string(),
        redundant_children: vec![BlockId(format_block_id(child_id))],
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.reduction_draft(&root_id).is_some());
}

#[test]
fn test_draft_instruction_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Draft(DraftCommands::Instruction(InstructionDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
        text: "test instruction".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.instruction_draft(&root_id).is_some());
}

#[test]
fn test_draft_inquiry_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Draft(DraftCommands::Inquiry(InquiryDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
        response: "test response".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.inquiry_draft(&root_id).is_some());
}

#[test]
fn test_draft_list_command() {
    let mut store = create_test_store();
    let root_id = store.roots()[0];
    store.insert_expansion_draft(root_id, crate::store::ExpansionDraftRecord {
        rewrite: Some("test".to_string()), children: vec![],
    });
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::DraftList { expansion: Some(_), .. }));
}

#[test]
fn test_draft_clear_command() {
    let mut store = create_test_store();
    let root_id = store.roots()[0];
    store.insert_expansion_draft(root_id, crate::store::ExpansionDraftRecord {
        rewrite: None, children: vec![],
    });
    let cmd = BlockCommands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
        all: true, expand: false, reduce: false, instruction: false, inquiry: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.expansion_draft(&root_id).is_none());
}

#[test]
fn test_fold_toggle_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let initial = store.is_collapsed(&root_id);
    let cmd = BlockCommands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Collapsed(c) if c != initial && store.is_collapsed(&root_id) == c));
}

#[test]
fn test_fold_status_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Fold(FoldCommands::Status(StatusFoldCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Collapsed(c) if c == store.is_collapsed(&root_id)));
}

#[test]
fn test_output_json_format() {
    print_result(&CliResult::Roots(vec!["1v1".to_string(), "2v1".to_string()]), OutputFormat::Json);
}

#[test]
fn test_output_table_format() {
    print_result(&CliResult::Roots(vec!["1v1".to_string(), "2v1".to_string()]), OutputFormat::Table);
}

#[test]
fn test_output_error_to_stderr() {
    print_result(&CliResult::Error("test error".to_string()), OutputFormat::Table);
}

#[test]
fn test_output_success() {
    print_result(&CliResult::Success, OutputFormat::Table);
}

#[test]

#[test]
fn test_find_with_limit() {
    let store = create_test_store();
    let cmd = BlockCommands::Find(FindCommand { query: "".to_string(), limit: 1 });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Find(matches) if matches.len() == 1));
}

#[test]
fn test_move_with_unknown_source() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId("0v0".to_string()),
        target_id: BlockId(format_block_id(root_id)),
        before: false, after: false, under: false,
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(msg) if msg.contains("Unknown")));
}
