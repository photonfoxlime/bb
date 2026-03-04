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
//!     └─> cmd.execute() -> (BlockStore, CliResult)
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
    results::{
        BatchError, BatchOutput, BatchResult, ExpansionDraftInfo, FriendInfo, Match,
        ReductionDraftInfo, ShowResult,
    },
};
use crate::llm::{BlockContext, ContextFormatter, LineageContext};
use crate::store;

fn indent_lines(s: impl AsRef<str>) -> String {
    s.as_ref().lines().map(|line| format!("  {}", line)).collect::<Vec<_>>().join("\n")
}

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
        | CliResult::Show(show) => {
            print_show(show, output);
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
        | CliResult::Lineage(lineage) => {
            print_lineage(lineage, output);
        }
        | CliResult::Context(ctx) => {
            print_context(ctx, output);
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
        | CliResult::MountInlined(count) => {
            print_mount_inlined(*count, output);
        }
        | CliResult::BlockPanelState(state) => {
            print_block_panel_state(state.as_deref(), output);
        }
        | CliResult::Batch(report) => {
            print_batch_report(report, output);
        }
    }
}

/// Print batch operation report.
fn print_batch_report(report: &BatchResult, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::to_string(report).unwrap_or_default());
        }
        | OutputFormat::Table => {
            println!(
                "Operation: {} (success: {}, failed: {})",
                report.operation, report.successes, report.failures
            );
            for item in &report.outputs {
                print_batch_output_table(item);
            }
            if !report.errors.is_empty() {
                println!("Errors:");
                for err in &report.errors {
                    print_batch_error_table(err);
                }
            }
        }
    }
}

/// Print one batch output item in table mode.
fn print_batch_output_table(item: &BatchOutput) {
    match item {
        | BatchOutput::Id { input, id } => println!("{} -> {}", input, id),
        | BatchOutput::Removed { input, removed } => {
            println!("{} -> removed {}", input, removed.join(", "))
        }
        | BatchOutput::Collapsed { input, collapsed } => {
            println!("{} -> collapsed: {}", input, collapsed)
        }
        | BatchOutput::OptionalId { input, id } => {
            println!("{} -> {}", input, id.as_deref().unwrap_or("(none)"))
        }
        | BatchOutput::Lineage { input, lineage } => {
            let fmt = ContextFormatter::new(lineage.clone()).build();
            println!("{} ->\n{}", input, indent_lines(fmt.lineage_lines()));
        }
        | BatchOutput::Show { input, show } => {
            println!("{} ->\n{}", input, indent_lines(format_show_table(show)));
        }
        | BatchOutput::Context { input, lineage, children, friends } => {
            let fmt = ContextFormatter::new(LineageContext::from_points(lineage.clone()))
                .with_children(children.clone())
                .build();
            println!(
                "{} -> lineage: {}, children: {}, friends: {}",
                input,
                fmt.lineage_points().collect::<Vec<_>>().join(" | "),
                fmt.children().point_strs().collect::<Vec<_>>().join(", "),
                friends
            )
        }
        | BatchOutput::DraftList { input, expansion, reduction, instruction, inquiry } => {
            let count = usize::from(expansion.is_some())
                + usize::from(reduction.is_some())
                + usize::from(instruction.is_some())
                + usize::from(inquiry.is_some());
            println!("{} -> drafts: {}", input, count);
        }
        | BatchOutput::MountInfo { input, path, format, expanded } => {
            println!(
                "{} -> path: {}, format: {}, expanded: {}",
                input,
                path.as_deref().unwrap_or("(none)"),
                format,
                expanded
            )
        }
        | BatchOutput::InlinedCount { input, count } => {
            println!("{} -> inlined mounts: {}", input, count)
        }
        | BatchOutput::Success { input } => println!("{} -> OK", input),
    }
}

/// Print one batch error item in table mode.
fn print_batch_error_table(error: &BatchError) {
    println!("  - {}: {}", error.input, error.error);
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

fn format_show_table(show: &ShowResult) -> String {
    let mut s = format!("ID: {}\nText: {}", show.id, show.text);
    if !show.children.is_empty() {
        s.push_str("\n\nChildren\n");
        for (i, c) in show.children.iter().enumerate() {
            s.push_str(&format!("[{}] {}\n", i, c));
        }
    }
    s
}

/// Print block details (ID, text, children).
fn print_show(show: &ShowResult, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::to_string(show).unwrap_or_default());
        }
        | OutputFormat::Table => {
            println!("{}", format_show_table(show));
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
fn print_lineage(lineage: &LineageContext, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::to_string(lineage).unwrap_or_default());
        }
        | OutputFormat::Table => {
            let fmt = ContextFormatter::new(lineage.clone()).build();
            print!("{}", fmt.lineage_lines());
        }
    }
}

/// Print block context for LLM requests.
fn print_context(ctx: &BlockContext, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::to_string(ctx).unwrap_or_default());
        }
        | OutputFormat::Table => {
            let fmt = ContextFormatter::from_block_context(ctx);
            println!("{}", fmt.format_for_display());
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
                match &r.reduction {
                    Some(text) => println!("  Reduction: {}", text),
                    None => println!("  Reduction: (rejected)"),
                }
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
                for (i, f) in friends.iter().enumerate() {
                    println!("[{}] {}", i, f.id);
                    if let Some(p) = &f.perspective {
                        println!("  perspective: {}", p);
                    }
                    if f.telescope_lineage {
                        println!("  telescope_lineage: yes");
                    }
                    if f.telescope_children {
                        println!("  telescope_children: yes");
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

/// Print recursive mount inline count.
fn print_mount_inlined(count: usize, output: OutputFormat) {
    match output {
        | OutputFormat::Json => {
            println!("{}", serde_json::json!({ "inlined_mounts": count }));
        }
        | OutputFormat::Table => {
            println!("Inlined mounts: {}", count);
        }
    }
}

/// Print panel sidebar state.
fn print_block_panel_state(state: Option<&str>, output: OutputFormat) {
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
