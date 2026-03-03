#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use blooming_blockery::BloomingBlockery;

fn main() -> anyhow::Result<()> {
    BloomingBlockery::run_gui()
}
