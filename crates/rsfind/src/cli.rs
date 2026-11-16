use std::path::PathBuf;

use clap::{ArgAction, Parser, ValueEnum};

/// Color output mode.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorWhen {
    #[default]
    Auto,
    Always,
    Never,
}

/// Fast parallel file finder with an interactive fuzzy TUI, combining fd and fzf.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "rsfind",
    version,
    about = "Fast parallel file finder with interactive fuzzy TUI",
    after_help = "Note: -i/--interactive launches the TUI; use --ignore-case for case-insensitive matching."
)]
pub struct Args {
    /// Search pattern (regex by default, or glob/fixed-string with -g/-F).
    #[arg(value_name = "PATTERN")]
    pub pattern: Option<String>,

    /// Root directories to search (defaults to current directory).
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Additional root directories to search.
    #[arg(long = "search-path", value_name = "PATH")]
    pub search_path: Vec<PathBuf>,

    // --- Mode flags ---
    /// Launch interactive fuzzy-finder TUI.
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Force non-interactive CLI output even when stdout is a TTY.
    #[arg(long = "no-interactive", conflicts_with = "interactive")]
    pub no_interactive: bool,

    /// Enable multi-select mode in the interactive TUI.
    #[arg(short = 'm', long = "multi")]
    pub multi: bool,

    /// Preview command for the interactive TUI (supports `{}` placeholder).
    #[arg(long = "preview", value_name = "CMD")]
    pub preview: Option<String>,

    /// Preview window layout specification (e.g. "right:50%", "down:40%:hidden").
    #[arg(long = "preview-window", value_name = "SPEC")]
    pub preview_window: Option<String>,

    /// Initial query string for the interactive TUI.
    #[arg(long = "query", value_name = "STR")]
    pub query: Option<String>,

    // --- Traversal flags ---
    /// Include hidden files and directories.
    #[arg(short = 'H', long = "hidden")]
    pub hidden: bool,

    /// Do not respect .gitignore, .ignore, or .fdignore files.
    #[arg(short = 'I', long = "no-ignore")]
    pub no_ignore: bool,

    /// Do not respect .gitignore files.
    #[arg(long = "no-ignore-vcs")]
    pub no_ignore_vcs: bool,

    /// Unrestricted search (-u: --no-ignore, -uu: --no-ignore --hidden).
    #[arg(short = 'u', long = "unrestricted", action = ArgAction::Count)]
    pub unrestricted: u8,

    /// Follow symbolic links.
    #[arg(short = 'L', long = "follow")]
    pub follow: bool,

    /// Maximum directory traversal depth.
    #[arg(short = 'd', long = "max-depth", value_name = "N")]
    pub max_depth: Option<usize>,

    /// Minimum directory traversal depth.
    #[arg(long = "min-depth", value_name = "N")]
    pub min_depth: Option<usize>,

    /// Do not cross filesystem boundaries.
    #[arg(long = "one-file-system")]
    pub one_file_system: bool,

    /// Exclude entries matching the given glob pattern.
    #[arg(short = 'E', long = "exclude", value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Number of threads to use for traversal and filtering.
    #[arg(short = 'j', long = "threads", value_name = "N")]
    pub threads: Option<usize>,

    // --- Filter flags ---
    /// Treat pattern as a glob instead of a regular expression.
    #[arg(short = 'g', long = "glob")]
    pub glob: bool,

    /// Treat pattern as a literal fixed string instead of a regular expression.
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,

    /// Match pattern against the full path instead of just the file name.
    #[arg(short = 'p', long = "full-path")]
    pub full_path: bool,

    /// Force case-sensitive pattern matching (overrides smart-case).
    #[arg(short = 's', long = "case-sensitive")]
    pub case_sensitive: bool,

    /// Force case-insensitive pattern matching (overrides smart-case).
    #[arg(long = "ignore-case")]
    pub ignore_case: bool,

    /// Filter by file type (f/file, d/directory, l/symlink, x/executable, e/empty, s/socket, p/pipe).
    #[arg(short = 't', long = "type", value_name = "TYPE")]
    pub file_type: Vec<String>,

    /// Filter by file extension (without leading dot).
    #[arg(short = 'e', long = "extension", value_name = "EXT")]
    pub extension: Vec<String>,

    /// Filter by file size (e.g. +10M, -1k, 100B, +1Gi).
    #[arg(
        short = 'S',
        long = "size",
        value_name = "SIZE",
        allow_hyphen_values = true
    )]
    pub size: Vec<String>,

    /// Filter for files modified within the given duration or since date (e.g. 10m, 2d, 2025-01-01).
    #[arg(long = "changed-within", value_name = "DUR")]
    pub changed_within: Option<String>,

    /// Filter for files modified before the given duration or date.
    #[arg(long = "changed-before", value_name = "DUR")]
    pub changed_before: Option<String>,

    // --- Output & exec flags ---
    /// Print absolute paths instead of relative paths.
    #[arg(short = 'a', long = "absolute-path")]
    pub absolute_path: bool,

    /// Separate output paths with NUL bytes instead of newlines.
    #[arg(short = '0', long = "print0")]
    pub print0: bool,

    /// Show detailed file metadata (permissions, size, mtime).
    #[arg(short = 'l', long = "list-details")]
    pub list_details: bool,

    /// When to use colored output.
    #[arg(short = 'c', long = "color", value_enum, default_value_t = ColorWhen::Auto, value_name = "WHEN")]
    pub color: ColorWhen,

    /// Disable colored output (equivalent to --color=never).
    #[arg(long = "no-color")]
    pub no_color: bool,

    /// Stop after printing at most N results.
    #[arg(short = '1', long = "max-results", value_name = "N")]
    pub max_results: Option<usize>,

    /// Execute a command for each search result (use `;` to terminate if followed by more args).
    #[arg(
        short = 'x',
        long = "exec",
        num_args = 1..,
        value_terminator = ";",
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub exec: Vec<String>,

    /// Execute a command once with all search results as arguments.
    #[arg(
        short = 'X',
        long = "exec-batch",
        num_args = 1..,
        value_terminator = ";",
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub exec_batch: Vec<String>,
}

impl Args {
    /// Return effective search roots, defaulting to `["."]` when none are given.
    pub fn effective_paths(&self) -> Vec<PathBuf> {
        let mut roots = Vec::with_capacity(self.paths.len() + self.search_path.len());
        roots.extend(self.paths.iter().cloned());
        roots.extend(self.search_path.iter().cloned());
        if roots.is_empty() {
            roots.push(PathBuf::from("."));
        }
        roots
    }

    /// Whether hidden files should be included (via `-H` or `-uu`).
    pub fn include_hidden(&self) -> bool {
        self.hidden || self.unrestricted >= 2
    }

    /// Whether ignore files should be bypassed (via `-I` or `-u`).
    pub fn ignore_disabled(&self) -> bool {
        self.no_ignore || self.unrestricted >= 1
    }

    /// Effective color mode taking `--no-color` into account.
    pub fn effective_color(&self) -> ColorWhen {
        if self.no_color {
            ColorWhen::Never
        } else {
            self.color
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_interactive_and_ignore_case_disambiguation() {
        let args = Args::try_parse_from(["rsfind", "-i", "--ignore-case", "foo"]).unwrap();
        assert!(args.interactive);
        assert!(args.ignore_case);
        assert_eq!(args.pattern.as_deref(), Some("foo"));
    }

    #[test]
    fn parses_negative_size_filter() {
        let args = Args::try_parse_from(["rsfind", "-S", "-1k", "-S", "+10B"]).unwrap();
        assert_eq!(args.size, vec!["-1k", "+10B"]);
    }

    #[test]
    fn unrestricted_count_and_effective_paths() {
        let args = Args::try_parse_from(["rsfind", "-uu"]).unwrap();
        assert!(args.ignore_disabled());
        assert!(args.include_hidden());
        assert_eq!(args.effective_paths(), vec![PathBuf::from(".")]);

        let args2 =
            Args::try_parse_from(["rsfind", "pat", "src", "--search-path", "tests"]).unwrap();
        assert_eq!(
            args2.effective_paths(),
            vec![PathBuf::from("src"), PathBuf::from("tests")]
        );
    }

    #[test]
    fn parses_exec_with_hyphen_flags() {
        let args = Args::try_parse_from(["rsfind", "-x", "ls", "-la", "{}"]).unwrap();
        assert_eq!(args.exec, vec!["ls", "-la", "{}"]);
    }
}
