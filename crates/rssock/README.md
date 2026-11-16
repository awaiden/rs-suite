# rssock

Interactive Linux socket and connection explorer written in Rust, designed as a modern `ss` replacement.

## Features

- **TCP, UDP & UNIX domain sockets**: Parses `/proc/net/{tcp,tcp6,udp,udp6,unix}` directly without spawning external tools.
- **Process attribution**: Maps socket inodes to owning processes (`/proc/<pid>/fd/*`, `comm`, `cmdline`, `uid`) with `-p` or automatically in `--tui` mode.
- **Flexible filtering & sorting**: Filter by protocol (`-t`, `-u`, `-x`), listen mode (`-l`, `-a`), socket state (`--state`), or substring query (`-f`), and sort by any column (`--sort`, `-r`).
- **Multiple output modes**: Aligned `ss`-style table, `ss -s`-style summary (`-s`), structured JSON (`--json`), and an interactive `ratatui` TUI (`--tui`).
- **Control-character sanitization**: Safely sanitizes untrusted process names, command lines, and UNIX socket paths.

## Requirements

- Linux (reads `/proc/net/{tcp,tcp6,udp,udp6,unix}` and `/proc/<pid>/fd/*`)
- Rust toolchain (edition 2024, MSRV 1.85) — only needed if building from source

## Build from Source

```bash
git clone https://github.com/awaiden/rs-suite.git
cd rs-suite
cargo build --release -p rssock
./target/release/rssock --tui
```

## Usage

```text
Usage: rssock [OPTIONS]

Options:
  -t, --tcp              Show TCP sockets
  -u, --udp              Show UDP sockets
  -x, --unix             Show UNIX domain sockets
  -l, --listening        Show only listening sockets
  -a, --all              Show both listening and non-listening sockets
  -n, --numeric          Do not resolve service names
  -p, --processes        Show process using socket
  -s, --summary          Print socket summary statistics and exit
      --state <STATE>    Filter by socket state (e.g. estab, listen, time-wait, unconn)
      --sort <COLUMN>    Sort column [default: proto]
                         [possible values: proto, state, recv-q, send-q, local, remote, pid, process]
  -r, --reverse          Reverse sort order
  -f, --filter <QUERY>   Substring filter on address, port, path, state, or process
      --json             Emit JSON output
      --no-color         Disable color output
      --tui              Launch interactive TUI mode
  -d, --delay <MS>       TUI refresh interval in milliseconds (100..=60000) [default: 1000]
  -h, --help             Print help
  -V, --version          Print version
```

### TUI Keybindings

- `q` / `F10` / `Ctrl+C`: Quit
- `j` / `k` or `Up` / `Down`: Move selection
- `PageUp` / `PageDown`: Scroll by page
- `g` / `G`: Jump to top / bottom
- `t` / `u` / `x`: Toggle TCP / UDP / UNIX sockets
- `l` / `a`: Toggle Listening / All / Non-Listening
- `s`: Cycle sort column (`Proto` / `State` / `RecvQ` / `SendQ` / `Local` / `Remote` / `Pid` / `Process`)
- `r`: Reverse sort order
- `n`: Toggle numeric port display
- `/`: Enter substring filter mode (`Enter` / `Esc` to confirm, `Esc` in Normal mode to clear)
- `Enter`: Toggle socket detail popup (shows full cmdline, UID, inode, flags, UNIX path)
- `Space` / `p`: Pause or resume refresh
- `?` / `F1` / `Esc`: Toggle or close the help overlay
