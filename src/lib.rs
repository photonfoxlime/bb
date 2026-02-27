#![doc = include_str!("../README.md")]
rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
mod cli;
mod i18n;
mod store;
mod llm;
mod paths;
mod theme;
mod undo;

use self::{
    app::AppState,
    cli::{BlockCli, CliResult, Commands},
    store::BlockStore,
};
use clap::{CommandFactory, Parser};
use std::path::PathBuf;

pub struct BloomingBlockery;

impl BloomingBlockery {
    pub fn main() -> anyhow::Result<()> {
        #[cfg(feature = "log")]
        Self::init_tracing()?;

        let cli = BlockCli::parse();
        // Handle CLI commands
        match cli.command {
            | Some(Commands::GenerateCompletion { shell }) => {
                clap_complete::generate(
                    shell,
                    &mut BlockCli::command(),
                    "blooming-blockery",
                    &mut std::io::stdout(),
                );
            }
            | Some(Commands::Gui) | None => {
                let () = Self::gui()?;
            }
            // Block store manipulation commands
            | Some(Commands::Block(block_commands)) => {
                // Load store from CLI path or default
                let store_path = cli.store.clone().unwrap_or_else(|| PathBuf::from("blocks.json"));
                let base_dir = cli
                    .store
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));

                let store = BlockStore::load_from_path(&store_path)
                    .unwrap_or_else(|_| BlockStore::default());

                // Execute the command
                let (store, result) = block_commands.execute(store, &base_dir);

                // Handle output based on result type
                match result {
                    | CliResult::Success => {
                        // Save and print success
                        let _ = store.save();
                        println!("OK");
                    }
                    | CliResult::Error(msg) => {
                        eprintln!("Error: {}", msg);
                    }
                    | CliResult::Roots(ids) => {
                        Self::print_roots(&ids, cli.output);
                    }
                    | CliResult::Show { id, text, children } => {
                        Self::print_show(id, &text, &children, cli.output);
                    }
                    | CliResult::Find(matches) => {
                        Self::print_find(&matches, cli.output);
                    }
                    | CliResult::BlockId(id) => {
                        Self::print_block_id(id, cli.output);
                    }
                    | CliResult::OptionalBlockId(id) => {
                        Self::print_optional_block_id(id.as_ref(), cli.output);
                    }
                    | CliResult::Removed(ids) => {
                        Self::print_removed(&ids, cli.output);
                    }
                    | CliResult::Collapsed(collapsed) => {
                        Self::print_collapsed(collapsed, cli.output);
                    }
                    | CliResult::Lineage(points) => {
                        Self::print_lineage(&points, cli.output);
                    }
                    | CliResult::Context { lineage, children, friends } => {
                        Self::print_context(&lineage, &children, friends, cli.output);
                    }
                    | CliResult::DraftList { expansion, reduction, instruction, inquiry } => {
                        Self::print_draft_list(
                            expansion.as_ref(),
                            reduction.as_ref(),
                            instruction.as_ref(),
                            inquiry.as_ref(),
                            cli.output,
                        );
                    }
                    | CliResult::FriendList(friends) => {
                        Self::print_friend_list(&friends, cli.output);
                    }
                    | CliResult::MountInfo { ref path, ref format, expanded } => {
                        Self::print_mount_info(path.as_deref(), format, expanded, cli.output);
                    }
                    | CliResult::PanelState(state) => {
                        Self::print_panel_state(state.as_deref(), cli.output);
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "log")]
    pub fn init_tracing() -> anyhow::Result<()> {
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("blooming_blockery=info"));
        let () = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .try_init()
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    pub fn gui() -> anyhow::Result<()> {
        let () = iced::application(AppState::load, AppState::update, AppState::view)
            .subscription(AppState::subscription)
            .font(include_bytes!("../assets/fonts/Inter-300.woff2").as_slice())
            .font(include_bytes!("../assets/fonts/Inter-400.woff2").as_slice())
            .font(include_bytes!("../assets/fonts/Inter-500.woff2").as_slice())
            .font(include_bytes!("../assets/fonts/LXGWWenKai-Light.ttf").as_slice())
            .font(include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf").as_slice())
            .font(include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf").as_slice())
            .font(lucide_icons::LUCIDE_FONT_BYTES)
            .default_font(theme::DEFAULT_FONT)
            .theme(|state: &AppState| AppState::theme(state.is_dark))
            .title("Blooming Blockery")
            .run()?;
        Ok(())
    }

    // ============================================================================
    // CLI Output Helpers
    // ============================================================================

    fn print_roots(ids: &[String], output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "roots": ids }));
            }
            | cli::OutputFormat::Table => {
                for id in ids {
                    println!("{}", id);
                }
            }
        }
    }

    fn print_show(id: store::BlockId, text: &str, children: &[String], output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "id": format!("{:?}", id),
                        "text": text,
                        "children": children
                    })
                );
            }
            | cli::OutputFormat::Table => {
                println!("ID:       {:?}", id);
                println!("Text:     {}", text);
                println!("Children: {:?}", children);
            }
        }
    }

    fn print_find(matches: &[cli::Match], output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::to_string(matches).unwrap_or_default());
            }
            | cli::OutputFormat::Table => {
                for m in matches {
                    println!("{}: {}", m.id, m.text);
                }
            }
        }
    }

    fn print_lineage(points: &[String], output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "lineage": points }));
            }
            | cli::OutputFormat::Table => {
                for (i, p) in points.iter().enumerate() {
                    println!("{}. {}", i + 1, p);
                }
            }
        }
    }

    fn print_context(
        lineage: &[String], children: &[String], friends: usize, output: cli::OutputFormat,
    ) {
        match output {
            | cli::OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "lineage": lineage,
                        "children": children,
                        "friends": friends
                    })
                );
            }
            | cli::OutputFormat::Table => {
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
        expansion: Option<&cli::ExpansionDraftInfo>, reduction: Option<&cli::ReductionDraftInfo>,
        instruction: Option<&String>, inquiry: Option<&String>, output: cli::OutputFormat,
    ) {
        match output {
            | cli::OutputFormat::Json => {
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
            | cli::OutputFormat::Table => {
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

    fn print_friend_list(friends: &[cli::FriendInfo], output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::to_string(friends).unwrap_or_default());
            }
            | cli::OutputFormat::Table => {
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

    fn print_mount_info(
        path: Option<&str>, format: &str, expanded: bool, output: cli::OutputFormat,
    ) {
        match output {
            | cli::OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "path": path,
                        "format": format,
                        "expanded": expanded
                    })
                );
            }
            | cli::OutputFormat::Table => {
                println!("Path: {}", path.unwrap_or("(none)"));
                println!("Format: {}", format);
                println!("Expanded: {}", expanded);
            }
        }
    }

    fn print_panel_state(state: Option<&str>, output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "panel": state }));
            }
            | cli::OutputFormat::Table => match state {
                | Some(s) => println!("{}", s),
                | None => println!("(not set)"),
            },
        }
    }

    fn print_block_id(id: store::BlockId, output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "id": format!("{:?}", id) }));
            }
            | cli::OutputFormat::Table => {
                println!("{:?}", id);
            }
        }
    }

    fn print_optional_block_id(id: Option<&store::BlockId>, output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "id": id.map(|i| format!("{:?}", i)) }));
            }
            | cli::OutputFormat::Table => {
                if let Some(id) = id {
                    println!("{:?}", id);
                }
            }
        }
    }

    fn print_removed(ids: &[String], output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "removed": ids }));
            }
            | cli::OutputFormat::Table => {
                for id in ids {
                    println!("Removed: {}", id);
                }
            }
        }
    }

    fn print_collapsed(collapsed: bool, output: cli::OutputFormat) {
        match output {
            | cli::OutputFormat::Json => {
                println!("{}", serde_json::json!({ "collapsed": collapsed }));
            }
            | cli::OutputFormat::Table => {
                println!("Collapsed: {}", collapsed);
            }
        }
    }
}
