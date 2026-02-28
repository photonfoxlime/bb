//! BlockCommands execution implementation.
//!
//! This module implements the execution logic for all CLI commands defined
//! in the `BlockCommands` enum. It serves as the bridge between parsed CLI
//! arguments and the underlying `BlockStore` operations.
//!
//! # Architecture
//!
//! The `execute()` method follows a consistent pattern:
//!
//! 1. Resolve BlockId: Convert CLI string IDs to internal `store::BlockId`
//! 2. Validate: Check that referenced blocks exist
//! 3. Execute: Call the appropriate `BlockStore` method
//! 4. Return: Package result as `CliResult` for formatting
//!
//! Batch-capable commands also support comma-separated target IDs in a single
//! ID argument. In batch mode, execution is continue-on-error: all targets are
//! attempted and per-item failures are collected in `CliResult::Batch`.
//!
//! # Error Handling
//!
//! All errors are returned as `CliResult::Error` with descriptive messages.
//! The execution never panics—unknown block IDs, invalid operations, and
//! store failures all result in error variants.
//!
//! # Command Categories
//!
//! - Query (`roots`, `show`, `find`): Read-only, no store modification
//! - Tree (`add-child`, `move`, `delete`, etc.): Structural edits
//! - Nav (`next`, `prev`, `lineage`): DFS navigation helpers
//! - Draft (`expand`, `reduce`, `instruction`, `inquiry`): LLM interaction
//! - Fold (`toggle`, `status`): Visibility state management
//! - Friend (`add`, `remove`, `list`): Cross-reference links
//! - Mount (`set`, `expand`, `collapse`, `extract`): External file integration
//! - Panel (`set`, `get`, `clear`): Sidebar UI state
//! - Context: LLM context preparation

use super::BlockId;
use super::results::{
    BatchError, BatchOutput, BatchResult, CliResult, ExpansionDraftInfo, FriendInfo, Match,
    ReductionDraftInfo,
};
use super::{
    BlockCommands, draft::DraftCommands, fold::FoldCommands, friend::FriendCommands,
    mount::MountCommands, nav::NavCommands, panel::PanelCommands, query::RootCommand,
    tree::TreeCommands,
};
use crate::store as store_module;
use crate::store::{BlockStore, Direction};

impl BlockCommands {
    // Execute a block command with the given store.
    ///
    // This is the main entry point for CLI command execution. It pattern matches
    // on the command variant, performs necessary validation, calls the appropriate
    // `BlockStore` method, and returns both the (possibly modified) store and
    // a result for output formatting.
    ///
    // # Arguments
    ///
    // - `self`: The command to execute (consumed)
    // - `store`: The block store to operate on (passed by value for ownership transfer)
    // - `base_dir`: Base directory for resolving relative mount paths
    ///
    // # Returns
    ///
    // A tuple of `(BlockStore, CliResult)`:
    // - The store is always returned (modified or unchanged) for potential saving
    // - The `CliResult` indicates success, failure, or query results
    ///
    // # Design Notes
    ///
    // - Block ID resolution is case-insensitive for format
    // - All operations validate block existence before proceeding
    // - Mount operations use `base_dir` to resolve relative paths
    pub fn execute(
        self, mut store: BlockStore, base_dir: &std::path::Path,
    ) -> (BlockStore, CliResult) {
        match self {
            // ========================================================================
            // Query Commands
            // ========================================================================
            // Read-only operations that inspect the store without modification.
            // List all root block IDs.
            | BlockCommands::Roots(RootCommand {}) => {
                let roots: Vec<String> = store.roots().iter().map(|id| format!("{}", id)).collect();
                (store, CliResult::Roots(roots))
            }
            // Show details of a specific block.
            | BlockCommands::Show(cmd) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(id) => {
                            let text = store.point(&id).unwrap_or_default();
                            let children: Vec<String> =
                                store.children(&id).iter().map(|c| format!("{}", c)).collect();
                            (store, CliResult::Show { id, text, children })
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(id) => {
                                let text = store.point(&id).unwrap_or_default();
                                let children: Vec<String> =
                                    store.children(&id).iter().map(|c| format!("{}", c)).collect();
                                outputs.push(BatchOutput::Show {
                                    input,
                                    id: format!("{}", id),
                                    text,
                                    children,
                                });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("query.show", outputs, errors)),
                    )
                }
            }
            // Search blocks by text content using store-level query matching.
            | BlockCommands::Find(cmd) => {
                let matches: Vec<Match> = store
                    .find_block_point(&cmd.query)
                    .into_iter()
                    .filter_map(|id| {
                        let text = store.point(&id)?;
                        Some(Match { id: format!("{}", id), text })
                    })
                    .take(cmd.limit)
                    .collect();
                (store, CliResult::Find(matches))
            }
            // Edit the text content of a block.
            | BlockCommands::Point(cmd) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            store.update_point(&block_id, cmd.text);
                            (store, CliResult::Success)
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                store.update_point(&block_id, cmd.text.clone());
                                outputs.push(BatchOutput::Success { input });
                            }
                        }
                    }
                    (store, CliResult::Batch(Self::make_batch_result("point", outputs, errors)))
                }
            }
            // ========================================================================
            // Tree Commands
            // ========================================================================
            // Structural editing operations for modifying the block hierarchy.
            // Add a child block to a parent.
            | BlockCommands::Tree(TreeCommands::AddChild(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.parent_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown parent block ID".to_string())),
                        | Some(parent_id) => {
                            let new_id = store.append_child(&parent_id, cmd.text.clone());
                            match new_id {
                                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                                | None => (
                                    store,
                                    CliResult::Error(
                                        "Failed to add child (parent may be a mount)".to_string(),
                                    ),
                                ),
                            }
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors.push(BatchError {
                                input,
                                error: "Unknown parent block ID".to_string(),
                            }),
                            | Some(parent_id) => {
                                match store.append_child(&parent_id, cmd.text.clone()) {
                                    | Some(new_id) => outputs
                                        .push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                                    | None => errors.push(BatchError {
                                        input,
                                        error: "Failed to add child (parent may be a mount)"
                                            .to_string(),
                                    }),
                                }
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result(
                            "tree.add-child",
                            outputs,
                            errors,
                        )),
                    )
                }
            }
            // Add a sibling block after the target.
            | BlockCommands::Tree(TreeCommands::AddSibling(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let new_id = store.append_sibling(&block_id, cmd.text.clone());
                            match new_id {
                                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                                | None => {
                                    (store, CliResult::Error("Failed to add sibling".to_string()))
                                }
                            }
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                match store.append_sibling(&block_id, cmd.text.clone()) {
                                    | Some(new_id) => outputs
                                        .push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                                    | None => errors.push(BatchError {
                                        input,
                                        error: "Failed to add sibling".to_string(),
                                    }),
                                }
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result(
                            "tree.add-sibling",
                            outputs,
                            errors,
                        )),
                    )
                }
            }
            // Wrap a block in a new parent.
            | BlockCommands::Tree(TreeCommands::Wrap(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let new_id = store.insert_parent(&block_id, cmd.text.clone());
                            match new_id {
                                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                                | None => {
                                    (store, CliResult::Error("Failed to wrap block".to_string()))
                                }
                            }
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                match store.insert_parent(&block_id, cmd.text.clone()) {
                                    | Some(new_id) => outputs
                                        .push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                                    | None => errors.push(BatchError {
                                        input,
                                        error: "Failed to wrap block".to_string(),
                                    }),
                                }
                            }
                        }
                    }
                    (store, CliResult::Batch(Self::make_batch_result("tree.wrap", outputs, errors)))
                }
            }
            // Duplicate a block and its entire subtree.
            | BlockCommands::Tree(TreeCommands::Duplicate(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let new_id = store.duplicate_subtree_after(&block_id);
                            match new_id {
                                | Some(new_id) => (store, CliResult::BlockId(new_id)),
                                | None => {
                                    (store, CliResult::Error("Failed to duplicate".to_string()))
                                }
                            }
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => match store.duplicate_subtree_after(&block_id) {
                                | Some(new_id) => outputs
                                    .push(BatchOutput::Id { input, id: format!("{}", new_id) }),
                                | None => errors.push(BatchError {
                                    input,
                                    error: "Failed to duplicate".to_string(),
                                }),
                            },
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result(
                            "tree.duplicate",
                            outputs,
                            errors,
                        )),
                    )
                }
            }
            // Delete a block and its entire subtree.
            | BlockCommands::Tree(TreeCommands::Delete(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let removed = store.remove_block_subtree(&block_id);
                            match removed {
                                | Some(ids) => {
                                    let ids_str: Vec<String> =
                                        ids.iter().map(|i| format!("{}", i)).collect();
                                    (store, CliResult::Removed(ids_str))
                                }
                                | None => (store, CliResult::Error("Failed to delete".to_string())),
                            }
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => match store.remove_block_subtree(&block_id) {
                                | Some(ids) => outputs.push(BatchOutput::Removed {
                                    input,
                                    removed: ids.iter().map(|id| format!("{}", id)).collect(),
                                }),
                                | None => errors.push(BatchError {
                                    input,
                                    error: "Failed to delete".to_string(),
                                }),
                            },
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("tree.delete", outputs, errors)),
                    )
                }
            }
            // Move a block relative to a target.
            | BlockCommands::Tree(TreeCommands::Move(cmd)) => {
                let pairs = match Self::expand_cli_pairs(&cmd.source_id, &cmd.target_id) {
                    | Ok(pairs) => pairs,
                    | Err(msg) => return (store, CliResult::Error(msg)),
                };

                let dir = if cmd.before {
                    Direction::Before
                } else if cmd.after {
                    Direction::After
                } else {
                    Direction::Under
                };

                if pairs.len() == 1 {
                    let source = Self::resolve_block_id(&store, &pairs[0].0);
                    let target = Self::resolve_block_id(&store, &pairs[0].1);
                    match (source, target) {
                        | (Some(src), Some(tgt)) => {
                            let result = store.move_block(&src, &tgt, dir);
                            match result {
                                | Some(()) => (store, CliResult::Success),
                                | None => (
                                    store,
                                    CliResult::Error("Move failed (check constraints)".to_string()),
                                ),
                            }
                        }
                        | _ => (
                            store,
                            CliResult::Error("Unknown source or target block ID".to_string()),
                        ),
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for (source_cli, target_cli) in pairs {
                        let input = format!("{} -> {}", source_cli.0, target_cli.0);
                        let source = Self::resolve_block_id(&store, &source_cli);
                        let target = Self::resolve_block_id(&store, &target_cli);
                        match (source, target) {
                            | (Some(src), Some(tgt)) => match store.move_block(&src, &tgt, dir) {
                                | Some(()) => outputs.push(BatchOutput::Success { input }),
                                | None => errors.push(BatchError {
                                    input,
                                    error: "Move failed (check constraints)".to_string(),
                                }),
                            },
                            | _ => errors.push(BatchError {
                                input,
                                error: "Unknown source or target block ID".to_string(),
                            }),
                        }
                    }
                    (store, CliResult::Batch(Self::make_batch_result("tree.move", outputs, errors)))
                }
            }
            // ========================================================================
            // Navigation Commands
            // ========================================================================
            // DFS-based navigation helpers for traversing the block tree.
            // Get the next visible block in DFS order.
            | BlockCommands::Nav(NavCommands::Next(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let next = store.next_visible_in_dfs(&block_id);
                            (store, CliResult::OptionalBlockId(next))
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let next = store.next_visible_in_dfs(&block_id);
                                outputs.push(BatchOutput::OptionalId {
                                    input,
                                    id: next.map(|id| format!("{}", id)),
                                });
                            }
                        }
                    }
                    (store, CliResult::Batch(Self::make_batch_result("nav.next", outputs, errors)))
                }
            }
            // Get the previous visible block in DFS order.
            | BlockCommands::Nav(NavCommands::Prev(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let prev = store.prev_visible_in_dfs(&block_id);
                            (store, CliResult::OptionalBlockId(prev))
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let prev = store.prev_visible_in_dfs(&block_id);
                                outputs.push(BatchOutput::OptionalId {
                                    input,
                                    id: prev.map(|id| format!("{}", id)),
                                });
                            }
                        }
                    }
                    (store, CliResult::Batch(Self::make_batch_result("nav.prev", outputs, errors)))
                }
            }
            // Get the lineage (ancestor chain) of a block.
            | BlockCommands::Nav(NavCommands::Lineage(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let lineage = store.lineage_points_for_id(&block_id);
                            let points: Vec<String> = lineage.points().map(String::from).collect();
                            (store, CliResult::Lineage(points))
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let lineage = store.lineage_points_for_id(&block_id);
                                outputs.push(BatchOutput::Lineage {
                                    input,
                                    points: lineage.points().map(String::from).collect(),
                                });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("nav.lineage", outputs, errors)),
                    )
                }
            }
            // Jump to the next query match in DFS order.
            | BlockCommands::Nav(NavCommands::FindNext(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let next = Self::find_relative_query_match(
                            &store,
                            &block_id,
                            &cmd.query,
                            true,
                            !cmd.no_wrap,
                        );
                        (store, CliResult::OptionalBlockId(next))
                    }
                }
            }
            // Jump to the previous query match in DFS order.
            | BlockCommands::Nav(NavCommands::FindPrev(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let prev = Self::find_relative_query_match(
                            &store,
                            &block_id,
                            &cmd.query,
                            false,
                            !cmd.no_wrap,
                        );
                        (store, CliResult::OptionalBlockId(prev))
                    }
                }
            }
            // ========================================================================
            // Draft Commands
            // ========================================================================
            // LLM interaction drafts for managing AI-assisted editing.
            // Create an expansion draft for block refinement.
            | BlockCommands::Draft(DraftCommands::Expand(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let draft = store_module::ExpansionDraftRecord {
                            rewrite: cmd.rewrite,
                            children: cmd.children,
                        };
                        store.insert_expansion_draft(block_id, draft);
                        (store, CliResult::Success)
                    }
                }
            }
            // Create a reduction draft for summarization.
            | BlockCommands::Draft(DraftCommands::Reduce(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let redundant: Vec<_> = cmd
                            .redundant_children
                            .iter()
                            .filter_map(|c| Self::resolve_block_id(&store, c))
                            .collect();
                        let draft = store_module::ReductionDraftRecord {
                            reduction: cmd.reduction,
                            redundant_children: redundant,
                        };
                        store.insert_reduction_draft(block_id, draft);
                        (store, CliResult::Success)
                    }
                }
            }
            // Set an instruction draft for LLM guidance.
            | BlockCommands::Draft(DraftCommands::Instruction(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            store.set_instruction_draft(block_id, cmd.text);
                            (store, CliResult::Success)
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                store.set_instruction_draft(block_id, cmd.text.clone());
                                outputs.push(BatchOutput::Success { input });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result(
                            "draft.instruction",
                            outputs,
                            errors,
                        )),
                    )
                }
            }
            // Set an inquiry draft (LLM response).
            | BlockCommands::Draft(DraftCommands::Inquiry(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            store.set_inquiry_draft(block_id, cmd.response);
                            (store, CliResult::Success)
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                store.set_inquiry_draft(block_id, cmd.response.clone());
                                outputs.push(BatchOutput::Success { input });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("draft.inquiry", outputs, errors)),
                    )
                }
            }
            // List all drafts for a block.
            | BlockCommands::Draft(DraftCommands::List(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let expansion =
                                store.expansion_draft(&block_id).map(|d| ExpansionDraftInfo {
                                    rewrite: d.rewrite.clone(),
                                    children: d.children.clone(),
                                });
                            let reduction =
                                store.reduction_draft(&block_id).map(|d| ReductionDraftInfo {
                                    reduction: d.reduction.clone(),
                                    redundant_children: d
                                        .redundant_children
                                        .iter()
                                        .map(|id| format!("{}", id))
                                        .collect(),
                                });
                            let instruction =
                                store.instruction_draft(&block_id).map(|d| d.instruction.clone());
                            let inquiry =
                                store.inquiry_draft(&block_id).map(|d| d.response.clone());
                            (
                                store,
                                CliResult::DraftList { expansion, reduction, instruction, inquiry },
                            )
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let expansion =
                                    store.expansion_draft(&block_id).map(|d| ExpansionDraftInfo {
                                        rewrite: d.rewrite.clone(),
                                        children: d.children.clone(),
                                    });
                                let reduction =
                                    store.reduction_draft(&block_id).map(|d| ReductionDraftInfo {
                                        reduction: d.reduction.clone(),
                                        redundant_children: d
                                            .redundant_children
                                            .iter()
                                            .map(|id| format!("{}", id))
                                            .collect(),
                                    });
                                let instruction = store
                                    .instruction_draft(&block_id)
                                    .map(|d| d.instruction.clone());
                                let inquiry =
                                    store.inquiry_draft(&block_id).map(|d| d.response.clone());
                                outputs.push(BatchOutput::DraftList {
                                    input,
                                    expansion,
                                    reduction,
                                    instruction,
                                    inquiry,
                                });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("draft.list", outputs, errors)),
                    )
                }
            }
            // Clear drafts from a block.
            | BlockCommands::Draft(DraftCommands::Clear(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            if cmd.all || cmd.expand {
                                store.remove_expansion_draft(&block_id);
                            }
                            if cmd.all || cmd.reduce {
                                store.remove_reduction_draft(&block_id);
                            }
                            if cmd.all || cmd.instruction {
                                store.remove_instruction_draft(&block_id);
                            }
                            if cmd.all || cmd.inquiry {
                                store.remove_inquiry_draft(&block_id);
                            }
                            (store, CliResult::Success)
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                if cmd.all || cmd.expand {
                                    store.remove_expansion_draft(&block_id);
                                }
                                if cmd.all || cmd.reduce {
                                    store.remove_reduction_draft(&block_id);
                                }
                                if cmd.all || cmd.instruction {
                                    store.remove_instruction_draft(&block_id);
                                }
                                if cmd.all || cmd.inquiry {
                                    store.remove_inquiry_draft(&block_id);
                                }
                                outputs.push(BatchOutput::Success { input });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("draft.clear", outputs, errors)),
                    )
                }
            }
            // ========================================================================
            // Fold Commands
            // ========================================================================
            // Visibility state management for collapsing/expanding blocks.
            // Toggle collapsed state of a block.
            | BlockCommands::Fold(FoldCommands::Toggle(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let collapsed = store.toggle_collapsed(&block_id);
                            (store, CliResult::Collapsed(collapsed))
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let collapsed = store.toggle_collapsed(&block_id);
                                outputs.push(BatchOutput::Collapsed { input, collapsed });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("fold.toggle", outputs, errors)),
                    )
                }
            }
            // Get collapsed state of a block.
            | BlockCommands::Fold(FoldCommands::Status(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let collapsed = store.is_collapsed(&block_id);
                            (store, CliResult::Collapsed(collapsed))
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let collapsed = store.is_collapsed(&block_id);
                                outputs.push(BatchOutput::Collapsed { input, collapsed });
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("fold.status", outputs, errors)),
                    )
                }
            }
            // ========================================================================
            // Friend Commands
            // ========================================================================
            // Cross-reference link management between blocks.
            // Add a friend (cross-reference) link.
            | BlockCommands::Friend(FriendCommands::Add(cmd)) => {
                let pairs = match Self::expand_cli_pairs(&cmd.target_id, &cmd.friend_id) {
                    | Ok(pairs) => pairs,
                    | Err(msg) => return (store, CliResult::Error(msg)),
                };

                if pairs.len() == 1 {
                    let target = Self::resolve_block_id(&store, &pairs[0].0);
                    let friend = Self::resolve_block_id(&store, &pairs[0].1);
                    match (target, friend) {
                        | (Some(tid), Some(fid)) => {
                            let mut friends = store.friend_blocks_for(&tid).to_vec();
                            friends.push(store_module::FriendBlock {
                                block_id: fid,
                                perspective: cmd.perspective,
                                parent_lineage_telescope: cmd.telescope_lineage,
                                children_telescope: cmd.telescope_children,
                            });
                            store.set_friend_blocks_for(&tid, friends);
                            (store, CliResult::Success)
                        }
                        | _ => (store, CliResult::Error("Unknown block ID".to_string())),
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for (target_cli, friend_cli) in pairs {
                        let input = format!("{} -> {}", target_cli.0, friend_cli.0);
                        let target = Self::resolve_block_id(&store, &target_cli);
                        let friend = Self::resolve_block_id(&store, &friend_cli);
                        match (target, friend) {
                            | (Some(tid), Some(fid)) => {
                                let mut friends = store.friend_blocks_for(&tid).to_vec();
                                friends.push(store_module::FriendBlock {
                                    block_id: fid,
                                    perspective: cmd.perspective.clone(),
                                    parent_lineage_telescope: cmd.telescope_lineage,
                                    children_telescope: cmd.telescope_children,
                                });
                                store.set_friend_blocks_for(&tid, friends);
                                outputs.push(BatchOutput::Success { input });
                            }
                            | _ => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("friend.add", outputs, errors)),
                    )
                }
            }
            // Remove a friend (cross-reference) link.
            | BlockCommands::Friend(FriendCommands::Remove(cmd)) => {
                let pairs = match Self::expand_cli_pairs(&cmd.target_id, &cmd.friend_id) {
                    | Ok(pairs) => pairs,
                    | Err(msg) => return (store, CliResult::Error(msg)),
                };

                if pairs.len() == 1 {
                    let target = Self::resolve_block_id(&store, &pairs[0].0);
                    let friend = Self::resolve_block_id(&store, &pairs[0].1);
                    match (target, friend) {
                        | (Some(tid), Some(fid)) => {
                            let mut friends = store.friend_blocks_for(&tid).to_vec();
                            friends.retain(|f| f.block_id != fid);
                            store.set_friend_blocks_for(&tid, friends);
                            (store, CliResult::Success)
                        }
                        | _ => (store, CliResult::Error("Unknown block ID".to_string())),
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for (target_cli, friend_cli) in pairs {
                        let input = format!("{} -> {}", target_cli.0, friend_cli.0);
                        let target = Self::resolve_block_id(&store, &target_cli);
                        let friend = Self::resolve_block_id(&store, &friend_cli);
                        match (target, friend) {
                            | (Some(tid), Some(fid)) => {
                                let mut friends = store.friend_blocks_for(&tid).to_vec();
                                friends.retain(|f| f.block_id != fid);
                                store.set_friend_blocks_for(&tid, friends);
                                outputs.push(BatchOutput::Success { input });
                            }
                            | _ => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("friend.remove", outputs, errors)),
                    )
                }
            }
            // List all friends (cross-references) of a block.
            | BlockCommands::Friend(FriendCommands::List(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                match target {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(tid) => {
                        let friends: Vec<FriendInfo> = store
                            .friend_blocks_for(&tid)
                            .iter()
                            .map(|f| FriendInfo {
                                id: format!("{}", f.block_id),
                                perspective: f.perspective.clone(),
                                telescope_lineage: f.parent_lineage_telescope,
                                telescope_children: f.children_telescope,
                            })
                            .collect();
                        (store, CliResult::FriendList(friends))
                    }
                }
            }
            // ========================================================================
            // Mount Commands
            // ========================================================================
            // External file integration for importing/exporting block trees.
            // Set mount path and format for a block.
            | BlockCommands::Mount(MountCommands::Set(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                let format: store_module::MountFormat = cmd.format.into();
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let result =
                                store.set_mount_path_with_format(&block_id, cmd.path, format);
                            match result {
                                | Some(()) => (store, CliResult::Success),
                                | None => (
                                    store,
                                    CliResult::Error(
                                        "Failed to set mount path (block may have children)"
                                            .to_string(),
                                    ),
                                ),
                            }
                        }
                    }
                } else {
                    if !Self::is_directory_like(&cmd.path) {
                        return (
                            store,
                            CliResult::Error(
                                "Batch mount set requires a directory-like PATH".to_string(),
                            ),
                        );
                    }

                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    let ext = Self::mount_format_extension(format);
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let path = Self::batch_child_file_path(&cmd.path, &input, ext);
                                match store.set_mount_path_with_format(&block_id, path, format) {
                                    | Some(()) => outputs.push(BatchOutput::Success { input }),
                                    | None => errors.push(BatchError {
                                        input,
                                        error: "Failed to set mount path (block may have children)"
                                            .to_string(),
                                    }),
                                }
                            }
                        }
                    }

                    (store, CliResult::Batch(Self::make_batch_result("mount.set", outputs, errors)))
                }
            }
            // Expand a mount by loading external file contents.
            | BlockCommands::Mount(MountCommands::Expand(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => match store.expand_mount(&block_id, base_dir) {
                            | Ok(_) => (store, CliResult::Success),
                            | Err(e) => (store, CliResult::Error(format!("Expand failed: {}", e))),
                        },
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => match store.expand_mount(&block_id, base_dir) {
                                | Ok(_) => outputs.push(BatchOutput::Success { input }),
                                | Err(e) => errors.push(BatchError {
                                    input,
                                    error: format!("Expand failed: {}", e),
                                }),
                            },
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("mount.expand", outputs, errors)),
                    )
                }
            }
            // Collapse a mount, removing loaded children.
            | BlockCommands::Mount(MountCommands::Collapse(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => match store.collapse_mount(&block_id) {
                            | Some(()) => (store, CliResult::Success),
                            | None => (
                                store,
                                CliResult::Error("Block is not an expanded mount".to_string()),
                            ),
                        },
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => match store.collapse_mount(&block_id) {
                                | Some(()) => outputs.push(BatchOutput::Success { input }),
                                | None => errors.push(BatchError {
                                    input,
                                    error: "Block is not an expanded mount".to_string(),
                                }),
                            },
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result(
                            "mount.collapse",
                            outputs,
                            errors,
                        )),
                    )
                }
            }
            // Move a mount file and update mount metadata.
            | BlockCommands::Mount(MountCommands::Move(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            match store.move_mount_file(&block_id, &cmd.path, base_dir) {
                                | Ok(()) => (store, CliResult::Success),
                                | Err(e) => {
                                    (store, CliResult::Error(format!("Move failed: {}", e)))
                                }
                            }
                        }
                    }
                } else {
                    if !Self::is_directory_like(&cmd.path) {
                        return (
                            store,
                            CliResult::Error(
                                "Batch mount move requires a directory-like PATH".to_string(),
                            ),
                        );
                    }

                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let ext = Self::mount_format_extension(
                                    Self::mount_format_for_block(&store, &block_id)
                                        .unwrap_or(store_module::MountFormat::Json),
                                );
                                let path = Self::batch_child_file_path(&cmd.path, &input, ext);
                                match store.move_mount_file(&block_id, &path, base_dir) {
                                    | Ok(()) => outputs.push(BatchOutput::Success { input }),
                                    | Err(e) => errors.push(BatchError {
                                        input,
                                        error: format!("Move failed: {}", e),
                                    }),
                                }
                            }
                        }
                    }

                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("mount.move", outputs, errors)),
                    )
                }
            }
            // Inline one mount into the current store.
            | BlockCommands::Mount(MountCommands::Inline(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => match store.inline_mount(&block_id, base_dir) {
                            | Ok(()) => (store, CliResult::Success),
                            | Err(e) => (store, CliResult::Error(format!("Inline failed: {}", e))),
                        },
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => match store.inline_mount(&block_id, base_dir) {
                                | Ok(()) => outputs.push(BatchOutput::Success { input }),
                                | Err(e) => errors.push(BatchError {
                                    input,
                                    error: format!("Inline failed: {}", e),
                                }),
                            },
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("mount.inline", outputs, errors)),
                    )
                }
            }
            // Inline all mounts recursively under a block.
            | BlockCommands::Mount(MountCommands::InlineRecursive(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => match store.inline_mount_recursive(&block_id, base_dir)
                        {
                            | Ok(count) => (store, CliResult::MountInlined(count)),
                            | Err(e) => {
                                (store, CliResult::Error(format!("Inline recursive failed: {}", e)))
                            }
                        },
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                match store.inline_mount_recursive(&block_id, base_dir) {
                                    | Ok(count) => {
                                        outputs.push(BatchOutput::InlinedCount { input, count })
                                    }
                                    | Err(e) => errors.push(BatchError {
                                        input,
                                        error: format!("Inline recursive failed: {}", e),
                                    }),
                                }
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result(
                            "mount.inline-recursive",
                            outputs,
                            errors,
                        )),
                    )
                }
            }
            // Extract block subtree to a file.
            | BlockCommands::Mount(MountCommands::Extract(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                let format_override = cmd.format.map(Into::into);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            match store.save_subtree_to_file_with_format(
                                &block_id,
                                &cmd.output,
                                base_dir,
                                format_override,
                            ) {
                                | Ok(()) => (store, CliResult::Success),
                                | Err(e) => {
                                    (store, CliResult::Error(format!("Extract failed: {}", e)))
                                }
                            }
                        }
                    }
                } else {
                    if !Self::is_directory_like(&cmd.output) {
                        return (
                            store,
                            CliResult::Error(
                                "Batch mount extract requires a directory-like --output PATH"
                                    .to_string(),
                            ),
                        );
                    }

                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    let ext = format_override.map(Self::mount_format_extension).unwrap_or("json");

                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let path = Self::batch_child_file_path(&cmd.output, &input, ext);
                                match store.save_subtree_to_file_with_format(
                                    &block_id,
                                    &path,
                                    base_dir,
                                    format_override,
                                ) {
                                    | Ok(()) => outputs.push(BatchOutput::Success { input }),
                                    | Err(e) => errors.push(BatchError {
                                        input,
                                        error: format!("Extract failed: {}", e),
                                    }),
                                }
                            }
                        }
                    }

                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("mount.extract", outputs, errors)),
                    )
                }
            }
            // Get mount information (path, format, expanded state).
            | BlockCommands::Mount(MountCommands::Info(cmd)) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let node = store.node(&block_id);
                            let mount_entry = store.mount_table().entry(block_id);
                            let result = match (node, mount_entry) {
                                | (Some(store_module::BlockNode::Mount { path, format }), None) => {
                                    CliResult::MountInfo {
                                        path: Some(path.display().to_string()),
                                        format: format!("{}", format),
                                        expanded: false,
                                    }
                                }
                                | (_, Some(entry)) => CliResult::MountInfo {
                                    path: Some(entry.path.display().to_string()),
                                    format: format!("{}", entry.format),
                                    expanded: true,
                                },
                                | _ => CliResult::Error("Block is not a mount".to_string()),
                            };
                            (store, result)
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let node = store.node(&block_id);
                                let mount_entry = store.mount_table().entry(block_id);
                                match (node, mount_entry) {
                                    | (
                                        Some(store_module::BlockNode::Mount { path, format }),
                                        None,
                                    ) => outputs.push(BatchOutput::MountInfo {
                                        input,
                                        path: Some(path.display().to_string()),
                                        format: format!("{}", format),
                                        expanded: false,
                                    }),
                                    | (_, Some(entry)) => outputs.push(BatchOutput::MountInfo {
                                        input,
                                        path: Some(entry.path.display().to_string()),
                                        format: format!("{}", entry.format),
                                        expanded: true,
                                    }),
                                    | _ => errors.push(BatchError {
                                        input,
                                        error: "Block is not a mount".to_string(),
                                    }),
                                }
                            }
                        }
                    }
                    (
                        store,
                        CliResult::Batch(Self::make_batch_result("mount.info", outputs, errors)),
                    )
                }
            }
            // Save all expanded mounts to their source files.
            | BlockCommands::Mount(MountCommands::Save(_)) => match store.save_mounts() {
                | Ok(()) => (store, CliResult::Success),
                | Err(e) => (store, CliResult::Error(format!("Save mounts failed: {}", e))),
            },
            // ========================================================================
            // Panel Commands
            // ========================================================================
            // Sidebar UI state management.
            // Set panel sidebar state.
            | BlockCommands::Panel(PanelCommands::Set(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        store.set_block_panel_state(&block_id, Some(cmd.panel.into()));
                        (store, CliResult::Success)
                    }
                }
            }
            // Get panel sidebar state.
            | BlockCommands::Panel(PanelCommands::Get(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let state = store.block_panel_state(&block_id).map(|s| match s {
                            | store_module::BlockPanelBarState::Friends => "friends",
                            | store_module::BlockPanelBarState::Instruction => "instruction",
                        });
                        (store, CliResult::BlockPanelState(state.map(String::from)))
                    }
                }
            }
            // Clear panel sidebar state.
            | BlockCommands::Panel(PanelCommands::Clear(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        store.set_block_panel_state(&block_id, None);
                        (store, CliResult::Success)
                    }
                }
            }
            // ========================================================================
            // Context Command
            // ========================================================================
            // LLM context preparation.
            // Get block context for LLM requests.
            | BlockCommands::Context(cmd) => {
                let targets = Self::expand_cli_targets(&cmd.block_id);
                if targets.len() == 1 {
                    let id = Self::resolve_block_id(&store, &targets[0]);
                    match id {
                        | None => (store, CliResult::Error("Unknown block ID".to_string())),
                        | Some(block_id) => {
                            let context = store.block_context_for_id(&block_id);
                            let lineage: Vec<String> =
                                context.lineage.points().map(String::from).collect();
                            let children = context.existing_children;
                            let friends = context.friend_blocks.len();
                            (store, CliResult::Context { lineage, children, friends })
                        }
                    }
                } else {
                    let mut outputs = Vec::new();
                    let mut errors = Vec::new();
                    for target in targets {
                        let input = target.0.clone();
                        match Self::resolve_block_id(&store, &target) {
                            | None => errors
                                .push(BatchError { input, error: "Unknown block ID".to_string() }),
                            | Some(block_id) => {
                                let context = store.block_context_for_id(&block_id);
                                outputs.push(BatchOutput::Context {
                                    input,
                                    lineage: context.lineage.points().map(String::from).collect(),
                                    children: context.existing_children,
                                    friends: context.friend_blocks.len(),
                                });
                            }
                        }
                    }
                    (store, CliResult::Batch(Self::make_batch_result("context", outputs, errors)))
                }
            }
        }
    }

    /// Expand one CLI ID field into one-or-many targets.
    ///
    /// Batch mode is enabled by providing comma-separated IDs in a single
    /// argument (for example, `1v1,2v1,3v1`). Empty tokens are ignored.
    fn expand_cli_targets(single: &BlockId) -> Vec<BlockId> {
        let mut all = Vec::new();
        for part in single.0.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            all.push(BlockId(trimmed.to_string()));
        }
        if all.is_empty() {
            all.push(single.clone());
        }
        all
    }

    /// Expand two CLI ID fields into operation pairs.
    ///
    /// Pairing rules:
    /// - same lengths: zip by index,
    /// - one side has length 1: broadcast over the other side,
    /// - otherwise: return an error.
    fn expand_cli_pairs(
        left: &BlockId, right: &BlockId,
    ) -> Result<Vec<(BlockId, BlockId)>, String> {
        let lefts = Self::expand_cli_targets(left);
        let rights = Self::expand_cli_targets(right);

        if lefts.len() == rights.len() {
            return Ok(lefts.into_iter().zip(rights).collect());
        }

        if lefts.len() == 1 {
            let left = lefts[0].clone();
            return Ok(rights.into_iter().map(|right| (left.clone(), right)).collect());
        }

        if rights.len() == 1 {
            let right = rights[0].clone();
            return Ok(lefts.into_iter().map(|left| (left, right.clone())).collect());
        }

        Err(
            "batch pair mismatch: ID list lengths must match, or one side must contain exactly one ID"
                .to_string(),
        )
    }

    /// Returns true when a path should be treated as a directory target.
    fn is_directory_like(path: &std::path::Path) -> bool {
        path.is_dir() || path.extension().is_none()
    }

    /// Build a per-target file path under a directory-like base path.
    fn batch_child_file_path(
        base: &std::path::Path, target: &str, ext: &str,
    ) -> std::path::PathBuf {
        base.join(format!("{}.{}", target, ext))
    }

    /// File extension used for each mount format in batch path generation.
    fn mount_format_extension(format: store_module::MountFormat) -> &'static str {
        match format {
            | store_module::MountFormat::Json => "json",
            | store_module::MountFormat::Markdown => "md",
        }
    }

    /// Best-effort mount format lookup for a block.
    fn mount_format_for_block(
        store: &BlockStore, block_id: &store_module::BlockId,
    ) -> Option<store_module::MountFormat> {
        if let Some(entry) = store.mount_table().entry(*block_id) {
            return Some(entry.format);
        }
        match store.node(block_id) {
            | Some(store_module::BlockNode::Mount { format, .. }) => Some(*format),
            | _ => None,
        }
    }

    /// Build a standardized continue-on-error batch result.
    fn make_batch_result(
        operation: &str, outputs: Vec<BatchOutput>, errors: Vec<BatchError>,
    ) -> BatchResult {
        let successes = outputs.len();
        let failures = errors.len();
        BatchResult { operation: operation.to_string(), successes, failures, outputs, errors }
    }

    /// Resolve a CLI BlockId string to an actual store BlockId.
    ///
    /// Performs flexible, case-insensitive matching on block ID strings.
    /// Format: `NvG` where N=index and G=generation (e.g., `1v1`, `2v3`).
    ///
    /// # Arguments
    ///
    /// - `store`: The store to search
    /// - `cli_id`: The CLI-provided ID string
    ///
    /// # Returns
    ///
    /// `Some(BlockId)` if a matching block exists, `None` otherwise.
    fn resolve_block_id(store: &BlockStore, cli_id: &BlockId) -> Option<crate::store::BlockId> {
        let cli_str = &cli_id.0;
        for (id, _) in &store.nodes {
            let id_str = format!("{}", id);
            if id_str.eq_ignore_ascii_case(cli_str) {
                return Some(id);
            }
        }
        None
    }

    /// Find the nearest query match before/after a cursor block in DFS order.
    ///
    /// Matching uses [`BlockStore::find_block_point`], so mixed-language phrase
    /// tokenization and full-query fallback stay consistent with `block find`.
    ///
    /// # Behavior
    /// - Returns `None` when query is empty or no matches exist.
    /// - For `forward = true`, returns the nearest match strictly after `cursor`.
    /// - For `forward = false`, returns the nearest match strictly before `cursor`.
    /// - If no strict candidate exists and `wrap` is true, wraps to first/last match.
    fn find_relative_query_match(
        store: &BlockStore, cursor: &store_module::BlockId, query: &str, forward: bool, wrap: bool,
    ) -> Option<store_module::BlockId> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }

        let matches = store.find_block_point(query);
        if matches.is_empty() {
            return None;
        }

        let all_ids = store.find_block_point("");
        let cursor_position = all_ids.iter().position(|id| id == cursor)?;

        if forward {
            for matched in &matches {
                let Some(position) = all_ids.iter().position(|id| id == matched) else {
                    continue;
                };
                if position > cursor_position {
                    return Some(*matched);
                }
            }
            if wrap {
                return matches.first().copied();
            }
            return None;
        }

        let mut candidate = None;
        for matched in &matches {
            let Some(position) = all_ids.iter().position(|id| id == matched) else {
                continue;
            };
            if position < cursor_position {
                candidate = Some(*matched);
            } else {
                break;
            }
        }

        if candidate.is_some() {
            candidate
        } else if wrap {
            matches.last().copied()
        } else {
            None
        }
    }
}
