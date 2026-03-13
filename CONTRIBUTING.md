# Contributing

`blooming-blockery` is a Rust application with two operator surfaces over the same core block store:

- `cargo run` launches the GUI editor.
- `cargo run -- <subcommand>` runs the CLI (`roots`, `tree`, `draft`, `mount`, and others).

This guide focuses on repository-specific expectations for human contributors.

## Setup

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone
jj git clone --colocate git@github.com:photonfoxlime/bb.git
cd bb
```

On Linux, CI also installs `nasm` and `binutils` before building. If your local build fails on a fresh machine, install those first.

## Daily Workflow

```bash
# Launch the GUI
cargo run

# Inspect CLI commands
cargo run -- --help

# Run a CLI command against the same store model
cargo run -- roots

# Format, lint, test
cargo fmt -- --check
cargo clippy
cargo test
```

For behavior changes, prefer running with tracing enabled while you iterate:

```bash
RUST_LOG=blooming_blockery=debug cargo run
```

## Documentation Expectations

Inline Rust documentation in `src/**/*.rs` is the canonical documentation source for this repository.

- Document all public APIs with `//!` and `///`.
- Keep design rationale close to the owning module, type, field, or method.
- Use `/// Note:` for unusual design choices or compromises.
- Keep documentation concise, accurate, and in English.
- Standalone docs can supplement the code, but they should not replace inline Rust docs.

## Rust Code Guidelines

- Fail fast. If an edge case is not part of the design, log it and stop the current operation instead of guessing at recovery.
- Prefer typed data structures over stringly-typed state and ad hoc parsing.
- Prefer declarative crates and derive-based APIs where they fit: `thiserror`, `serde`, and derive-style `clap`.
- Prefer methods on structs over free functions when a type can own the namespace or enforce invariants.
- When a builder consumes its fields, use `build(self)` plus chainable `with_*` setters. When a builder can be reused, prefer `build(&self)` plus `set_*` setters.
- When adding new features or important flows, record useful events with `tracing`.

## UI and UX Changes

- Put UI numeric values in [`src/theme.rs`](./src/theme.rs). Do not scatter magic numbers for sizes, padding, gaps, or colors.
- All user-facing text must go through `rust_i18n::t!`.
- When adding or changing UI copy, update all locale files:
  - [`locales/en-US.yml`](./locales/en-US.yml)
  - [`locales/ja.yml`](./locales/ja.yml)
  - [`locales/zh-CN.yml`](./locales/zh-CN.yml)
- When adding, removing, or changing keyboard shortcuts, update the in-app shortcut guide in [`src/app/shortcut_help_banner.rs`](./src/app/shortcut_help_banner.rs) and keep locale entries in sync.

## Commits and History

This repository uses Jujutsu (`jj`) for local history management, but GitHub pull requests are still the review surface.

Commit messages use this format:

```text
prefix: lowercase description
```

Available prefixes:

- `feat`: user-visible capability
- `incr`: incremental improvement to an existing feature
- `sisy`: mechanical refactor or housekeeping with no behavior change
- `vibe`: exploratory or prototype-quality work
- `repo`: repository maintenance
- `docs`: documentation-only change
- `test`: tests only

Additional expectations:

- Keep one logical change per commit.
- Pair code changes with their tests in the same commit.
- Prefer a sequence of small commits over one large commit.

## Submitting Changes

1. Create a branch or `jj` change for one logical piece of work.
2. Make the code change and update inline Rust docs where the new understanding belongs.
3. Update locale files, shortcut help, and `theme.rs` constants when your change touches those areas.
4. Run `cargo fmt -- --check`, `cargo clippy`, and `cargo test`.
5. Open a pull request against `main`.
