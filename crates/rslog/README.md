# rslog

Interactive Linux log and systemd `journald` viewer with live regex/level filtering, file tailing, and an `lnav`-style TUI.

## Installation

From the workspace root:

```bash
cargo install --path crates/rslog
```

Or build in release mode:

```bash
cargo build --release -p rslog
```

## CLI Usage

By default, `rslog` opens the interactive TUI (or emits plain/colored CLI output when stdout is piped or `--cli` / `--no-tui` / `--json` is passed). If no file paths are provided, it reads from `journalctl -o json`.

```bash
# View and tail a log file in the interactive TUI
rslog -f /var/log/syslog

# View systemd journald entries for specific units since 1 hour ago
rslog -j -u nginx -u sshd --since "1 hour ago"

# Non-interactive CLI output filtered to WARN+ and matching a regex
rslog --cli -l warn -g "timeout|refused" /var/log/app.log

# Emit newline-delimited JSON
rslog --json -n 100 /var/log/app.log

# Disable ANSI colors and TUI colors
rslog --no-color /var/log/app.log
```

### Flags

| Flag | Description |
| :--- | :--- |
| `[FILES]...` | Log file paths to view or tail (defaults to journald if omitted) |
| `-j`, `--journal` | Read logs from systemd journald via `journalctl` |
| `-u`, `--unit <UNIT>` | Filter systemd journal by unit name (repeatable) |
| `-f`, `--follow` | Follow new log entries as they arrive |
| `-n`, `--lines <NUM>` | Number of recent lines to load (default: all for files, 200 for journal) |
| `-g`, `--grep <PATTERN>` | Filter log entries by regular expression |
| `-l`, `--level <LEVEL>` | Minimum log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `--since <TIME>` | Show entries on or after timestamp (passed to `journalctl`) |
| `--until <TIME>` | Show entries on or before timestamp (passed to `journalctl`) |
| `--cli`, `--no-tui` | Run in non-interactive CLI mode |
| `--json` | Emit newline-delimited JSON in CLI mode |
| `--no-color` | Disable ANSI colors and TUI colors |

## Interactive TUI Keybindings

| Key | Action |
| :--- | :--- |
| `j` / `Down`, `k` / `Up` | Move selection down / up |
| `PageDown`, `PageUp` | Scroll by 20 lines |
| `g` / `Home`, `G` / `End` | Jump to top / bottom |
| `/` | Enter live regex filter mode (`Enter` / `Esc` to finish) |
| `1` - `5` | Set minimum level (`1`=Trace, `2`=Debug, `3`=Info, `4`=Warn, `5`=Error) |
| `L` | Cycle minimum level filter (`ALL -> TRACE -> DEBUG -> INFO -> WARN -> ERROR`) |
| `f` | Toggle follow mode |
| `Enter` | Toggle entry detail pane |
| `c` | Clear active regex and level filters |
| `q` / `Esc` / `Ctrl-C` | Quit |
