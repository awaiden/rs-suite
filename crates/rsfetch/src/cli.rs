use clap::{Parser, ValueEnum};

/// Logo rendering mode.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogoType {
    /// Automatically choose the standard or small logo.
    #[default]
    Auto,
    /// Use the compact small logo variant.
    Small,
    /// Do not render any logo.
    None,
}

/// Command-line arguments for `rsfetch`.
#[derive(Parser, Debug, Clone, PartialEq, Eq)]
#[command(
    name = "rsfetch",
    version,
    about = "Fast native Linux system information fetcher"
)]
pub struct CliArgs {
    /// Output system information as JSON.
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Disable ANSI color output.
    #[arg(long)]
    pub no_color: bool,

    /// Override detected distro ASCII logo (or "none").
    #[arg(short = 'l', long, value_name = "DISTRO")]
    pub logo: Option<String>,

    /// Hide the ASCII logo completely.
    #[arg(long)]
    pub no_logo: bool,

    /// Logo variant to render.
    #[arg(long, value_enum, default_value_t = LogoType::Auto, value_name = "TYPE")]
    pub logo_type: LogoType,

    /// Comma-separated module names to display (preserves order).
    #[arg(short = 'm', long, value_delimiter = ',', value_name = "LIST")]
    pub modules: Vec<String>,

    /// Comma-separated module names to exclude.
    #[arg(long, value_delimiter = ',', value_name = "LIST")]
    pub hide: Vec<String>,

    /// Enable all modules (including optional ones like battery and local-ip).
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Print supported distro logo names and exit.
    #[arg(long)]
    pub list_logos: bool,

    /// Print supported module names and exit.
    #[arg(long)]
    pub list_modules: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_expected() {
        let args = CliArgs::try_parse_from(["rsfetch"]).expect("default args should parse");
        assert!(!args.json);
        assert!(!args.no_color);
        assert_eq!(args.logo, None);
        assert!(!args.no_logo);
        assert_eq!(args.logo_type, LogoType::Auto);
        assert!(args.modules.is_empty());
        assert!(args.hide.is_empty());
        assert!(!args.all);
        assert!(!args.list_logos);
        assert!(!args.list_modules);
    }

    #[test]
    fn short_flags_parse() {
        let args =
            CliArgs::try_parse_from(["rsfetch", "-j", "-l", "arch", "-m", "os,kernel,cpu", "-a"])
                .expect("short flags should parse");
        assert!(args.json);
        assert_eq!(args.logo.as_deref(), Some("arch"));
        assert_eq!(args.modules, vec!["os", "kernel", "cpu"]);
        assert!(args.all);
    }

    #[test]
    fn long_flags_and_hide_list_parse() {
        let args = CliArgs::try_parse_from([
            "rsfetch",
            "--json",
            "--no-color",
            "--logo",
            "none",
            "--no-logo",
            "--logo-type",
            "small",
            "--modules",
            "uptime,memory,disk",
            "--hide",
            "gpu,shell",
            "--all",
            "--list-logos",
            "--list-modules",
        ])
        .expect("long flags should parse");
        assert!(args.json);
        assert!(args.no_color);
        assert_eq!(args.logo.as_deref(), Some("none"));
        assert!(args.no_logo);
        assert_eq!(args.logo_type, LogoType::Small);
        assert_eq!(args.modules, vec!["uptime", "memory", "disk"]);
        assert_eq!(args.hide, vec!["gpu", "shell"]);
        assert!(args.all);
        assert!(args.list_logos);
        assert!(args.list_modules);
    }

    #[test]
    fn logo_type_none_parses() {
        let args = CliArgs::try_parse_from(["rsfetch", "--logo-type", "none"])
            .expect("logo-type none should parse");
        assert_eq!(args.logo_type, LogoType::None);
    }

    #[test]
    fn invalid_logo_type_is_rejected() {
        let err = CliArgs::try_parse_from(["rsfetch", "--logo-type", "giant"])
            .expect_err("invalid logo-type should be rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }
}
