# rsfind

Fast parallel file finder with an interactive fuzzy-finder TUI, combining `fd`-style directory traversal and filtering with `fzf`-style interactive selection.

## Installation

From the workspace root:

```bash
cargo install --path crates/rsfind
```

Or build in release mode:

```bash
cargo build --release -p rsfind
```

## CLI Usage (`fd`-style)

By default, `rsfind` searches the current directory, respects `.gitignore`, `.ignore`, and `.fdignore` files, skips hidden files/directories, and uses smart-case regex matching.

```bash
# Find all Rust files
rsfind -e rs

# Search for a regex pattern in a specific directory
rsfind 'mod\.rs$' src/

# Glob pattern matching, including hidden and ignored files
rsfind -H -I -g '*.toml'

# Filter by type, size, and modification time
rsfind -t f -S +10k --changed-within 2d

# Exclude directories and limit depth
rsfind -d 3 -E target -E node_modules

# Execute a command per match (-x) or in batch (-X)
rsfind -e rs -x wc -l {}
rsfind -e log -X rm
```

### Command Execution Placeholders (`-x` / `-X`)

| Placeholder | Expansion |
| :--- | :--- |
| `{}` | Full path |
| `{/}` | Basename |
| `{//}` | Parent directory |
| `{.}` | Path without extension |
| `{/.}` | Basename without extension |

If no placeholder is provided, matching paths are appended at the end of the command. Terminate `-x`/`-X` argument lists with `;` if followed by additional `rsfind` flags.

## Interactive TUI Usage (`fzf`-style)

`rsfind` launches the interactive TUI when:
- `-i` / `--interactive` is passed, **or**
- `stdin` is piped (not a TTY) and `--no-interactive`, `-x`, and `-X` are not set (reading candidate lines from `stdin`).

```bash
# Interactive file finder in the current directory
rsfind -i

# Multi-select with a custom preview command
rsfind -i -m --preview 'head -n 50 {}' --preview-window right:50%

# Filter piped input interactively
git ls-files | rsfind --query 'main'
```

Exit codes: `0` on success, `1` on error, `130` when aborted in TUI (`Esc` / `Ctrl-C`).

### Extended Search Syntax

Space-separated terms are combined with `AND`; `|` separates `OR` clauses. Escape literal spaces or pipes with `\ ` or `\|`.

| Syntax | Meaning |
| :--- | :--- |
| `foo` | Fuzzy match |
| `'foo` | Exact substring match |
| `^foo` | Prefix match |
| `foo$` | Suffix match |
| `^foo$` | Exact full-string match |
| `!foo` | Negated fuzzy match (also `!'foo`, `!^foo`, `!foo$`) |
| `a \| b` | Match `a` or `b` |

### TUI Keybindings

| Key | Action |
| :--- | :--- |
| `Up` / `Ctrl-K` / `Ctrl-P` | Move selection up |
| `Down` / `Ctrl-J` / `Ctrl-N` | Move selection down |
| `PageUp` / `PageDown` | Scroll by page |
| `Home` / `End` | Jump to top / bottom |
| `Tab` / `Shift-Tab` | Toggle selection and move down / up (with `-m`) |
| `Ctrl-A` | Select all (with `-m`) or move cursor to start of query |
| `Ctrl-D` | Deselect all |
| `Alt-P` | Toggle preview pane |
| `Left` / `Right` / `Ctrl-E` | Move query cursor left / right / end |
| `Backspace` / `Delete` | Delete character before / at cursor |
| `Ctrl-W` / `Ctrl-U` | Delete previous word / clear query |
| `Enter` | Confirm selection and print to stdout |
| `Esc` / `Ctrl-C` | Abort (exit code `130`) |

## Flags Reference

> **Note:** `-i` / `--interactive` launches the TUI (use `--ignore-case` for case-insensitive matching).

- **Mode:** `-i, --interactive`, `--no-interactive`, `-m, --multi`, `--preview <CMD>`, `--preview-window <SPEC>`, `--query <STR>`
- **Traversal:** `-H, --hidden`, `-I, --no-ignore`, `--no-ignore-vcs`, `-u, --unrestricted` (`-uu` also includes hidden), `-L, --follow`, `-d, --max-depth <N>`, `--min-depth <N>`, `--one-file-system`, `-E, --exclude <GLOB>`, `-j, --threads <N>`, `--search-path <PATH>`
- **Filtering:** `-g, --glob`, `-F, --fixed-strings`, `-p, --full-path`, `-s, --case-sensitive`, `--ignore-case`, `-t, --type <TYPE>` (`f`, `d`, `l`, `x`, `e`, `s`, `p`), `-e, --extension <EXT>`, `-S, --size <SIZE>`, `--changed-within <DUR>`, `--changed-before <DUR>`
- **Output & Exec:** `-a, --absolute-path`, `-0, --print0`, `-l, --list-details`, `-c, --color <WHEN>`, `--no-color`, `-1, --max-results <N>`, `-x, --exec <CMD...>`, `-X, --exec-batch <CMD...>`
