//! CLI output formatting utilities.
//!
//! This module provides the complete output formatting logic for all CLI results.
//! It transforms `CliResult` values into user-visible output in two formats:
//!
//! - JSON: Machine-readable, suitable for scripting and programmatic consumption
//! - Table: Human-readable, with structured formatting and labels
//!
//! # Architecture
//!
//! The module exports a single public function `print_result()` that dispatches
//! to private helper functions based on the `CliResult` variant. This design:
//!
//! - Centralizes all output formatting logic in one place
//! - Makes it easy to add new output formats (e.g., YAML, CSV)
//! - Keeps the execution logic in `execute.rs` clean and focused
//!
//! # Output Flow
//!
//! ```text
//! lib.rs::main()
//!     └─> block_commands.execute() -> (BlockStore, CliResult)
//!         └─> print_result(&CliResult, OutputFormat)
//!             ├─> Json: serde_json::json!() with structured objects
//!             └─> Table: println!() with formatted text
//! ```
//!
//! # Design Principles
//!
//! - Consistency: All variants use the same JSON/table pattern
//! - Error handling: Errors go to stderr, everything else to stdout
//! - Graceful degradation: Uses `unwrap_or_default()` for serialization failures
//! - No side effects: Pure formatting logic, all I/O via println!/eprintln!

use crate::cli::{
    CliResult, OutputFormat,
    results::{ExpansionDraftInfo, FriendInfo, Match, ReductionDraftInfo},
};
use crate::store;

/// Print a `CliResult` to stdout in the specified format.
///
/// This is the single entry point for all CLI output. It pattern matches on the
/// result variant and delegates to the appropriate helper function.
///
/// # Arguments
///
/// - `result`: The CLI result to print
/// - `output`: The desired output format (Json or Table)
///
/// # Side Effects
///
/// - Prints to stdout for all variants except `CliResult::Error`
/// - Prints to stderr for `CliResult::Error`
/// - May serialize to JSON using `serde_json`
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

/// Print root block IDs.
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

/// Print block details (ID, text, children).
fn print_show(id: store::BlockId, text: &str, children: &[String], output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "id": format!("{}", id),
                    "text": text,
                    "children": children
                })
            );
        }
        | OutputFormat::Table => {
            println!("ID:       {}", id);
            println!("Text:     {}", text);
            println!("Children: {}", children.join(", "));
        }
    }
}

/// Print search results.
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

/// Print lineage (ancestor chain) points.
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

/// Print block context for LLM requests.
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
            println!("Children: {}", children.join(", "));
            println!("Friends: {}", friends);
        }
    }
}

/// Print draft listing results.
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
                    println!("  Children: {}", e.children.join(", "));
                }
            }
            if let Some(r) = reduction {
                println!("Reduction draft:");
                println!("  Reduction: {}", r.reduction);
                if !r.redundant_children.is_empty() {
                    println!("  Redundant children: {}", r.redundant_children.join(", "));
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

/// Print friend list (cross-references).
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

/// Print mount information.
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

/// Print panel sidebar state.
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

/// Print a single block ID.
fn print_block_id(id: store::BlockId, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "id": format!("{}", id) }));
        }
        | OutputFormat::Table => {
            println!("{}", id);
        }
    }
}

/// Print an optional block ID (navigation result).
fn print_optional_block_id(id: Option<&store::BlockId>, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "id": id.map(|i| format!("{}", i)) }));
        }
        | OutputFormat::Table => {
            if let Some(id) = id {
                println!("{}", id);
            }
        }
    }
}

/// Print removed block IDs.
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

/// Print collapsed state boolean.
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
