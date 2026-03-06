#![doc = include_str!("../DESIGN.md")]
//!
//! # Runtime surfaces
//!
//! Blooming Blockery ships two entry points over the same block-store core:
//!
//! A single `blooming-blockery` binary launches the iced document editor by default, or runs
//! the Basic Block CLI when a subcommand is given (roots, tree, draft, mount, etc.).
//!
//! Both runtimes operate on the same persisted document model:
//! - `blocks.json` stores the main block forest.
//! - `app.toml` stores UI and per-task LLM preferences.
//! - `llm.toml` stores provider credentials and endpoint definitions.
//! - mounted `.json` / `.md` files project subtrees into external documents.
//!
//! # Investigation summary
//!
//! The crate centers on a typed [`store::BlockStore`] forest whose nodes can be
//! plain text points or typed links, and whose optional per-block metadata
//! stores LLM drafts, instruction/probe state, friend links, fold state, and
//! panel visibility. The iced app layers transient editing state, global
//! shortcuts, per-block LLM request tracking, and responsive document/settings
//! screens over that store. The CLI exposes the same structure and metadata
//! through domain-grouped subcommands (`tree`, `point`, `draft`, `mount`,
//! `friend`, `panel`, `context`), which makes the repository effectively one
//! core model with two operator surfaces instead of two separate products.
rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
pub mod cli;
mod component;
mod i18n;
mod store;
mod llm;
mod paths;
pub mod text;
mod theme;
mod undo;

use self::{
    app::AppState,
    cli::{Cli, CliResult, Commands, print_result},
    paths::AppPaths,
    store::BlockStore,
};
use clap::CommandFactory;
use std::path::PathBuf;

/// Entry-point namespace for the Blooming Blockery binary (GUI and CLI).
///
/// The type itself carries no state; it groups the two runtime modes so both share
/// startup, tracing, and application bootstrap logic.
pub struct BloomingBlockery;

impl BloomingBlockery {
    /// Runs the merged binary: GUI when no subcommand, CLI when a subcommand is given.
    pub fn run(cli: Cli) -> anyhow::Result<()> {
        #[cfg(feature = "log")]
        Self::init_tracing()?;

        match cli.command {
            | None | Some(Commands::Gui) => Self::gui(),
            | Some(Commands::GenerateCompletion { shell }) => {
                clap_complete::generate(
                    shell,
                    &mut Cli::command(),
                    "blooming-blockery",
                    &mut std::io::stdout(),
                );
                Ok(())
            }
            | Some(cmd) => {
                let store_path = cli
                    .store
                    .clone()
                    .or_else(AppPaths::data_file)
                    .ok_or_else(|| anyhow::anyhow!("failed to determine data file path"))?;
                let base_dir = cli
                    .store
                    .as_ref()
                    .and_then(|p: &PathBuf| p.parent())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));

                let store = BlockStore::load_from_path(&store_path)
                    .unwrap_or_else(|_| BlockStore::default());
                let (store, result) = cmd.execute(store, &base_dir);

                if !matches!(result, CliResult::Error(_)) {
                    let () = store.save()?;
                }
                print_result(&result, cli.output);
                Ok(())
            }
        }
    }

    /// Runs the Blooming Blockery GUI entry point.
    pub fn run_gui() -> anyhow::Result<()> {
        #[cfg(feature = "log")]
        Self::init_tracing()?;

        Self::gui()
    }

    /// Initialize process-wide tracing subscribers when the `log` feature is enabled.
    ///
    /// Falls back to `blooming_blockery=info` when `RUST_LOG` is not set.
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

    /// Launch the iced GUI application with fonts, icon, theme, and subscriptions configured.
    pub fn gui() -> anyhow::Result<()> {
        let window_settings = {
            let mut window_settings = iced::window::Settings::default();
            window_settings.icon = iced::window::icon::from_file_data(
                include_bytes!("../assets/icons/icon.PNG").as_slice(),
                None,
            )
            .ok();
            window_settings
        };

        let () = iced::application(AppState::load, AppState::update, AppState::view)
            .subscription(AppState::subscription)
            .window(window_settings)
            .font(include_bytes!("../assets/fonts/Inter-300.woff2").as_slice())
            .font(include_bytes!("../assets/fonts/Inter-400.woff2").as_slice())
            .font(include_bytes!("../assets/fonts/Inter-500.woff2").as_slice())
            .font(include_bytes!("../assets/fonts/LXGWWenKai-Light.ttf").as_slice())
            .font(include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf").as_slice())
            .font(include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf").as_slice())
            .font(lucide_icons::LUCIDE_FONT_BYTES)
            .default_font(theme::DEFAULT_FONT)
            .theme(|state: &AppState| AppState::theme(state.is_dark_mode()))
            .title("Blooming Blockery")
            .run()?;

        Ok(())
    }
}
