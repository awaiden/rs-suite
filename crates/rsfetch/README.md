# rsfetch

Fast native Linux system information fetcher written in Rust, inspired by `fastfetch` and `neofetch`.

## Features

- **Native Linux detection**: Reads `/proc`, `/sys`, `/etc/os-release`, `uname`, `statvfs`, and `getifaddrs` directly in Rust without spawning slow subprocesses where native interfaces exist, with graceful fallbacks on missing files or non-Linux hosts.
- **Parallel collection**: Executes independent system modules concurrently with `rayon` for low-latency startup.
- **Built-in distro logos**: Includes standard and compact (`--logo-type small`) ASCII art logos with automatic `ID` / `ID_LIKE` resolution, custom overrides (`-l` / `--logo`), and `--no-logo`.
- **Terminal-safe rendering**: Sanitizes OS, hardware, and environment strings against control-character injection, and honors `--no-color` and `NO_COLOR`.
- **Flexible output**: Renders side-by-side logo + info text output by default or structured JSON via `--json` (`-j`).

## Supported Modules

Default modules (in display order):
- `os`, `host`, `kernel`, `uptime`, `packages`, `shell`, `resolution`, `de`, `wm`, `terminal`, `cpu`, `gpu`, `memory`, `disk`, `colors`

Optional modules (enabled with `--all` or `--modules`):
- `local-ip`, `battery`

Run `rsfetch --list-modules` to print all supported module names.

## Supported Logos

`linux`, `ubuntu`, `debian`, `arch`, `fedora`, `nixos`, `alpine`, `manjaro`, `endeavouros`, `void`, `gentoo`, `opensuse`, `pop`, `mint`, `kali`, `rocky`, `alma`, `cachyos`, `macos`, `windows`, `unknown` (plus `none` to disable).

Run `rsfetch --list-logos` to print all supported canonical logo names.

## Requirements

- Linux (reads `/proc`, `/sys`, `/etc/os-release`, `uname`, `statvfs`, and `getifaddrs`; non-Linux hosts use graceful fallbacks)
- Rust toolchain (edition 2024, MSRV 1.85) — only needed if building from source

## Install

### Quick Install (Linux x86_64 / aarch64)

```sh
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/install.sh | sh -s -- rsfetch
```

Override the install directory with `INSTALL_DIR=/usr/local/bin` or pin a release with `RSFETCH_VERSION=rsfetch-v0.1.0`.

### Build from Source

```bash
git clone https://github.com/awaiden/rs-suite.git
cd rs-suite
cargo build --release -p rsfetch
./target/release/rsfetch
```

## Usage

```text
Usage: rsfetch [OPTIONS]

Options:
  -j, --json                  Output system information as JSON
      --no-color              Disable ANSI color output
  -l, --logo <DISTRO>         Override detected distro ASCII logo (or "none")
      --no-logo               Hide the ASCII logo completely
      --logo-type <TYPE>      Logo variant to render [default: auto]
                              [possible values: auto, small, none]
  -m, --modules <LIST>        Comma-separated module names to display (preserves order)
      --hide <LIST>           Comma-separated module names to exclude
  -a, --all                   Enable all modules (including optional ones like battery and local-ip)
      --list-logos            Print supported distro logo names and exit
      --list-modules          Print supported module names and exit
  -h, --help                  Print help (see a summary with '-h')
  -V, --version               Print version
```

### Examples

```bash
# Default system summary with auto-detected distro logo
rsfetch

# Include all modules (including local-ip and battery)
rsfetch -a

# Show only specific modules in order
rsfetch -m os,kernel,cpu,memory,disk

# Use a specific distro logo in compact small mode
rsfetch -l arch --logo-type small

# Emit JSON output without ANSI colors
rsfetch --json
```
