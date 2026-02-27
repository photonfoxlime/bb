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

use app::AppState;
use clap::{CommandFactory, Parser};
use clap_complete::Shell;

/// CLI arguments for blooming-blockery.
#[derive(Parser, Debug)]
#[command(name = "blooming-blockery")]
#[command(version, about, long_about = None)]
pub struct BloomingBlockery {
    /// Launch the GUI (default behavior if no subcommand is provided).
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Generate shell completions.
    GenerateCompletion {
        /// The shell to generate completions for.
        shell: Shell,
    },
    /// Launch the GUI.
    Gui,
}

impl BloomingBlockery {
    pub fn main() -> anyhow::Result<()> {
        #[cfg(feature = "log")]
        Self::init_tracing()?;

        let args = Self::parse();
        // Handle CLI commands
        match args.command {
            | Some(Commands::GenerateCompletion { shell }) => {
                clap_complete::generate(
                    shell,
                    &mut Self::command(),
                    "blooming-blockery",
                    &mut std::io::stdout(),
                );
            }
            | Some(Commands::Gui) | None => {
                let () = Self::gui()?;
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
