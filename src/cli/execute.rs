//! BlockCommands execution implementation.

use crate::cli::results::{CliResult, ExpansionDraftInfo, FriendInfo, Match, ReductionDraftInfo};
use crate::cli::types::BlockId;
use crate::cli::{
    BlockCommands,
    context::ContextCommand,
    draft::DraftCommands,
    fold::FoldCommands,
    friend::FriendCommands,
    mount::MountCommands,
    nav::NavCommands,
    panel::PanelCommands,
    query::{FindCommand, RootCommand, ShowCommand},
    tree::TreeCommands,
};
use crate::store as store_module;
use crate::store::{BlockStore, Direction};

impl BlockCommands {
    /// Execute a block command with the given store.
    ///
    /// This method handles all block manipulation commands, operating on the
    /// provided store and returning the modified store (or the same one if
    /// no changes were made).
    ///
    /// # Arguments
    ///
    /// - `store`: The block store to operate on
    /// - `base_dir`: Base directory for resolving relative mount paths
    /// - `output`: Output format for query results
    ///
    /// # Returns
    ///
    /// Modified store (or original if no changes) and command result.
    pub fn execute(
        self, mut store: BlockStore, base_dir: &std::path::Path, output: super::OutputFormat,
    ) -> (BlockStore, CliResult) {
        match self {
            // Query commands - no store modification
            | BlockCommands::Roots(RootCommand {}) => {
                let roots: Vec<String> =
                    store.roots().iter().map(|id| format!("{:?}", id)).collect();
                (store, CliResult::Roots(roots))
            }
            | BlockCommands::Show(cmd) => {
                let id = Self::resolve_block_id(&store, &cmd.block_id);
                match id {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(id) => {
                        let text = store.point(&id).unwrap_or_default();
                        let children: Vec<String> =
                            store.children(&id).iter().map(|c| format!("{:?}", c)).collect();
                        (store, CliResult::Show { id, text, children })
                    }
                }
            }
            | BlockCommands::Find(cmd) => {
                let matches: Vec<Match> = store
                    .roots()
                    .iter()
                    .flat_map(|root| Self::find_in_subtree(&store, root, &cmd.query))
                    .take(cmd.limit)
                    .collect();
                (store, CliResult::Find(matches))
            }
            // Tree commands
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
            // Navigation commands
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
            // Draft commands
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
                                    .map(|id| format!("{:?}", id))
                                    .collect(),
                            });
                        let instruction =
                            store.instruction_draft(&block_id).map(|d| d.instruction.clone());
                        let inquiry = store.inquiry_draft(&block_id).map(|d| d.response.clone());
                        (store, CliResult::DraftList { expansion, reduction, instruction, inquiry })
                    }
                }
            }
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
            // Fold commands
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
            // Friend commands
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
            | BlockCommands::Friend(FriendCommands::List(cmd)) => {
                let target = Self::resolve_block_id(&store, &cmd.target_id);
                match target {
                    | None => (store, CliResult::Error("Unknown block ID".to_string())),
                    | Some(tid) => {
                        let friends: Vec<FriendInfo> = store
                            .friend_blocks_for(&tid)
                            .iter()
                            .map(|f| FriendInfo {
                                id: format!("{:?}", f.block_id),
                                perspective: f.perspective.clone(),
                                telescope_lineage: f.parent_lineage_telescope,
                                telescope_children: f.children_telescope,
                            })
                            .collect();
                        (store, CliResult::FriendList(friends))
                    }
                }
            }
            // Mount commands
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
            // Panel commands
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
            // Context command
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

    /// Resolve a CLI BlockId to an actual store BlockId.
    fn resolve_block_id(store: &BlockStore, cli_id: &BlockId) -> Option<crate::store::BlockId> {
        let cli_str = cli_id.0.strip_prefix("0x").unwrap_or(&cli_id.0);
        for (id, _) in &store.nodes {
            let id_str = format!("{:?}", id);
            let id_str = id_str.strip_prefix("0x").unwrap_or(&id_str);
            if id_str.eq_ignore_ascii_case(cli_str) {
                return Some(id);
            }
        }
        None
    }

    /// Find all blocks matching a query in their text content.
    fn find_in_subtree(
        store: &BlockStore, root: &crate::store::BlockId, query: &str,
    ) -> Vec<Match> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        Self::find_recursive(store, root, &query_lower, &mut results);
        results
    }

    fn find_recursive(
        store: &BlockStore, id: &crate::store::BlockId, query: &str, results: &mut Vec<Match>,
    ) {
        if let Some(text) = store.point(id) {
            if text.to_lowercase().contains(query) {
                results.push(Match { id: format!("{:?}", id), text });
            }
        }
        for child in store.children(id) {
            Self::find_recursive(store, child, query, results);
        }
    }
}
