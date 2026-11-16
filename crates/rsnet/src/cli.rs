use clap::{Parser, ValueEnum};

/// Unit family for formatting bandwidth rates and byte counts.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnitFamily {
    /// Binary bytes (B, KiB, MiB, GiB, TiB).
    #[default]
    BinBytes,
    /// Binary bits (b, Kib, Mib, Gib, Tib).
    BinBits,
    /// SI decimal bytes (B, kB, MB, GB, TB).
    SiBytes,
    /// SI decimal bits (b, kb, Mb, Gb, Tb).
    SiBits,
}

/// Table sort column.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Total,
    Rx,
    Tx,
    Pid,
    Name,
}

/// Active traffic view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    All,
    Processes,
    Connections,
    Addresses,
}

impl ViewMode {
    /// Resolves the active view mode from mutually exclusive CLI view flags.
    pub fn from_args(args: &CliArgs) -> Self {
        if args.processes {
            Self::Processes
        } else if args.connections {
            Self::Connections
        } else if args.addresses {
            Self::Addresses
        } else {
            Self::All
        }
    }
}

/// Command-line arguments for `rsnet`.
#[derive(Parser, Debug, Clone, PartialEq, Eq)]
#[command(
    name = "rsnet",
    version,
    about = "Interactive Linux network bandwidth and connection monitor"
)]
pub struct CliArgs {
    /// Network interface name to monitor (default: all active interfaces).
    #[arg(short = 'i', long, value_name = "NAME")]
    pub interface: Option<String>,

    /// Refresh interval in milliseconds (100..=60000).
    #[arg(
        short = 'd',
        long,
        default_value_t = 1000,
        value_name = "MS",
        value_parser = clap::value_parser!(u64).range(100..=60_000)
    )]
    pub delay: u64,

    /// Run in non-interactive raw text mode instead of TUI.
    #[arg(short = 'r', long)]
    pub raw: bool,

    /// Emit newline-delimited JSON snapshots.
    #[arg(long)]
    pub json: bool,

    /// Stop after N refresh cycles.
    #[arg(short = 'c', long, value_name = "N")]
    pub count: Option<u64>,

    /// Disable reverse DNS hostname resolution.
    #[arg(short = 'n', long)]
    pub no_resolve: bool,

    /// Unit family for rates and sizes.
    #[arg(
        short = 'u',
        long,
        value_enum,
        default_value_t = UnitFamily::BinBytes,
        value_name = "UNIT"
    )]
    pub unit_family: UnitFamily,

    /// Display cumulative totals instead of per-second rates.
    #[arg(short = 't', long)]
    pub total: bool,

    /// Show only the processes table.
    #[arg(short = 'p', long, group = "view")]
    pub processes: bool,

    /// Show only the connections table.
    #[arg(short = 'C', long, group = "view")]
    pub connections: bool,

    /// Show only the remote addresses table.
    #[arg(short = 'a', long, group = "view")]
    pub addresses: bool,

    /// Initial sort column.
    #[arg(
        short = 's',
        long,
        value_enum,
        default_value_t = SortColumn::Total,
        value_name = "COLUMN"
    )]
    pub sort: SortColumn,

    /// Filter processes, connections, or addresses by substring.
    #[arg(short = 'f', long, value_name = "QUERY")]
    pub filter: Option<String>,

    /// Disable color styling.
    #[arg(long)]
    pub no_color: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_applied() {
        let args = CliArgs::try_parse_from(["rsnet"]).expect("default args should parse");
        assert_eq!(args.interface, None);
        assert_eq!(args.delay, 1000);
        assert!(!args.raw);
        assert!(!args.json);
        assert_eq!(args.count, None);
        assert!(!args.no_resolve);
        assert_eq!(args.unit_family, UnitFamily::BinBytes);
        assert!(!args.total);
        assert!(!args.processes);
        assert!(!args.connections);
        assert!(!args.addresses);
        assert_eq!(args.sort, SortColumn::Total);
        assert_eq!(args.filter, None);
        assert!(!args.no_color);
        assert_eq!(ViewMode::from_args(&args), ViewMode::All);
    }

    #[test]
    fn short_flags_parse() {
        let args = CliArgs::try_parse_from([
            "rsnet", "-i", "eth0", "-d", "250", "-r", "-c", "5", "-n", "-u", "si-bits", "-t", "-p",
            "-s", "rx", "-f", "curl",
        ])
        .expect("short flags should parse");
        assert_eq!(args.interface.as_deref(), Some("eth0"));
        assert_eq!(args.delay, 250);
        assert!(args.raw);
        assert_eq!(args.count, Some(5));
        assert!(args.no_resolve);
        assert_eq!(args.unit_family, UnitFamily::SiBits);
        assert!(args.total);
        assert!(args.processes);
        assert!(!args.connections);
        assert!(!args.addresses);
        assert_eq!(args.sort, SortColumn::Rx);
        assert_eq!(args.filter.as_deref(), Some("curl"));
        assert_eq!(ViewMode::from_args(&args), ViewMode::Processes);
    }

    #[test]
    fn long_flags_parse() {
        let args = CliArgs::try_parse_from([
            "rsnet",
            "--interface",
            "wlan0",
            "--delay",
            "60000",
            "--raw",
            "--json",
            "--count",
            "10",
            "--no-resolve",
            "--unit-family",
            "bin-bits",
            "--total",
            "--connections",
            "--sort",
            "name",
            "--filter",
            "443",
            "--no-color",
        ])
        .expect("long flags should parse");
        assert_eq!(args.interface.as_deref(), Some("wlan0"));
        assert_eq!(args.delay, 60_000);
        assert!(args.raw);
        assert!(args.json);
        assert_eq!(args.count, Some(10));
        assert!(args.no_resolve);
        assert_eq!(args.unit_family, UnitFamily::BinBits);
        assert!(args.total);
        assert!(args.connections);
        assert_eq!(args.sort, SortColumn::Name);
        assert_eq!(args.filter.as_deref(), Some("443"));
        assert!(args.no_color);
        assert_eq!(ViewMode::from_args(&args), ViewMode::Connections);
    }

    #[test]
    fn delay_bounds_are_enforced() {
        assert!(CliArgs::try_parse_from(["rsnet", "-d", "100"]).is_ok());
        assert!(CliArgs::try_parse_from(["rsnet", "-d", "60000"]).is_ok());
        assert!(CliArgs::try_parse_from(["rsnet", "-d", "99"]).is_err());
        assert!(CliArgs::try_parse_from(["rsnet", "-d", "60001"]).is_err());
        assert!(CliArgs::try_parse_from(["rsnet", "-d", "0"]).is_err());
        assert!(CliArgs::try_parse_from(["rsnet", "-d", "abc"]).is_err());
    }

    #[test]
    fn unit_family_variants_parse() {
        let cases = [
            ("bin-bytes", UnitFamily::BinBytes),
            ("bin-bits", UnitFamily::BinBits),
            ("si-bytes", UnitFamily::SiBytes),
            ("si-bits", UnitFamily::SiBits),
        ];
        for (raw, expected) in cases {
            let args = CliArgs::try_parse_from(["rsnet", "-u", raw])
                .expect("unit family variant should parse");
            assert_eq!(args.unit_family, expected);
        }
    }

    #[test]
    fn sort_column_variants_parse() {
        let cases = [
            ("total", SortColumn::Total),
            ("rx", SortColumn::Rx),
            ("tx", SortColumn::Tx),
            ("pid", SortColumn::Pid),
            ("name", SortColumn::Name),
        ];
        for (raw, expected) in cases {
            let args = CliArgs::try_parse_from(["rsnet", "-s", raw])
                .expect("sort column variant should parse");
            assert_eq!(args.sort, expected);
        }
    }

    #[test]
    fn view_mode_flags_are_mutually_exclusive_and_resolve_cleanly() {
        let addr_args = CliArgs::try_parse_from(["rsnet", "-a"]).expect("-a should parse");
        assert_eq!(ViewMode::from_args(&addr_args), ViewMode::Addresses);

        let conn_args = CliArgs::try_parse_from(["rsnet", "-C"]).expect("-C should parse");
        assert_eq!(ViewMode::from_args(&conn_args), ViewMode::Connections);

        assert!(CliArgs::try_parse_from(["rsnet", "-p", "-C"]).is_err());
        assert!(CliArgs::try_parse_from(["rsnet", "-p", "-a"]).is_err());
        assert!(CliArgs::try_parse_from(["rsnet", "-C", "-a"]).is_err());
    }
}
