# rs-suite

A Cargo workspace monorepo for fast Rust CLI and TUI system utilities.

## Workspace Crates

- **[`rsdu`](crates/rsdu/README.md)** — Fast disk usage analyzer (`rsdu` binary). See [crates/rsdu/README.md](crates/rsdu/README.md).
- **[`rsnet`](crates/rsnet/README.md)** — Interactive Linux network bandwidth and connection-per-process monitor (`rsnet` binary). See [crates/rsnet/README.md](crates/rsnet/README.md).
- **[`rstop`](crates/rstop/README.md)** — Interactive system and process monitor (`rstop` binary). See [crates/rstop/README.md](crates/rstop/README.md).
- **[`rs-common`](crates/rs-common/)** — Shared internal library for size/rate/duration formatting, TUI color palettes & style helpers, and `/proc` parsing utilities.

## Installation

### Quick Install (Prebuilt Binaries)

Install all tools supported on your platform (`rsdu`, `rstop`, `rsnet` on Linux; `rsdu` on macOS arm64) into `~/.local/bin`:

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
- `RSDU_VERSION` / `RSTOP_VERSION` / `RSNET_VERSION`: Pin a specific release tag per tool

#### Per-Tool Installers

Each binary also has its own standalone installer:

```sh
# rsdu (Linux x86_64/aarch64, macOS arm64)
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/crates/rsdu/install.sh | sh

# rstop (Linux x86_64/aarch64)
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/crates/rstop/install.sh | sh

# rsnet (Linux x86_64/aarch64)
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/crates/rsnet/install.sh | sh
```

### Supported Prebuilt Targets

| Tool | Linux `x86_64` | Linux `aarch64` | macOS `arm64` |
|---|---|---|---|
| `rsdu` | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` | `aarch64-apple-darwin` |
| `rstop` | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-gnu` | — (Linux only) |
| `rsnet` | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-gnu` | — (Linux only) |

All release archives are verified against their `.sha256` checksums before extraction.

### Install via Cargo (From Source)

Requires Rust **1.85+** (edition 2024):

```sh
cargo install --git https://github.com/awaiden/rs-suite rsdu
cargo install --git https://github.com/awaiden/rs-suite rstop
cargo install --git https://github.com/awaiden/rs-suite rsnet
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
cargo build --release -p rsnet
cargo build --release -p rstop
```

## License

MIT, see [LICENSE](LICENSE).
