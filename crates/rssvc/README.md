# rssvc

Interactive systemd service manager and inspector with a split-pane TUI and scriptable CLI modes.

## Features

- **System & user units**: Inspect and manage system units or user units (`--user`).
- **Flexible filtering**: Filter by unit type (`--type`), active/sub state (`--state`), `--all` (including inactive units), and case-insensitive substring (`--filter` or interactive `/` in TUI).
- **Split-pane TUI**: Color-coded unit list (`active`, `failed`, `inactive`, transitioning) paired with live `systemctl status` and recent `journalctl` logs for the selected unit.
- **Safe service actions**: Start, stop, restart, reload, enable, and disable units behind a confirmation modal, with `--dry-run` support to preview exact `systemctl` commands without executing them.
- **CLI & JSON modes**: Print aligned tables (`--list` / `--no-tui`) or structured JSON (`--json`), with `--no-color` support and control-character sanitization.

## Requirements

- Linux with `systemd` (`systemctl` and `journalctl` in `PATH`)
- Rust toolchain (edition 2024, MSRV 1.85) — only needed if building from source

## Build from Source

```bash
git clone https://github.com/awaiden/rs-suite.git
cd rs-suite
cargo build --release -p rssvc
./target/release/rssvc
```

## Usage

```text
Usage: rssvc [OPTIONS]

Options:
      --user               Manage user systemd units (--user) instead of system units
  -a, --all                Show all loaded units, including inactive/dead ones
  -t, --type <UNIT_TYPE>   Filter units by type (e.g. service, socket, timer, target)
  -s, --state <STATE>      Filter units by active/sub state (e.g. active, failed, inactive, running)
  -f, --filter <FILTER>    Filter units by case-insensitive substring on name or description
      --list               Print unit table to stdout and exit instead of launching the TUI [aliases: --no-tui]
      --json               Emit JSON output and exit instead of launching the TUI
  -n, --dry-run            Preview mutating systemctl actions without executing them
      --no-color           Disable ANSI colors in CLI output and TUI
  -h, --help               Print help
  -V, --version            Print version
```

### TUI Keybindings

- `q` / `F10` / `Ctrl+C`: Quit
- `j` / `k` or `Up` / `Down`: Move selection or scroll focused pane
- `PageUp` / `PageDown`, `g` / `G`: Scroll by page or jump to top / bottom
- `Tab`: Cycle focus between Units, Status, and Journal Logs panes
- `/`: Enter interactive filter mode (`Enter` to confirm, `Esc` to clear)
- `a`: Toggle `--all` (include inactive units)
- `R` / `F5`: Refresh units, status, and logs
- `s`: Start selected unit (with confirmation modal)
- `x` / `S`: Stop selected unit (with confirmation modal)
- `r`: Restart selected unit (with confirmation modal)
- `l`: Reload selected unit (with confirmation modal)
- `e` / `d`: Enable / Disable selected unit (with confirmation modal)
- `?` / `F1`: Toggle help overlay
