#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use blooming_blockery::{BloomingBlockery, cli::Cli};
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    BloomingBlockery::run(cli)
}
