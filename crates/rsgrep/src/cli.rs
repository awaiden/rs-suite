use std::path::PathBuf;

use clap::Parser;

/// Fast parallel content search with interactive TUI.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "rsgrep",
    version,
    about = "Fast parallel content search with interactive TUI"
)]
pub struct Args {
    /// Search pattern (regex by default, or literal with -F/--fixed-strings).
    pub pattern: Option<String>,

    /// Files or directories to search (defaults to current directory).
    pub paths: Vec<PathBuf>,

    /// Treat the pattern as a literal string instead of a regular expression.
    #[arg(short = 'F', long)]
    pub fixed_strings: bool,

    /// Case-insensitive search.
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Search case-insensitively if the pattern contains no uppercase characters.
    #[arg(short = 'S', long)]
    pub smart_case: bool,

    /// Show line numbers (1-based).
    #[arg(short = 'n', long)]
    pub line_number: bool,

    /// Always print the file path with output lines.
    #[arg(short = 'H', long, overrides_with = "no_filename")]
    pub with_filename: bool,

    /// Never print the file path with output lines.
    #[arg(long, overrides_with = "with_filename")]
    pub no_filename: bool,

    /// Print NUM lines of trailing context after each match.
    #[arg(short = 'A', long = "after-context", value_name = "NUM")]
    pub after_context: Option<usize>,

    /// Print NUM lines of leading context before each match.
    #[arg(short = 'B', long = "before-context", value_name = "NUM")]
    pub before_context: Option<usize>,

    /// Print NUM lines of leading and trailing context around each match.
    #[arg(short = 'C', long = "context", value_name = "NUM")]
    pub context: Option<usize>,

    /// Search hidden files and directories.
    #[arg(long)]
    pub hidden: bool,

    /// Do not respect .gitignore, .ignore, or other ignore files.
    #[arg(long)]
    pub no_ignore: bool,

    /// Include or exclude files matching the given glob pattern (prefix with `!` to exclude).
    #[arg(short = 'g', long = "glob", value_name = "GLOB")]
    pub globs: Vec<String>,

    /// Maximum directory traversal depth.
    #[arg(long, value_name = "NUM")]
    pub max_depth: Option<usize>,

    /// Only print paths of files with at least one match.
    #[arg(short = 'l', long)]
    pub files_with_matches: bool,

    /// Only print a count of matching lines per file.
    #[arg(short = 'c', long)]
    pub count: bool,

    /// Emit results as newline-delimited JSON.
    #[arg(long)]
    pub json: bool,

    /// Disable ANSI color output.
    #[arg(long)]
    pub no_color: bool,

    /// Launch interactive split-pane TUI.
    #[arg(long, visible_alias = "tui")]
    pub interactive: bool,
}

impl Args {
    /// Validates flag combinations and required arguments for the selected mode.
    pub fn validate(&self) -> Result<(), String> {
        if !self.interactive && self.pattern.is_none() {
            return Err("a search pattern is required unless --interactive/--tui is used".into());
        }
        if self.files_with_matches && self.count {
            return Err("cannot combine -l/--files-with-matches and -c/--count".into());
        }
        Ok(())
    }

    /// Returns the search roots, defaulting to `.` when no paths were specified.
    pub fn effective_paths(&self) -> Vec<PathBuf> {
        if self.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            self.paths.clone()
        }
    }

    /// Returns the number of leading context lines (`-B` overrides `-C`).
    pub fn before_lines(&self) -> usize {
        self.before_context.or(self.context).unwrap_or(0)
    }

    /// Returns the number of trailing context lines (`-A` overrides `-C`).
    pub fn after_lines(&self) -> usize {
        self.after_context.or(self.context).unwrap_or(0)
    }

    /// Determines whether filenames should be printed for the given search paths.
    pub fn should_show_filename(&self) -> bool {
        if self.with_filename {
            return true;
        }
        if self.no_filename {
            return false;
        }
        let paths = self.effective_paths();
        paths.len() > 1 || paths.first().is_some_and(|p| p.is_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_required_flags() {
        let args = Args::try_parse_from([
            "rsgrep",
            "-F",
            "-i",
            "-S",
            "-n",
            "-H",
            "-A",
            "2",
            "-B",
            "3",
            "-C",
            "1",
            "--hidden",
            "--no-ignore",
            "-g",
            "*.rs",
            "-g",
            "!target/**",
            "--max-depth",
            "4",
            "--json",
            "--no-color",
            "needle",
            "src",
        ])
        .expect("should parse flags");

        assert_eq!(args.pattern.as_deref(), Some("needle"));
        assert_eq!(args.paths, vec![PathBuf::from("src")]);
        assert!(args.fixed_strings);
        assert!(args.ignore_case);
        assert!(args.smart_case);
        assert!(args.line_number);
        assert!(args.with_filename);
        assert_eq!(args.after_lines(), 2);
        assert_eq!(args.before_lines(), 3);
        assert!(args.hidden);
        assert!(args.no_ignore);
        assert_eq!(args.globs, vec!["*.rs", "!target/**"]);
        assert_eq!(args.max_depth, Some(4));
        assert!(args.json);
        assert!(args.no_color);
    }

    #[test]
    fn context_flag_sets_both_before_and_after() {
        let args = Args::try_parse_from(["rsgrep", "-C", "4", "foo"]).unwrap();
        assert_eq!(args.before_lines(), 4);
        assert_eq!(args.after_lines(), 4);
    }

    #[test]
    fn tui_alias_and_empty_pattern_allowed() {
        let args = Args::try_parse_from(["rsgrep", "--tui"]).unwrap();
        assert!(args.interactive);
        assert!(args.validate().is_ok());
    }

    #[test]
    fn cli_mode_requires_pattern() {
        let args = Args::try_parse_from(["rsgrep"]).unwrap();
        assert!(args.validate().is_err());
    }
}
