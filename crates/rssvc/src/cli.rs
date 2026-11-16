use clap::Parser;

/// Interactive systemd service manager and inspector with TUI.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "rssvc",
    version,
    about = "Interactive systemd service manager and inspector with TUI"
)]
pub struct Args {
    /// Manage user systemd units (--user) instead of system units
    #[arg(long)]
    pub user: bool,

    /// Show all loaded units, including inactive/dead ones
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Filter units by type (e.g. service, socket, timer, target)
    #[arg(short = 't', long = "type")]
    pub unit_type: Option<String>,

    /// Filter units by active/sub state (e.g. active, failed, inactive, running)
    #[arg(short = 's', long)]
    pub state: Option<String>,

    /// Filter units by case-insensitive substring on name or description
    #[arg(short = 'f', long)]
    pub filter: Option<String>,

    /// Print unit table to stdout and exit instead of launching the TUI
    #[arg(long, visible_alias = "no-tui")]
    pub list: bool,

    /// Emit JSON output and exit instead of launching the TUI
    #[arg(long)]
    pub json: bool,

    /// Preview mutating systemctl actions without executing them
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Disable ANSI colors in CLI output and TUI
    #[arg(long)]
    pub no_color: bool,
}

impl Args {
    /// Returns `true` when non-interactive CLI output was requested.
    pub fn is_cli_mode(&self) -> bool {
        self.list || self.json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_flags() {
        let args = Args::try_parse_from([
            "rssvc",
            "--user",
            "--all",
            "--type",
            "service",
            "--state",
            "failed",
            "--filter",
            "ssh",
            "--list",
            "--json",
            "--dry-run",
            "--no-color",
        ])
        .expect("should parse all flags");

        assert!(args.user);
        assert!(args.all);
        assert_eq!(args.unit_type.as_deref(), Some("service"));
        assert_eq!(args.state.as_deref(), Some("failed"));
        assert_eq!(args.filter.as_deref(), Some("ssh"));
        assert!(args.list);
        assert!(args.json);
        assert!(args.dry_run);
        assert!(args.no_color);
        assert!(args.is_cli_mode());
    }

    #[test]
    fn no_tui_alias_sets_list_mode() {
        let args = Args::try_parse_from(["rssvc", "--no-tui"]).expect("should parse --no-tui");
        assert!(args.list);
        assert!(args.is_cli_mode());
    }

    #[test]
    fn defaults_to_tui_mode() {
        let args = Args::try_parse_from(["rssvc"]).expect("should parse empty args");
        assert!(!args.user);
        assert!(!args.all);
        assert!(!args.list);
        assert!(!args.json);
        assert!(!args.dry_run);
        assert!(!args.no_color);
        assert!(!args.is_cli_mode());
    }
}
