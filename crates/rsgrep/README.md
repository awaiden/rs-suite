# rsgrep

Fast parallel content search with an interactive split-pane TUI, combining `ripgrep`-style `.gitignore`-aware directory traversal and context rendering with live interactive preview.

## Installation

From the workspace root:

```bash
cargo install --path crates/rsgrep
```

Or build in release mode:

```bash
cargo build --release -p rsgrep
```

## CLI Usage

By default, `rsgrep` searches the current directory, respects `.gitignore` and `.ignore` files, skips hidden files/directories and binary files (containing NUL bytes), and performs regex matching.

```bash
# Search for a regex pattern in the current directory
rsgrep 'fn\s+main'

# Fixed-string search with line numbers and smart-case
rsgrep -F -S -n 'Config::default()' src/

# Case-insensitive search with 2 lines of context
rsgrep -i -C 2 TODO

# Include hidden files and ignored files, filter by glob and depth
rsgrep --hidden --no-ignore -g '*.rs' -g '!target/**' --max-depth 3 needle

# List only matching file paths (-l) or match counts (-c)
rsgrep -l 'unwrap()'
rsgrep -c 'TODO'

# Emit newline-delimited JSON
rsgrep --json -n 'pattern' src/

# Disable ANSI colors
rsgrep --no-color 'pattern'
```

### Flags

| Flag | Description |
| :--- | :--- |
| `-F`, `--fixed-strings` | Treat pattern as a literal string |
| `-i`, `--ignore-case` | Case-insensitive matching |
| `-S`, `--smart-case` | Case-insensitive unless pattern has uppercase characters |
| `-n`, `--line-number` | Show 1-based line numbers |
| `-H`, `--with-filename` | Always print file paths |
| `--no-filename` | Never print file paths |
| `-A`, `--after-context <NUM>` | Lines of trailing context |
| `-B`, `--before-context <NUM>` | Lines of leading context |
| `-C`, `--context <NUM>` | Lines of leading and trailing context |
| `--hidden` | Search hidden files and directories |
| `--no-ignore` | Do not respect `.gitignore` / `.ignore` |
| `-g`, `--glob <GLOB>` | Include or exclude files (`!pattern` to exclude) |
| `--max-depth <NUM>` | Maximum directory traversal depth |
| `-l`, `--files-with-matches` | Print only matching file paths |
| `-c`, `--count` | Print only matching line counts per file |
| `--json` | Emit newline-delimited JSON |
| `--no-color` | Disable ANSI colors and TUI colors |
| `--interactive`, `--tui` | Launch interactive split-pane TUI |

## Interactive TUI (`--interactive` / `--tui`)

```bash
rsgrep --tui
rsgrep --interactive 'initial_query' src/
```

### Keybindings

| Key | Action |
| :--- | :--- |
| Typing / `Backspace` | Edit search query live |
| `Ctrl-U` | Clear search query |
| `Up` / `Down`, `Ctrl-P` / `Ctrl-N` | Move selection |
| `PageUp` / `PageDown`, `Home` / `End` | Jump through matches |
| `Enter` | Print selected `path:line` and exit (`0`) |
| `Esc` / `Ctrl-C` | Cancel and exit (`130`) |

## Exit Codes

- `0`: One or more matches found (or match selected in TUI)
- `1`: No matches found
- `2`: Invalid arguments, regex error, or I/O error
- `130`: Cancelled in interactive TUI
