//! Binary entry point for `bb`.
//!
//! Runtime wiring in this file:
//! - load app state (`AppState::load`),
//! - route `update` + `view` + `subscription`,
//! - load fonts and theme from app state.

use blooming_blockery::AppState;
use clap::{CommandFactory, Parser};
use clap_complete::Shell;

// const DEFAULT_FONT: iced::Font = iced::Font::with_name("Inter");
const DEFAULT_FONT: iced::Font = iced::Font::with_name("LXGW WenKai");

/// CLI arguments for blooming-blockery.
#[derive(Parser, Debug)]
#[command(name = "blooming-blockery")]
#[command(version, about, long_about = None)]
struct Args {
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

fn main() {
    let args = Args::parse();

    // Handle CLI commands
    match args.command {
        | Some(Commands::GenerateCompletion { shell }) => {
            clap_complete::generate(
                shell,
                &mut Args::command(),
                "blooming-blockery",
                &mut std::io::stdout(),
            );
        }
        | Some(Commands::Gui) | None => {
            if let Err(e) = run_gui() {
                eprintln!("Error running GUI: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn run_gui() -> iced::Result {
    #[cfg(feature = "log")]
    init_tracing();
    iced::application(AppState::load, AppState::update, AppState::view)
        .subscription(AppState::subscription)
        .font(include_bytes!("../assets/fonts/Inter-300.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-400.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-500.woff2").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Light.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf").as_slice())
        .font(lucide_icons::LUCIDE_FONT_BYTES)
        .default_font(DEFAULT_FONT)
        .theme(|state: &AppState| AppState::theme(state.is_dark))
        .title("Blooming Blockery")
        .run()
}

#[cfg(feature = "log")]
fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("bb=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(env_filter).with_target(true).try_init();
}
