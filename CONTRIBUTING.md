# Contributing

`bb` is a standard Rust project that follows common Rust project contribution procedures.

## Setup

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone
git clone git@github.com:photonfoxlime/bb.git
cd bb
```

## Development

- Build: `cargo build`
- Format code: `cargo fmt`
- Lint: `cargo clippy`
- Test: `cargo test`

## Submitting Changes

1. Create a branch
2. Make your changes
3. Run `cargo fmt` and `cargo clippy`
4. Ensure tests pass: `cargo test`
5. Submit a pull request
