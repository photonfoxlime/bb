#![doc = include_str!("../DESIGN.md")]
rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
mod cli;
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
use clap::{CommandFactory, Parser};
use std::path::PathBuf;

pub struct BloomingBlockery;

impl BloomingBlockery {
    /// Runs the Basic Block CLI entry point (used by the `bb` binary).
    pub fn run_cli(binary_name: &str) -> anyhow::Result<()> {
        #[cfg(feature = "log")]
        Self::init_tracing()?;

        let cli = Cli::parse();
        match cli.command {
            | Commands::GenerateCompletion { shell } => {
                clap_complete::generate(
                    shell,
                    &mut Cli::command(),
                    binary_name,
                    &mut std::io::stdout(),
                );
            }
            | cmd => {
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
            }
        }
        Ok(())
    }

    /// Runs the Blooming Blockery GUI entry point (used by the `blooming-blockery` binary).
    pub fn run_gui() -> anyhow::Result<()> {
        #[cfg(feature = "log")]
        Self::init_tracing()?;

        Self::gui()
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
