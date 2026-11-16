use clap::{Parser, ValueEnum};

use crate::model::{ListenMode, SocketFilter, SocketState, SortColumn};
use crate::procfs::CollectOptions;

/// Sort column accepted by `--sort`.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumnArg {
    #[default]
    Proto,
    State,
    RecvQ,
    SendQ,
    Local,
    Remote,
    Pid,
    Process,
}

impl From<SortColumnArg> for SortColumn {
    fn from(arg: SortColumnArg) -> Self {
        match arg {
            SortColumnArg::Proto => Self::Proto,
            SortColumnArg::State => Self::State,
            SortColumnArg::RecvQ => Self::RecvQ,
            SortColumnArg::SendQ => Self::SendQ,
            SortColumnArg::Local => Self::Local,
            SortColumnArg::Remote => Self::Remote,
            SortColumnArg::Pid => Self::Pid,
            SortColumnArg::Process => Self::Process,
        }
    }
}

fn parse_socket_state(s: &str) -> Result<SocketState, String> {
    SocketState::parse_filter(s).ok_or_else(|| {
        format!(
            "unknown socket state '{s}' (expected e.g. estab, listen, time-wait, syn-sent, syn-recv, fin-wait-1, fin-wait-2, close, close-wait, last-ack, closing, unconn)"
        )
    })
}

/// Command-line arguments for `rssock`.
#[derive(Parser, Debug, Clone, PartialEq, Eq)]
#[command(
    name = "rssock",
    version,
    about = "Interactive Linux socket and connection explorer (ss replacement)"
)]
pub struct CliArgs {
    /// Show TCP sockets.
    #[arg(short = 't', long)]
    pub tcp: bool,

    /// Show UDP sockets.
    #[arg(short = 'u', long)]
    pub udp: bool,

    /// Show UNIX domain sockets.
    #[arg(short = 'x', long)]
    pub unix: bool,

    /// Show only listening sockets.
    #[arg(short = 'l', long)]
    pub listening: bool,

    /// Show both listening and non-listening sockets.
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Do not resolve service names.
    #[arg(short = 'n', long)]
    pub numeric: bool,

    /// Show process using socket.
    #[arg(short = 'p', long)]
    pub processes: bool,

    /// Print socket summary statistics and exit.
    #[arg(short = 's', long)]
    pub summary: bool,

    /// Filter by socket state (e.g. estab, listen, time-wait, unconn).
    #[arg(long, value_name = "STATE", value_parser = parse_socket_state)]
    pub state: Option<SocketState>,

    /// Sort column.
    #[arg(
        long,
        value_enum,
        default_value_t = SortColumnArg::Proto,
        value_name = "COLUMN"
    )]
    pub sort: SortColumnArg,

    /// Reverse sort order.
    #[arg(short = 'r', long)]
    pub reverse: bool,

    /// Substring filter on address, port, path, state, or process.
    #[arg(short = 'f', long, value_name = "QUERY")]
    pub filter: Option<String>,

    /// Emit JSON output.
    #[arg(long)]
    pub json: bool,

    /// Disable color output.
    #[arg(long)]
    pub no_color: bool,

    /// Launch interactive TUI mode.
    #[arg(long)]
    pub tui: bool,

    /// TUI refresh interval in milliseconds (100..=60000).
    #[arg(
        short = 'd',
        long,
        default_value_t = 1000,
        value_name = "MS",
        value_parser = clap::value_parser!(u64).range(100..=60_000)
    )]
    pub delay: u64,
}

impl CliArgs {
    /// Builds `CollectOptions` from the selected protocol and process flags.
    ///
    /// If none of `-t`, `-u`, or `-x` is set, all three families are collected.
    #[must_use]
    pub fn collect_options(&self) -> CollectOptions {
        let any_proto = self.tcp || self.udp || self.unix;
        CollectOptions {
            tcp: !any_proto || self.tcp,
            udp: !any_proto || self.udp,
            unix: !any_proto || self.unix,
            with_processes: self.processes || self.tui,
        }
    }

    /// Builds `SocketFilter` from protocol, listen-mode, state, and query flags.
    #[must_use]
    pub fn socket_filter(&self) -> SocketFilter {
        let any_proto = self.tcp || self.udp || self.unix;
        let listen_mode = if self.all {
            ListenMode::All
        } else if self.listening {
            ListenMode::Listening
        } else {
            ListenMode::NonListening
        };

        SocketFilter {
            tcp: !any_proto || self.tcp,
            udp: !any_proto || self.udp,
            unix: !any_proto || self.unix,
            listen_mode,
            state: self.state,
            query: self.filter.clone(),
        }
    }

    /// Returns the resolved `SortColumn` for `filter_and_sort`.
    #[must_use]
    pub fn sort_column(&self) -> SortColumn {
        self.sort.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_args_collect_all_protocols_and_non_listening() {
        let args = CliArgs::try_parse_from(["rssock"]).expect("default args should parse");
        assert_eq!(
            args.collect_options(),
            CollectOptions {
                tcp: true,
                udp: true,
                unix: true,
                with_processes: false,
            }
        );
        assert_eq!(
            args.socket_filter(),
            SocketFilter {
                tcp: true,
                udp: true,
                unix: true,
                listen_mode: ListenMode::NonListening,
                state: None,
                query: None,
            }
        );
        assert_eq!(args.sort_column(), SortColumn::Proto);
        assert_eq!(args.delay, 1000);
        assert!(!args.reverse);
        assert!(!args.numeric);
        assert!(!args.json);
        assert!(!args.no_color);
        assert!(!args.tui);
        assert!(!args.summary);
    }

    #[test]
    fn protocol_flag_combinations() {
        let tcp_only = CliArgs::try_parse_from(["rssock", "-t"]).unwrap();
        assert_eq!(
            tcp_only.collect_options(),
            CollectOptions {
                tcp: true,
                udp: false,
                unix: false,
                with_processes: false,
            }
        );
        assert!(tcp_only.socket_filter().tcp);
        assert!(!tcp_only.socket_filter().udp);
        assert!(!tcp_only.socket_filter().unix);

        let tcp_udp = CliArgs::try_parse_from(["rssock", "--tcp", "--udp"]).unwrap();
        assert_eq!(
            tcp_udp.collect_options(),
            CollectOptions {
                tcp: true,
                udp: true,
                unix: false,
                with_processes: false,
            }
        );

        let unix_only = CliArgs::try_parse_from(["rssock", "-x"]).unwrap();
        assert_eq!(
            unix_only.collect_options(),
            CollectOptions {
                tcp: false,
                udp: false,
                unix: true,
                with_processes: false,
            }
        );
    }

    #[test]
    fn listen_mode_precedence_all_over_listening() {
        let listening = CliArgs::try_parse_from(["rssock", "-l"]).unwrap();
        assert_eq!(listening.socket_filter().listen_mode, ListenMode::Listening);

        let all = CliArgs::try_parse_from(["rssock", "-a"]).unwrap();
        assert_eq!(all.socket_filter().listen_mode, ListenMode::All);

        let both = CliArgs::try_parse_from(["rssock", "-l", "-a"]).unwrap();
        assert_eq!(both.socket_filter().listen_mode, ListenMode::All);
    }

    #[test]
    fn with_processes_enabled_by_p_or_tui() {
        let with_p = CliArgs::try_parse_from(["rssock", "-p"]).unwrap();
        assert!(with_p.collect_options().with_processes);

        let with_tui = CliArgs::try_parse_from(["rssock", "--tui"]).unwrap();
        assert!(with_tui.collect_options().with_processes);
    }

    #[test]
    fn state_sort_filter_and_delay_flags() {
        let args = CliArgs::try_parse_from([
            "rssock",
            "--state",
            "time-wait",
            "--sort",
            "recv-q",
            "-r",
            "-f",
            "443",
            "-n",
            "-s",
            "--json",
            "--no-color",
            "-d",
            "250",
        ])
        .unwrap();

        assert_eq!(args.state, Some(SocketState::TimeWait));
        assert_eq!(args.sort_column(), SortColumn::RecvQ);
        assert!(args.reverse);
        assert_eq!(args.filter.as_deref(), Some("443"));
        assert!(args.numeric);
        assert!(args.summary);
        assert!(args.json);
        assert!(args.no_color);
        assert_eq!(args.delay, 250);
    }

    #[test]
    fn invalid_state_and_delay_bounds_rejected() {
        assert!(CliArgs::try_parse_from(["rssock", "--state", "bogus"]).is_err());
        assert!(CliArgs::try_parse_from(["rssock", "-d", "99"]).is_err());
        assert!(CliArgs::try_parse_from(["rssock", "-d", "60001"]).is_err());
        assert!(CliArgs::try_parse_from(["rssock", "--sort", "unknown"]).is_err());
    }
}
