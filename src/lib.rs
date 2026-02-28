#![doc = include_str!("../README.md")]
rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
mod cli;
mod i18n;
mod store;
mod llm;
mod paths;
pub mod text;
mod theme;
mod undo;

use self::{
    app::AppState,
    cli::{BlockCli, CliResult, Commands, print_result},
    paths::AppPaths,
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
                // Load store from CLI path or default to AppPaths::data_file()
                let store_path = cli
                    .store
                    .clone()
                    .or_else(AppPaths::data_file)
                    .ok_or_else(|| anyhow::anyhow!("failed to determine data file path"))?;
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

                // Save store if command didn't fail
                if !matches!(result, CliResult::Error(_)) {
                    let () = store.save()?;
                }

                // Print result with formatting
                print_result(&result, cli.output);
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
}
