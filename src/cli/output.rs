//! CLI output formatting utilities.
//!
//! This module provides functions to format and print `CliResult` values
//! in different output formats (JSON or table-style for human readability).

use crate::cli::{
    CliResult, OutputFormat,
    results::{ExpansionDraftInfo, FriendInfo, Match, ReductionDraftInfo},
};
use crate::store;

/// Print a `CliResult` to stdout in the specified format.
pub fn print_result(result: &CliResult, output: OutputFormat) {
    match result {
        | CliResult::Success => {
            println!("OK");
        }
        | CliResult::Error(msg) => {
            eprintln!("Error: {}", msg);
        }
        | CliResult::Roots(ids) => {
            print_roots(ids, output);
        }
        | CliResult::Show { id, text, children } => {
            print_show(*id, text, children, output);
        }
        | CliResult::Find(matches) => {
            print_find(matches, output);
        }
        | CliResult::BlockId(id) => {
            print_block_id(*id, output);
        }
        | CliResult::OptionalBlockId(id) => {
            print_optional_block_id(id.as_ref(), output);
        }
        | CliResult::Removed(ids) => {
            print_removed(ids, output);
        }
        | CliResult::Collapsed(collapsed) => {
            print_collapsed(*collapsed, output);
        }
        | CliResult::Lineage(points) => {
            print_lineage(points, output);
        }
        | CliResult::Context { lineage, children, friends } => {
            print_context(lineage, children, *friends, output);
        }
        | CliResult::DraftList { expansion, reduction, instruction, inquiry } => {
            print_draft_list(expansion, reduction, instruction, inquiry, output);
        }
        | CliResult::FriendList(friends) => {
            print_friend_list(friends, output);
        }
        | CliResult::MountInfo { path, format, expanded } => {
            print_mount_info(path.as_deref(), format, *expanded, output);
        }
        | CliResult::PanelState(state) => {
            print_panel_state(state.as_deref(), output);
        }
    }
}

fn print_roots(ids: &[String], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "roots": ids }));
        }
        | OutputFormat::Table => {
            for id in ids {
                println!("{}", id);
            }
        }
    }
}

fn print_show(id: store::BlockId, text: &str, children: &[String], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "id": format!("{:?}", id),
                    "text": text,
                    "children": children
                })
            );
        }
        | OutputFormat::Table => {
            println!("ID:       {:?}", id);
            println!("Text:     {}", text);
            println!("Children: {:?}", children);
        }
    }
}

fn print_find(matches: &[Match], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::to_string(matches).unwrap_or_default());
        }
        | OutputFormat::Table => {
            for m in matches {
                println!("{}: {}", m.id, m.text);
            }
        }
    }
}

fn print_lineage(points: &[String], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "lineage": points }));
        }
        | OutputFormat::Table => {
            for (i, p) in points.iter().enumerate() {
                println!("{}. {}", i + 1, p);
            }
        }
    }
}

fn print_context(lineage: &[String], children: &[String], friends: usize, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "lineage": lineage,
                    "children": children,
                    "friends": friends
                })
            );
        }
        | OutputFormat::Table => {
            println!("Lineage:");
            for p in lineage {
                println!("  - {}", p);
            }
            println!("Children: {:?}", children);
            println!("Friends: {}", friends);
        }
    }
}

fn print_draft_list(
    expansion: &Option<ExpansionDraftInfo>, reduction: &Option<ReductionDraftInfo>,
    instruction: &Option<String>, inquiry: &Option<String>, output: OutputFormat,
) {
    match output {
        | OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "expansion": expansion,
                    "reduction": reduction,
                    "instruction": instruction,
                    "inquiry": inquiry
                })
            );
        }
        | OutputFormat::Table => {
            if let Some(e) = expansion {
                println!("Expansion draft:");
                if let Some(r) = &e.rewrite {
                    println!("  Rewrite: {}", r);
                }
                if !e.children.is_empty() {
                    println!("  Children: {:?}", e.children);
                }
            }
            if let Some(r) = reduction {
                println!("Reduction draft:");
                println!("  Reduction: {}", r.reduction);
                if !r.redundant_children.is_empty() {
                    println!("  Redundant children: {:?}", r.redundant_children);
                }
            }
            if let Some(i) = instruction {
                println!("Instruction: {}", i);
            }
            if let Some(q) = inquiry {
                println!("Inquiry: {}", q);
            }
            if expansion.is_none()
                && reduction.is_none()
                && instruction.is_none()
                && inquiry.is_none()
            {
                println!("(no drafts)");
            }
        }
    }
}

fn print_friend_list(friends: &[FriendInfo], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::to_string(friends).unwrap_or_default());
        }
        | OutputFormat::Table => {
            if friends.is_empty() {
                println!("(no friends)");
            } else {
                for f in friends {
                    println!("{}", f.id);
                    if let Some(p) = &f.perspective {
                        println!("  Perspective: {}", p);
                    }
                    if f.telescope_lineage {
                        println!("  Telescope lineage: yes");
                    }
                    if f.telescope_children {
                        println!("  Telescope children: yes");
                    }
                }
            }
        }
    }
}

fn print_mount_info(path: Option<&str>, format: &str, expanded: bool, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "path": path,
                    "format": format,
                    "expanded": expanded
                })
            );
        }
        | OutputFormat::Table => {
            println!("Path: {}", path.unwrap_or("(none)"));
            println!("Format: {}", format);
            println!("Expanded: {}", expanded);
        }
    }
}

fn print_panel_state(state: Option<&str>, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "panel": state }));
        }
        | OutputFormat::Table => match state {
            | Some(s) => println!("{}", s),
            | None => println!("(not set)"),
        },
    }
}

fn print_block_id(id: store::BlockId, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "id": format!("{:?}", id) }));
        }
        | OutputFormat::Table => {
            println!("{:?}", id);
        }
    }
}

fn print_optional_block_id(id: Option<&store::BlockId>, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "id": id.map(|i| format!("{:?}", i)) }));
        }
        | OutputFormat::Table => {
            if let Some(id) = id {
                println!("{:?}", id);
            }
        }
    }
}

fn print_removed(ids: &[String], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "removed": ids }));
        }
        | OutputFormat::Table => {
            for id in ids {
                println!("Removed: {}", id);
            }
        }
    }
}

fn print_collapsed(collapsed: bool, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "collapsed": collapsed }));
        }
        | OutputFormat::Table => {
            println!("Collapsed: {}", collapsed);
        }
    }
}
