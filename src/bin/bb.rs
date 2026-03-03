//! Basic Block: CLI binary `bb` for block store manipulation.
//!
//! For Blooming Blockery (GUI), use the `blooming-blockery` binary.

use blooming_blockery::BloomingBlockery;

fn main() -> anyhow::Result<()> {
    BloomingBlockery::run_cli("bb")
}
