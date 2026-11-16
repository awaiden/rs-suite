# rs-suite

A Cargo workspace monorepo for fast Rust CLI and TUI system utilities.

## Workspace Crates

- **[`rsdu`](crates/rsdu/README.md)** — Fast disk usage analyzer (`rsdu` binary). See [crates/rsdu/README.md](crates/rsdu/README.md).
- **[`rsfetch`](crates/rsfetch/README.md)** — Fast native Linux system information fetcher (`rsfetch` binary). See [crates/rsfetch/README.md](crates/rsfetch/README.md).
- **[`rsfind`](crates/rsfind/README.md)** — Fast parallel file finder with interactive fuzzy TUI (`rsfind` binary). See [crates/rsfind/README.md](crates/rsfind/README.md).
- **[`rsnet`](crates/rsnet/README.md)** — Interactive Linux network bandwidth and connection-per-process monitor (`rsnet` binary). See [crates/rsnet/README.md](crates/rsnet/README.md).
- **[`rssock`](crates/rssock/README.md)** — Interactive Linux socket and connection explorer (`rssock` binary). See [crates/rssock/README.md](crates/rssock/README.md).
- **[`rstop`](crates/rstop/README.md)** — Interactive system and process monitor (`rstop` binary). See [crates/rstop/README.md](crates/rstop/README.md).
- **[`rs-common`](crates/rs-common/)** — Shared internal library for size/rate/duration formatting, TUI color palettes & style helpers, and `/proc` parsing utilities.

## Installation

### Quick Install (Prebuilt Binaries)

Install all tools supported on your platform (`rsdu`, `rsfind`, `rstop`, `rsnet`, `rssock`, `rsfetch` on Linux; `rsdu`, `rsfind` on macOS arm64) into `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh
```

#### Installer Options & Examples

```sh
# Install only specific tools
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh -s -- rsdu rstop

# Install to a system-wide directory (uses sudo if needed)
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh -s -- --dir /usr/local/bin

# Pin a specific release tag
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh -s -- --version v0.1.0

# Preview actions without downloading or writing files
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh -s -- --dry-run

# Build and install from source using cargo
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh -s -- --from-source
```

Environment variables:
- `INSTALL_DIR`: Target directory (default: `~/.local/bin`)
- `RSDU_VERSION` / `RSFIND_VERSION` / `RSTOP_VERSION` / `RSNET_VERSION` / `RSSOCK_VERSION` / `RSFETCH_VERSION`: Pin a specific release tag per tool

### Supported Prebuilt Targets

| Tool | Linux `x86_64` | Linux `aarch64` | macOS `arm64` |
|---|---|---|---|
| `rsdu` | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` | `aarch64-apple-darwin` |
| `rsfind` | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` | `aarch64-apple-darwin` |
| `rstop` | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-gnu` | — (Linux only) |
| `rsnet` | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-gnu` | — (Linux only) |
| `rssock` | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-gnu` | — (Linux only) |
| `rsfetch` | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-gnu` | — (Linux only) |

All release archives are verified against their `.sha256` checksums before extraction.

### Install via Cargo (From Source)

Requires Rust **1.85+** (edition 2024):

```sh
cargo install --git https://github.com/awaiden/rs-suite rsdu
cargo install --git https://github.com/awaiden/rs-suite rsfetch
cargo install --git https://github.com/awaiden/rs-suite rsfind
cargo install --git https://github.com/awaiden/rs-suite rsnet
cargo install --git https://github.com/awaiden/rs-suite rssock
cargo install --git https://github.com/awaiden/rs-suite rstop
```

### Post-Install: `rsnet` Live Packet Capture (Optional)

`rsnet` works out of the box in `PROCFS-ONLY` fallback mode. To enable live `AF_PACKET` per-packet capture without `sudo`:

```sh
sudo setcap cap_net_raw,cap_net_admin=eip ~/.local/bin/rsnet
```

## Build & Test

```bash
# Check, test, and lint the entire workspace
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets

# Build individual binaries
cargo build --release -p rsdu
cargo build --release -p rsfetch
cargo build --release -p rsfind
cargo build --release -p rsnet
cargo build --release -p rssock
cargo build --release -p rstop
```

## License

MIT, see [LICENSE](LICENSE).
