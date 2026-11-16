use std::path::PathBuf;

use clap::Parser;
use regex::Regex;

use crate::model::LogLevel;

/// Interactive Linux log and journald viewer with filtering and TUI.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "rslog",
    version,
    about = "Interactive Linux log and systemd journald viewer with filtering and TUI"
)]
pub struct Args {
    /// Log file paths to view or tail (defaults to systemd journal if omitted)
    pub files: Vec<PathBuf>,

    /// Read logs from systemd journald via journalctl
    #[arg(short = 'j', long)]
    pub journal: bool,

    /// Filter systemd journal by unit name (can be repeated)
    #[arg(short = 'u', long = "unit")]
    pub units: Vec<String>,

    /// Follow log output as new entries arrive
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// Number of recent lines to load (default: all for files, 200 for journal)
    #[arg(short = 'n', long)]
    pub lines: Option<usize>,

    /// Filter log entries by regular expression
    #[arg(short = 'g', long)]
    pub grep: Option<String>,

    /// Minimum log level to display (trace, debug, info, warn, error)
    #[arg(short = 'l', long, value_enum)]
    pub level: Option<LogLevel>,

    /// Show entries on or after the specified timestamp (passed to journalctl)
    #[arg(long)]
    pub since: Option<String>,

    /// Show entries on or before the specified timestamp (passed to journalctl)
    #[arg(long)]
    pub until: Option<String>,

    /// Run in non-interactive CLI mode instead of launching the TUI
    #[arg(long = "cli", visible_alias = "no-tui")]
    pub cli: bool,

    /// Emit newline-delimited JSON in CLI mode
    #[arg(long)]
    pub json: bool,

    /// Disable ANSI colors in CLI output and TUI
    #[arg(long)]
    pub no_color: bool,
}

impl Args {
    /// Returns `true` if journald is explicitly requested or no positional files were supplied.
    pub fn should_read_journal(&self) -> bool {
        self.journal || self.files.is_empty()
    }

    /// Validates CLI arguments (for example, ensuring `--grep` is a valid regex).
    pub fn validate(&self) -> Result<(), String> {
        if let Some(pattern) = &self.grep {
            Regex::new(pattern).map_err(|e| format!("invalid --grep regex '{pattern}': {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_flags_and_positional_files() {
        let args = Args::try_parse_from([
            "rslog",
            "-f",
            "-n",
            "50",
            "-g",
            "error|warn",
            "-l",
            "warn",
            "--since",
            "1 hour ago",
            "--until",
            "now",
            "--no-tui",
            "--json",
            "--no-color",
            "/var/log/syslog",
            "/var/log/auth.log",
        ])
        .expect("cli parse");

        assert!(args.follow);
        assert_eq!(args.lines, Some(50));
        assert_eq!(args.grep.as_deref(), Some("error|warn"));
        assert_eq!(args.level, Some(LogLevel::Warn));
        assert_eq!(args.since.as_deref(), Some("1 hour ago"));
        assert_eq!(args.until.as_deref(), Some("now"));
        assert!(args.cli);
        assert!(args.json);
        assert!(args.no_color);
        assert_eq!(args.files.len(), 2);
        assert!(!args.should_read_journal());
        assert!(args.validate().is_ok());
    }

    #[test]
    fn parses_journal_and_units() {
        let args = Args::try_parse_from(["rslog", "-j", "-u", "nginx", "-u", "sshd"])
            .expect("journal parse");
        assert!(args.journal);
        assert_eq!(args.units, vec!["nginx", "sshd"]);
        assert!(args.should_read_journal());
    }

    #[test]
    fn rejects_invalid_grep_regex() {
        let args = Args::try_parse_from(["rslog", "-g", "[unclosed"]).expect("cli parse");
        assert!(args.validate().is_err());
    }
}
