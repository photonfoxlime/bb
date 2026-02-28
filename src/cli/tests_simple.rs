//! End-to-end CLI command tests.
//!
//! These tests verify the complete CLI command execution flow:
//! - Command parsing and validation
//! - Store operations through `BlockCommands::execute()`
//! - Output formatting through `print_result()`
//!
//! Tests use the in-memory store to avoid modifying the real block store.

use crate::cli::{
    BlockCommands, BlockId, MountFormatCli, OutputFormat,
    commands::RootCommand,
    draft::{
        ClearDraftCommand, DraftCommands, ExpandDraftCommand, InquiryDraftCommand,
        InstructionDraftCommand, ListDraftCommand, ReduceDraftCommand,
    },
    fold::{FoldCommands, StatusFoldCommand, ToggleFoldCommand},
    mount::{
        ExpandMountCommand, ExtractMountCommand, InlineRecursiveMountCommand, MountCommands,
        MoveMountCommand, SaveMountsCommand, SetMountCommand,
    },
    nav::{
        FindNextCommand, FindPrevCommand, LineageCommand, NavCommands, NextCommand, PrevCommand,
    },
    point::EditPointCommand,
    print_result,
    query::{FindCommand, ShowCommand},
    results::CliResult,
    tree::{
        AddChildCommand, AddSiblingCommand, DeleteCommand, DuplicateCommand, MoveCommand,
        TreeCommands, WrapCommand,
    },
};
use crate::store::{BlockNode, BlockStore, MountFormat};
use std::path::PathBuf;

fn create_test_store() -> BlockStore {
    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    let child1 = store.append_child(&root_id, "child1".to_string()).unwrap();
    let _child2 = store.append_child(&root_id, "child2".to_string()).unwrap();
    let _grandchild1 = store.append_child(&child1, "grandchild1".to_string()).unwrap();
    store
}

fn format_block_id(id: crate::store::BlockId) -> String {
    format!("{}", id)
}

#[test]
fn roots_command() {
    let store = create_test_store();
    let cmd = BlockCommands::Roots(RootCommand {});
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Roots(ids) if ids.len() == 1));
}

#[test]
fn show_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId(format_block_id(root_id)) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Show { id, text, children }
        if id == root_id && text.contains("Tree of Thoughts") && children.len() == 2));
}

#[test]
fn show_unknown_block() {
    let store = create_test_store();
    let cmd = BlockCommands::Show(ShowCommand { block_id: BlockId("0v0".to_string()) });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(msg) if msg.contains("Unknown block ID")));
}

#[test]
fn find_command() {
    let store = create_test_store();
    let cmd = BlockCommands::Find(FindCommand { query: "child".to_string(), limit: 10 });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Find(matches) if matches.len() >= 2));
}

#[test]
fn point_edit_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Point(EditPointCommand {
        block_id: BlockId(format_block_id(root_id)),
        text: "Updated text".to_string(),
    });
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success));
    assert_eq!(store.point(&root_id), Some("Updated text".to_string()));
}

#[test]
fn point_edit_unknown_block() {
    let store = create_test_store();
    let cmd = BlockCommands::Point(EditPointCommand {
        block_id: BlockId("0v0".to_string()),
        text: "Should not work".to_string(),
    });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(msg) if msg.contains("Unknown block ID")));
}

#[test]
fn add_child_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Tree(TreeCommands::AddChild(AddChildCommand {
        parent_id: BlockId(format_block_id(root_id)),
        text: "new child".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(
        matches!(result, CliResult::BlockId(new_id) if store.children(&root_id).contains(&new_id))
    );
}

#[test]
fn add_sibling_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child_id = store.children(&root_id)[0];
    let cmd = BlockCommands::Tree(TreeCommands::AddSibling(AddSiblingCommand {
        block_id: BlockId(format_block_id(child_id)),
        text: "sibling".to_string(),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(
        matches!(result, CliResult::BlockId(new_id) if store.children(&root_id).contains(&new_id))
    );
}

#[test]
fn wrap_command() {
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
fn duplicate_command() {
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
fn delete_command() {
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
fn move_command_after() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let children = store.children(&root_id);
    let child1 = children[0];
    let child2 = children[1];
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId(format_block_id(child1)),
        target_id: BlockId(format_block_id(child2)),
        before: false,
        after: true,
        under: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.children(&root_id).contains(&child1));
}

#[test]
fn nav_next_command() {
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
fn nav_prev_command() {
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
fn nav_lineage_command() {
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
fn nav_find_next_wraps_by_default() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child2 = store.children(&root_id)[1];

    let cmd = BlockCommands::Nav(NavCommands::FindNext(FindNextCommand {
        block_id: BlockId(format_block_id(child2)),
        query: "child".to_string(),
        no_wrap: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));

    let expected = store.children(&root_id)[0];
    assert!(matches!(result, CliResult::OptionalBlockId(Some(id)) if id == expected));
}

#[test]
fn nav_find_prev_no_wrap_returns_none_without_earlier_match() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let child1 = store.children(&root_id)[0];

    let cmd = BlockCommands::Nav(NavCommands::FindPrev(FindPrevCommand {
        block_id: BlockId(format_block_id(child1)),
        query: "child".to_string(),
        no_wrap: true,
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));

    assert!(matches!(result, CliResult::OptionalBlockId(None)));
}

#[test]
fn draft_expand_command() {
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
fn draft_reduce_command() {
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
fn draft_instruction_command() {
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
fn draft_inquiry_command() {
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
fn draft_list_command() {
    let mut store = create_test_store();
    let root_id = store.roots()[0];
    store.insert_expansion_draft(
        root_id,
        crate::store::ExpansionDraftRecord { rewrite: Some("test".to_string()), children: vec![] },
    );
    let cmd = BlockCommands::Draft(DraftCommands::List(ListDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::DraftList { expansion: Some(_), .. }));
}

#[test]
fn draft_clear_command() {
    let mut store = create_test_store();
    let root_id = store.roots()[0];
    store.insert_expansion_draft(
        root_id,
        crate::store::ExpansionDraftRecord { rewrite: None, children: vec![] },
    );
    let cmd = BlockCommands::Draft(DraftCommands::Clear(ClearDraftCommand {
        block_id: BlockId(format_block_id(root_id)),
        all: true,
        expand: false,
        reduce: false,
        instruction: false,
        inquiry: false,
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Success) && store.expansion_draft(&root_id).is_none());
}

#[test]
fn fold_toggle_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let initial = store.is_collapsed(&root_id);
    let cmd = BlockCommands::Fold(FoldCommands::Toggle(ToggleFoldCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(
        matches!(result, CliResult::Collapsed(c) if c != initial && store.is_collapsed(&root_id) == c)
    );
}

#[test]
fn fold_status_command() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Fold(FoldCommands::Status(StatusFoldCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Collapsed(c) if c == store.is_collapsed(&root_id)));
}

#[test]
fn output_json_format() {
    print_result(&CliResult::Roots(vec!["1v1".to_string(), "2v1".to_string()]), OutputFormat::Json);
}

#[test]
fn output_table_format() {
    print_result(
        &CliResult::Roots(vec!["1v1".to_string(), "2v1".to_string()]),
        OutputFormat::Table,
    );
}

#[test]
fn output_error_to_stderr() {
    print_result(&CliResult::Error("test error".to_string()), OutputFormat::Table);
}

#[test]
fn output_success() {
    print_result(&CliResult::Success, OutputFormat::Table);
}

#[test]
fn find_with_limit() {
    let store = create_test_store();
    let cmd = BlockCommands::Find(FindCommand { query: "".to_string(), limit: 1 });
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Find(matches) if matches.len() == 1));
}

#[test]
fn move_with_unknown_source() {
    let store = create_test_store();
    let root_id = store.roots()[0];
    let cmd = BlockCommands::Tree(TreeCommands::Move(MoveCommand {
        source_id: BlockId("0v0".to_string()),
        target_id: BlockId(format_block_id(root_id)),
        before: false,
        after: false,
        under: false,
    }));
    let (_store, result) = cmd.execute(store, &PathBuf::from("."));
    assert!(matches!(result, CliResult::Error(msg) if msg.contains("Unknown")));
}

#[test]
fn mount_move_command_moves_backing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let source_path = tmp.path().join("source.json");
    let moved_path = tmp.path().join("moved.json");

    std::fs::write(&source_path, serde_json::to_string_pretty(&BlockStore::default()).unwrap())
        .unwrap();

    let store = BlockStore::default();
    let root_id = store.roots()[0];

    let cmd = BlockCommands::Mount(MountCommands::Set(SetMountCommand {
        block_id: BlockId(format_block_id(root_id)),
        path: source_path.clone(),
        format: MountFormatCli(MountFormat::Json),
    }));
    let (store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    let cmd = BlockCommands::Mount(MountCommands::Move(MoveMountCommand {
        block_id: BlockId(format_block_id(root_id)),
        path: moved_path.clone(),
    }));
    let (_store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));
    assert!(moved_path.exists());
    assert!(!source_path.exists());
}

#[test]
fn mount_inline_recursive_reports_inlined_count() {
    let tmp = tempfile::tempdir().unwrap();
    let mount_path = tmp.path().join("mounted.json");
    std::fs::write(&mount_path, serde_json::to_string_pretty(&BlockStore::default()).unwrap())
        .unwrap();

    let store = BlockStore::default();
    let root_id = store.roots()[0];

    let cmd = BlockCommands::Mount(MountCommands::Set(SetMountCommand {
        block_id: BlockId(format_block_id(root_id)),
        path: mount_path,
        format: MountFormatCli(MountFormat::Json),
    }));
    let (store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    let cmd = BlockCommands::Mount(MountCommands::Expand(ExpandMountCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    let cmd = BlockCommands::Mount(MountCommands::InlineRecursive(InlineRecursiveMountCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (_store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::MountInlined(1)));
}

#[test]
fn mount_save_command_persists_expanded_mount_content() {
    let tmp = tempfile::tempdir().unwrap();
    let mount_path = tmp.path().join("mounted.json");
    std::fs::write(&mount_path, serde_json::to_string_pretty(&BlockStore::default()).unwrap())
        .unwrap();

    let store = BlockStore::default();
    let root_id = store.roots()[0];

    let cmd = BlockCommands::Mount(MountCommands::Set(SetMountCommand {
        block_id: BlockId(format_block_id(root_id)),
        path: mount_path.clone(),
        format: MountFormatCli(MountFormat::Json),
    }));
    let (store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    let cmd = BlockCommands::Mount(MountCommands::Expand(ExpandMountCommand {
        block_id: BlockId(format_block_id(root_id)),
    }));
    let (mut store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    let mounted_root = store.children(&root_id)[0];
    store.update_point(&mounted_root, "updated from cli".to_string());

    let cmd = BlockCommands::Mount(MountCommands::Save(SaveMountsCommand {}));
    let (_store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    let written = std::fs::read_to_string(&mount_path).unwrap();
    assert!(written.contains("updated from cli"));
}

#[test]
fn mount_extract_respects_format_override() {
    let tmp = tempfile::tempdir().unwrap();
    let output_path = tmp.path().join("subtree.json");

    let mut store = BlockStore::default();
    let root_id = store.roots()[0];
    store.append_child(&root_id, "child".to_string()).unwrap();

    let cmd = BlockCommands::Mount(MountCommands::Extract(ExtractMountCommand {
        block_id: BlockId(format_block_id(root_id)),
        output: output_path,
        format: Some(MountFormatCli(MountFormat::Markdown)),
    }));
    let (store, result) = cmd.execute(store, tmp.path());
    assert!(matches!(result, CliResult::Success));

    match store.node(&root_id) {
        | Some(BlockNode::Mount { format, .. }) => assert_eq!(*format, MountFormat::Markdown),
        | _ => panic!("expected root to become markdown mount"),
    }
}
