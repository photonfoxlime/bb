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
use super::results::{CliResult, ExpansionDraftInfo, FriendInfo, Match, ReductionDraftInfo};
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
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(id) => {
                        let text = store.point(&id).unwrap_or_default();
                        let children: Vec<String> =
                            store.children(&id).iter().map(|c| format!("{}", c)).collect();
                        (store, CliResult::Show { id, text, children })
                    }
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
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        store.update_point(&block_id, cmd.text);
                        (store, CliResult::Success)
                    }
                }
            }
            // ========================================================================
            // Tree Commands
            // ========================================================================
            // Structural editing operations for modifying the block hierarchy.
            // Add a child block to a parent.
            | BlockCommands::Tree(TreeCommands::AddChild(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.parent_id);
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
            }
            // Add a sibling block after the target.
            | BlockCommands::Tree(TreeCommands::AddSibling(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
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
            }
            // Wrap a block in a new parent.
            | BlockCommands::Tree(TreeCommands::Wrap(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let new_id = store.insert_parent(&block_id, cmd.text.clone());
                        match new_id {
                            | Some(new_id) => (store, CliResult::BlockId(new_id)),
                            | None => (store, CliResult::Error("Failed to wrap block".to_string())),
                        }
                    }
                }
            }
            // Duplicate a block and its entire subtree.
            | BlockCommands::Tree(TreeCommands::Duplicate(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let new_id = store.duplicate_subtree_after(&block_id);
                        match new_id {
                            | Some(new_id) => (store, CliResult::BlockId(new_id)),
                            | None => (store, CliResult::Error("Failed to duplicate".to_string())),
                        }
                    }
                }
            }
            // Delete a block and its entire subtree.
            | BlockCommands::Tree(TreeCommands::Delete(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let removed = store.remove_block_subtree(&block_id);
                        match removed {
                            | Some(ids) => {
                                let ids_str: Vec<String> =
                                    ids.iter().map(|i| format!("{:?}", i)).collect();
                                (store, CliResult::Removed(ids_str))
                            }
                            | None => (store, CliResult::Error("Failed to delete".to_string())),
                        }
                    }
                }
            }
            // Move a block relative to a target.
            | BlockCommands::Tree(TreeCommands::Move(cmd)) => {
                let source = Self::resolve_block_id(&store, &cmd.source_id);
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                match (source, target) {
                    | (Some(src), Some(tgt)) => {
                        let dir = if cmd.before {
                            Direction::Before
                        } else if cmd.after {
                            Direction::After
                        } else {
                            Direction::Under
                        };
                        let result = store.move_block(&src, &tgt, dir);
                        match result {
                            | Some(()) => (store, CliResult::Success),
                            | None => (
                                store,
                                CliResult::Error("Move failed (check constraints)".to_string()),
                            ),
                        }
                    }
                    | _ => {
                        (store, CliResult::Error("Unknown source or target block ID".to_string()))
                    }
                }
            }
            // ========================================================================
            // Navigation Commands
            // ========================================================================
            // DFS-based navigation helpers for traversing the block tree.
            // Get the next visible block in DFS order.
            | BlockCommands::Nav(NavCommands::Next(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let next = store.next_visible_in_dfs(&block_id);
                        (store, CliResult::OptionalBlockId(next))
                    }
                }
            }
            // Get the previous visible block in DFS order.
            | BlockCommands::Nav(NavCommands::Prev(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let prev = store.prev_visible_in_dfs(&block_id);
                        (store, CliResult::OptionalBlockId(prev))
                    }
                }
            }
            // Get the lineage (ancestor chain) of a block.
            | BlockCommands::Nav(NavCommands::Lineage(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let lineage = store.lineage_points_for_id(&block_id);
                        let points: Vec<String> = lineage.points().map(String::from).collect();
                        (store, CliResult::Lineage(points))
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
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        store.set_instruction_draft(block_id, cmd.text);
                        (store, CliResult::Success)
                    }
                }
            }
            // Set an inquiry draft (LLM response).
            | BlockCommands::Draft(DraftCommands::Inquiry(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        store.set_inquiry_draft(block_id, cmd.response);
                        (store, CliResult::Success)
                    }
                }
            }
            // List all drafts for a block.
            | BlockCommands::Draft(DraftCommands::List(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
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
                        let inquiry = store.inquiry_draft(&block_id).map(|d| d.response.clone());
                        (store, CliResult::DraftList { expansion, reduction, instruction, inquiry })
                    }
                }
            }
            // Clear drafts from a block.
            | BlockCommands::Draft(DraftCommands::Clear(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
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
            }
            // ========================================================================
            // Fold Commands
            // ========================================================================
            // Visibility state management for collapsing/expanding blocks.
            // Toggle collapsed state of a block.
            | BlockCommands::Fold(FoldCommands::Toggle(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let collapsed = store.toggle_collapsed(&block_id);
                        (store, CliResult::Collapsed(collapsed))
                    }
                }
            }
            // Get collapsed state of a block.
            | BlockCommands::Fold(FoldCommands::Status(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let collapsed = store.is_collapsed(&block_id);
                        (store, CliResult::Collapsed(collapsed))
                    }
                }
            }
            // ========================================================================
            // Friend Commands
            // ========================================================================
            // Cross-reference link management between blocks.
            // Add a friend (cross-reference) link.
            | BlockCommands::Friend(FriendCommands::Add(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                let friend = Self::resolve_block_id(&store, &cmd.friend_id);
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
            }
            // Remove a friend (cross-reference) link.
            | BlockCommands::Friend(FriendCommands::Remove(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                let friend = Self::resolve_block_id(&store, &cmd.friend_id);
                match (target, friend) {
                    | (Some(tid), Some(fid)) => {
                        let mut friends = store.friend_blocks_for(&tid).to_vec();
                        friends.retain(|f| f.block_id != fid);
                        store.set_friend_blocks_for(&tid, friends);
                        (store, CliResult::Success)
                    }
                    | _ => (store, CliResult::Error("Unknown block ID".to_string())),
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
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let result = store.set_mount_path_with_format(
                            &block_id,
                            cmd.path,
                            cmd.format.into(),
                        );
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
            }
            // Expand a mount by loading external file contents.
            | BlockCommands::Mount(MountCommands::Expand(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => match store.expand_mount(&block_id, base_dir) {
                        | Ok(_) => (store, CliResult::Success),
                        | Err(e) => (store, CliResult::Error(format!("Expand failed: {}", e))),
                    },
                }
            }
            // Collapse a mount, removing loaded children.
            | BlockCommands::Mount(MountCommands::Collapse(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => match store.collapse_mount(&block_id) {
                        | Some(()) => (store, CliResult::Success),
                        | None => {
                            (store, CliResult::Error("Block is not an expanded mount".to_string()))
                        }
                    },
                }
            }
            // Extract block subtree to a file.
            | BlockCommands::Mount(MountCommands::Extract(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        match store.save_subtree_to_file(&block_id, &cmd.output, base_dir) {
                            | Ok(()) => (store, CliResult::Success),
                            | Err(e) => (store, CliResult::Error(format!("Extract failed: {}", e))),
                        }
                    }
                }
            }
            // Get mount information (path, format, expanded state).
            | BlockCommands::Mount(MountCommands::Info(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        let node = store.node(&block_id);
                        let mount_entry = store.mount_table().entry(block_id);
                        let result = match (node, mount_entry) {
                            | (Some(store_module::BlockNode::Mount { path, format }), None) => {
                                CliResult::MountInfo {
                                    path: Some(path.display().to_string()),
                                    format: format!("{:?}", format),
                                    expanded: false,
                                }
                            }
                            | (_, Some(entry)) => CliResult::MountInfo {
                                path: Some(entry.path.display().to_string()),
                                format: format!("{:?}", entry.format),
                                expanded: true,
                            },
                            | _ => CliResult::Error("Block is not a mount".to_string()),
                        };
                        (store, result)
                    }
                }
            }
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
                        store.set_panel_state(&block_id, Some(cmd.panel.into()));
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
                        let state = store.panel_state(&block_id).map(|s| match s {
                            | store_module::PanelBarState::Friends => "friends",
                            | store_module::PanelBarState::Instruction => "instruction",
                        });
                        (store, CliResult::PanelState(state.map(String::from)))
                    }
                }
            }
            // Clear panel sidebar state.
            | BlockCommands::Panel(PanelCommands::Clear(cmd)) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(block_id) => {
                        store.set_panel_state(&block_id, None);
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
                let id = Self::resolve_block_id(&store, &cmd.block_id);
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
            }
        }
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
}
