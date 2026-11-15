# rs-suite

A Cargo workspace monorepo for fast Rust CLI and TUI system utilities.

## Workspace Crates

- **[`rsdu`](crates/rsdu/README.md)** — Fast disk usage analyzer (`rsdu` binary). See [crates/rsdu/README.md](crates/rsdu/README.md).
- **[`rstop`](crates/rstop/README.md)** — Interactive system and process monitor (`rstop` binary). See [crates/rstop/README.md](crates/rstop/README.md).
- **[`rs-common`](crates/rs-common/)** — Shared internal library for size/rate/duration formatting, TUI color palettes & style helpers, and `/proc` parsing utilities.

## Build & Test

```bash
# Check, test, and lint the entire workspace
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets

# Build individual binaries
cargo build --release -p rsdu
cargo build --release -p rstop
```
