use std::cmp::Ordering;
use std::fmt;
use std::net::IpAddr;

/// Transport or address family protocol of a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Protocol {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
    Unix,
}

impl Protocol {
    /// Returns the lowercase protocol label as displayed in `ss`-style output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Tcp6 => "tcp6",
            Self::Udp => "udp",
            Self::Udp6 => "udp6",
            Self::Unix => "unix",
        }
    }

    /// Returns `true` for IPv4 or IPv6 TCP sockets.
    #[must_use]
    pub const fn is_tcp(self) -> bool {
        matches!(self, Self::Tcp | Self::Tcp6)
    }

    /// Returns `true` for IPv4 or IPv6 UDP sockets.
    #[must_use]
    pub const fn is_udp(self) -> bool {
        matches!(self, Self::Udp | Self::Udp6)
    }

    /// Returns `true` for UNIX domain sockets.
    #[must_use]
    pub const fn is_unix(self) -> bool {
        matches!(self, Self::Unix)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Normalized socket state across TCP, UDP, and UNIX domain sockets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SocketState {
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    Close,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    NewSynRecv,
    Unconn,
    Connecting,
    Connected,
    Disconnecting,
    Unknown(u8),
}

/// Linux `SO_ACCEPTCON` flag in `/proc/net/unix` indicating a listening socket.
const UNIX_FLAG_ACCEPTCON: u32 = 0x0001_0000;

impl SocketState {
    /// Maps a `/proc/net/{tcp,tcp6,udp,udp6}` numeric state to `SocketState`.
    #[must_use]
    pub const fn from_inet_state(protocol: Protocol, state: u8) -> Self {
        match state {
            1 => Self::Established,
            2 => Self::SynSent,
            3 => Self::SynRecv,
            4 => Self::FinWait1,
            5 => Self::FinWait2,
            6 => Self::TimeWait,
            7 => {
                if protocol.is_udp() {
                    Self::Unconn
                } else {
                    Self::Close
                }
            }
            8 => Self::CloseWait,
            9 => Self::LastAck,
            10 => Self::Listen,
            11 => Self::Closing,
            12 => Self::NewSynRecv,
            other => Self::Unknown(other),
        }
    }

    /// Maps `/proc/net/unix` state and flags to `SocketState`.
    #[must_use]
    pub const fn from_unix_state(state: u8, flags: u32) -> Self {
        if flags & UNIX_FLAG_ACCEPTCON != 0 {
            return Self::Listen;
        }
        match state {
            1 => Self::Unconn,
            2 => Self::Connecting,
            3 => Self::Connected,
            4 => Self::Disconnecting,
            other => Self::Unknown(other),
        }
    }

    /// Parses a user-supplied state filter string (case-insensitive).
    #[must_use]
    pub fn parse_filter(s: &str) -> Option<Self> {
        let normalized = s.trim().to_ascii_lowercase().replace('_', "-");
        match normalized.as_str() {
            "estab" | "established" => Some(Self::Established),
            "syn-sent" | "synsent" => Some(Self::SynSent),
            "syn-recv" | "synrecv" => Some(Self::SynRecv),
            "fin-wait-1" | "fin-wait1" | "finwait1" => Some(Self::FinWait1),
            "fin-wait-2" | "fin-wait2" | "finwait2" => Some(Self::FinWait2),
            "time-wait" | "timewait" => Some(Self::TimeWait),
            "close" | "closed" => Some(Self::Close),
            "close-wait" | "closewait" => Some(Self::CloseWait),
            "last-ack" | "lastack" => Some(Self::LastAck),
            "listen" | "listening" => Some(Self::Listen),
            "closing" => Some(Self::Closing),
            "new-syn-recv" | "newsynrecv" => Some(Self::NewSynRecv),
            "unconn" | "unconnected" => Some(Self::Unconn),
            "connecting" => Some(Self::Connecting),
            "connected" => Some(Self::Connected),
            "disconnecting" => Some(Self::Disconnecting),
            _ => None,
        }
    }

    /// Returns `true` if the socket is in a listening state.
    #[must_use]
    pub const fn is_listening(self) -> bool {
        matches!(self, Self::Listen)
    }
}

impl fmt::Display for SocketState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Established => "ESTAB",
            Self::SynSent => "SYN-SENT",
            Self::SynRecv => "SYN-RECV",
            Self::FinWait1 => "FIN-WAIT-1",
            Self::FinWait2 => "FIN-WAIT-2",
            Self::TimeWait => "TIME-WAIT",
            Self::Close => "CLOSE",
            Self::CloseWait => "CLOSE-WAIT",
            Self::LastAck => "LAST-ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSING",
            Self::NewSynRecv => "NEW-SYN-RECV",
            Self::Unconn => "UNCONN",
            Self::Connecting => "CONNECTING",
            Self::Connected => "CONNECTED",
            Self::Disconnecting => "DISCONNECTING",
            Self::Unknown(code) => return write!(f, "UNKNOWN({code})"),
        };
        f.write_str(s)
    }
}

/// UNIX domain socket type from `/proc/net/unix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnixSocketType {
    Stream,
    Dgram,
    Raw,
    Rdm,
    SeqPacket,
    Packet,
    Unknown(u16),
}

impl UnixSocketType {
    /// Maps the Linux kernel `sock_type` integer to `UnixSocketType`.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            1 => Self::Stream,
            2 => Self::Dgram,
            3 => Self::Raw,
            4 => Self::Rdm,
            5 => Self::SeqPacket,
            10 => Self::Packet,
            other => Self::Unknown(other),
        }
    }

    /// Returns the uppercase socket type label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stream => "STREAM",
            Self::Dgram => "DGRAM",
            Self::Raw => "RAW",
            Self::Rdm => "RDM",
            Self::SeqPacket => "SEQPACKET",
            Self::Packet => "PACKET",
            Self::Unknown(_) => "UNKNOWN",
        }
    }
}

impl fmt::Display for UnixSocketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(code) => write!(f, "UNKNOWN({code})"),
            _ => f.write_str(self.as_str()),
        }
    }
}

/// IP address and port pair for an INET socket endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Endpoint {
    pub ip: IpAddr,
    pub port: u16,
}

/// Protocol-specific socket addressing details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketKind {
    Inet {
        local: Endpoint,
        remote: Endpoint,
    },
    Unix {
        unix_type: UnixSocketType,
        flags: u32,
        ref_count: u32,
        path: Option<String>,
    },
}

/// Process metadata resolved from `/proc/<pid>/fd` inode mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    pub uid: u32,
}

/// Unified socket entry parsed from `/proc/net/*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketRecord {
    pub protocol: Protocol,
    pub state: SocketState,
    pub recv_q: u32,
    pub send_q: u32,
    pub kind: SocketKind,
    pub uid: u32,
    pub inode: u64,
    pub process: Option<ProcessInfo>,
}

/// Controls whether listening, non-listening, or all sockets are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListenMode {
    #[default]
    NonListening,
    Listening,
    All,
}

/// Column used for sorting socket records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
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

/// Filter criteria applied to socket records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketFilter {
    pub tcp: bool,
    pub udp: bool,
    pub unix: bool,
    pub listen_mode: ListenMode,
    pub state: Option<SocketState>,
    pub query: Option<String>,
}

impl Default for SocketFilter {
    fn default() -> Self {
        Self {
            tcp: true,
            udp: true,
            unix: true,
            listen_mode: ListenMode::All,
            state: None,
            query: None,
        }
    }
}

/// Filters `records` according to `filter` and sorts them by `sort` (optionally reversed).
#[must_use]
pub fn filter_and_sort(
    records: &[SocketRecord],
    filter: &SocketFilter,
    sort: SortColumn,
    reverse: bool,
) -> Vec<SocketRecord> {
    let query = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_ascii_lowercase);

    let mut result: Vec<SocketRecord> = records
        .iter()
        .filter(|rec| {
            let proto_ok = (rec.protocol.is_tcp() && filter.tcp)
                || (rec.protocol.is_udp() && filter.udp)
                || (rec.protocol.is_unix() && filter.unix);
            if !proto_ok {
                return false;
            }

            let listen_ok = match filter.listen_mode {
                ListenMode::All => true,
                ListenMode::Listening => rec.state.is_listening(),
                ListenMode::NonListening => !rec.state.is_listening(),
            };
            if !listen_ok {
                return false;
            }

            if let Some(target_state) = filter.state
                && rec.state != target_state
            {
                return false;
            }

            if let Some(ref q) = query
                && !record_matches_query(rec, q)
            {
                return false;
            }

            true
        })
        .cloned()
        .collect();

    result.sort_by(|a, b| {
        let ord = compare_records(a, b, sort).then_with(|| a.inode.cmp(&b.inode));
        if reverse { ord.reverse() } else { ord }
    });

    result
}

fn record_matches_query(rec: &SocketRecord, q: &str) -> bool {
    if rec.protocol.as_str().contains(q) {
        return true;
    }
    if rec.state.to_string().to_ascii_lowercase().contains(q) {
        return true;
    }
    if rec.inode.to_string().contains(q) {
        return true;
    }
    match &rec.kind {
        SocketKind::Inet { local, remote } => {
            if format_endpoint(local, rec.protocol, true)
                .to_ascii_lowercase()
                .contains(q)
                || format_endpoint(local, rec.protocol, false)
                    .to_ascii_lowercase()
                    .contains(q)
                || format_endpoint(remote, rec.protocol, true)
                    .to_ascii_lowercase()
                    .contains(q)
                || format_endpoint(remote, rec.protocol, false)
                    .to_ascii_lowercase()
                    .contains(q)
            {
                return true;
            }
        }
        SocketKind::Unix { path, .. } => {
            if let Some(p) = path
                && p.to_ascii_lowercase().contains(q)
            {
                return true;
            }
        }
    }
    if let Some(proc_info) = &rec.process
        && (proc_info.pid.to_string().contains(q)
            || proc_info.name.to_ascii_lowercase().contains(q)
            || proc_info.cmdline.to_ascii_lowercase().contains(q))
    {
        return true;
    }
    false
}

fn compare_records(a: &SocketRecord, b: &SocketRecord, sort: SortColumn) -> Ordering {
    match sort {
        SortColumn::Proto => a.protocol.cmp(&b.protocol),
        SortColumn::State => a.state.cmp(&b.state),
        SortColumn::RecvQ => a.recv_q.cmp(&b.recv_q),
        SortColumn::SendQ => a.send_q.cmp(&b.send_q),
        SortColumn::Local => local_sort_key(a).cmp(&local_sort_key(b)),
        SortColumn::Remote => remote_sort_key(a).cmp(&remote_sort_key(b)),
        SortColumn::Pid => {
            let pid_a = a.process.as_ref().map_or(u32::MAX, |p| p.pid);
            let pid_b = b.process.as_ref().map_or(u32::MAX, |p| p.pid);
            pid_a.cmp(&pid_b)
        }
        SortColumn::Process => {
            let name_a = a.process.as_ref().map_or("", |p| p.name.as_str());
            let name_b = b.process.as_ref().map_or("", |p| p.name.as_str());
            name_a.cmp(name_b)
        }
    }
}

fn local_sort_key(rec: &SocketRecord) -> String {
    match &rec.kind {
        SocketKind::Inet { local, .. } => format_endpoint(local, rec.protocol, true),
        SocketKind::Unix { path, .. } => path.clone().unwrap_or_default(),
    }
}

fn remote_sort_key(rec: &SocketRecord) -> String {
    match &rec.kind {
        SocketKind::Inet { remote, .. } => format_endpoint(remote, rec.protocol, true),
        SocketKind::Unix { .. } => String::new(),
    }
}

/// Aggregated socket counts by protocol and TCP state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SummaryStats {
    pub total: usize,
    pub tcp: usize,
    pub tcp6: usize,
    pub udp: usize,
    pub udp6: usize,
    pub unix: usize,
    pub tcp_established: usize,
    pub tcp_listen: usize,
    pub tcp_time_wait: usize,
    pub tcp_syn_sent: usize,
    pub tcp_syn_recv: usize,
    pub tcp_fin_wait: usize,
    pub tcp_close_wait: usize,
    pub tcp_other: usize,
}

impl SummaryStats {
    /// Computes summary statistics from a slice of socket records.
    #[must_use]
    pub fn from_records(records: &[SocketRecord]) -> Self {
        let mut stats = Self {
            total: records.len(),
            ..Self::default()
        };

        for rec in records {
            match rec.protocol {
                Protocol::Tcp => stats.tcp += 1,
                Protocol::Tcp6 => stats.tcp6 += 1,
                Protocol::Udp => stats.udp += 1,
                Protocol::Udp6 => stats.udp6 += 1,
                Protocol::Unix => stats.unix += 1,
            }

            if rec.protocol.is_tcp() {
                match rec.state {
                    SocketState::Established => stats.tcp_established += 1,
                    SocketState::Listen => stats.tcp_listen += 1,
                    SocketState::TimeWait => stats.tcp_time_wait += 1,
                    SocketState::SynSent => stats.tcp_syn_sent += 1,
                    SocketState::SynRecv | SocketState::NewSynRecv => stats.tcp_syn_recv += 1,
                    SocketState::FinWait1 | SocketState::FinWait2 => stats.tcp_fin_wait += 1,
                    SocketState::CloseWait => stats.tcp_close_wait += 1,
                    _ => stats.tcp_other += 1,
                }
            }
        }

        stats
    }
}

/// Static lookup for well-known port service names.
#[must_use]
pub const fn service_name(port: u16, protocol: Protocol) -> Option<&'static str> {
    if protocol.is_unix() {
        return None;
    }
    match port {
        20 => Some("ftp-data"),
        21 => Some("ftp"),
        22 => Some("ssh"),
        23 => Some("telnet"),
        25 => Some("smtp"),
        53 => Some("domain"),
        67 if protocol.is_udp() => Some("bootps"),
        68 if protocol.is_udp() => Some("bootpc"),
        80 => Some("http"),
        110 => Some("pop3"),
        111 => Some("sunrpc"),
        123 if protocol.is_udp() => Some("ntp"),
        143 => Some("imap"),
        443 => Some("https"),
        465 => Some("smtps"),
        514 if protocol.is_udp() => Some("syslog"),
        587 => Some("submission"),
        631 => Some("ipp"),
        993 => Some("imaps"),
        995 => Some("pop3s"),
        1433 => Some("ms-sql-s"),
        1521 => Some("oracle"),
        2049 => Some("nfs"),
        3306 => Some("mysql"),
        3389 => Some("ms-wbt-server"),
        5432 => Some("postgresql"),
        5672 => Some("amqp"),
        5900 => Some("vnc"),
        6379 => Some("redis"),
        8080 => Some("http-alt"),
        8443 => Some("https-alt"),
        9090 => Some("prometheus"),
        27017 => Some("mongodb"),
        _ => None,
    }
}

/// Formats an IP/port endpoint in `ss` style (`*:port`, `ip:port`, `[ipv6]:port`).
///
/// When `numeric` is `false`, well-known ports are resolved via [`service_name`].
#[must_use]
pub fn format_endpoint(ep: &Endpoint, proto: Protocol, numeric: bool) -> String {
    let ip_part = if ep.ip.is_unspecified() {
        "*".to_string()
    } else if ep.ip.is_ipv6() {
        format!("[{}]", ep.ip)
    } else {
        ep.ip.to_string()
    };

    let port_part = if ep.port == 0 {
        "*".to_string()
    } else if !numeric {
        service_name(ep.port, proto)
            .map(str::to_string)
            .unwrap_or_else(|| ep.port.to_string())
    } else {
        ep.port.to_string()
    };

    format!("{ip_part}:{port_part}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn inet_record(
        protocol: Protocol,
        state: SocketState,
        local: (IpAddr, u16),
        remote: (IpAddr, u16),
        inode: u64,
        process: Option<ProcessInfo>,
    ) -> SocketRecord {
        SocketRecord {
            protocol,
            state,
            recv_q: 0,
            send_q: 0,
            kind: SocketKind::Inet {
                local: Endpoint {
                    ip: local.0,
                    port: local.1,
                },
                remote: Endpoint {
                    ip: remote.0,
                    port: remote.1,
                },
            },
            uid: 1000,
            inode,
            process,
        }
    }

    #[test]
    fn state_parsing_from_proc_and_filter() {
        assert_eq!(
            SocketState::from_inet_state(Protocol::Tcp, 1),
            SocketState::Established
        );
        assert_eq!(
            SocketState::from_inet_state(Protocol::Tcp, 10),
            SocketState::Listen
        );
        assert_eq!(
            SocketState::from_inet_state(Protocol::Tcp, 7),
            SocketState::Close
        );
        assert_eq!(
            SocketState::from_inet_state(Protocol::Udp, 7),
            SocketState::Unconn
        );
        assert_eq!(
            SocketState::from_inet_state(Protocol::Tcp, 99),
            SocketState::Unknown(99)
        );

        assert_eq!(
            SocketState::from_unix_state(1, UNIX_FLAG_ACCEPTCON),
            SocketState::Listen
        );
        assert_eq!(SocketState::from_unix_state(1, 0), SocketState::Unconn);
        assert_eq!(SocketState::from_unix_state(3, 0), SocketState::Connected);

        assert_eq!(
            SocketState::parse_filter("ESTAB"),
            Some(SocketState::Established)
        );
        assert_eq!(
            SocketState::parse_filter("established"),
            Some(SocketState::Established)
        );
        assert_eq!(
            SocketState::parse_filter("time-wait"),
            Some(SocketState::TimeWait)
        );
        assert_eq!(
            SocketState::parse_filter("timewait"),
            Some(SocketState::TimeWait)
        );
        assert_eq!(
            SocketState::parse_filter("unconn"),
            Some(SocketState::Unconn)
        );
        assert_eq!(SocketState::parse_filter("bogus"), None);

        assert!(SocketState::Listen.is_listening());
        assert!(!SocketState::Established.is_listening());
        assert_eq!(SocketState::TimeWait.to_string(), "TIME-WAIT");
    }

    #[test]
    fn endpoint_formatting_numeric_and_named() {
        let ep_v4_listen = Endpoint {
            ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 22,
        };
        assert_eq!(format_endpoint(&ep_v4_listen, Protocol::Tcp, true), "*:22");
        assert_eq!(
            format_endpoint(&ep_v4_listen, Protocol::Tcp, false),
            "*:ssh"
        );

        let ep_v6 = Endpoint {
            ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            port: 443,
        };
        assert_eq!(format_endpoint(&ep_v6, Protocol::Tcp6, true), "[::1]:443");
        assert_eq!(
            format_endpoint(&ep_v6, Protocol::Tcp6, false),
            "[::1]:https"
        );

        let ep_zero = Endpoint {
            ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 0,
        };
        assert_eq!(format_endpoint(&ep_zero, Protocol::Udp, false), "*:*");
        assert_eq!(service_name(5432, Protocol::Tcp), Some("postgresql"));
        assert_eq!(service_name(22, Protocol::Unix), None);
    }

    #[test]
    fn filtering_and_sorting_works() {
        let records = vec![
            inet_record(
                Protocol::Tcp,
                SocketState::Listen,
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 22),
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                100,
                Some(ProcessInfo {
                    pid: 420,
                    name: "sshd".to_string(),
                    cmdline: "/usr/sbin/sshd -D".to_string(),
                    uid: 0,
                }),
            ),
            inet_record(
                Protocol::Tcp,
                SocketState::Established,
                (IpAddr::V4(Ipv4Addr::LOCALHOST), 50000),
                (IpAddr::V4(Ipv4Addr::LOCALHOST), 5432),
                200,
                Some(ProcessInfo {
                    pid: 1200,
                    name: "psql".to_string(),
                    cmdline: "psql mydb".to_string(),
                    uid: 1000,
                }),
            ),
            inet_record(
                Protocol::Udp,
                SocketState::Unconn,
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53),
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                300,
                None,
            ),
        ];

        let listen_filter = SocketFilter {
            listen_mode: ListenMode::Listening,
            ..SocketFilter::default()
        };
        let listening = filter_and_sort(&records, &listen_filter, SortColumn::Proto, false);
        assert_eq!(listening.len(), 1);
        assert_eq!(listening[0].inode, 100);

        let query_filter = SocketFilter {
            query: Some("postgresql".to_string()),
            ..SocketFilter::default()
        };
        let matched = filter_and_sort(&records, &query_filter, SortColumn::Proto, false);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].inode, 200);

        let all_sorted_pid_desc =
            filter_and_sort(&records, &SocketFilter::default(), SortColumn::Pid, true);
        assert_eq!(
            all_sorted_pid_desc
                .iter()
                .map(|r| r.inode)
                .collect::<Vec<_>>(),
            vec![300, 200, 100]
        );
    }

    #[test]
    fn summary_stats_counts_protocols_and_tcp_states() {
        let records = vec![
            inet_record(
                Protocol::Tcp,
                SocketState::Established,
                (IpAddr::V4(Ipv4Addr::LOCALHOST), 1000),
                (IpAddr::V4(Ipv4Addr::LOCALHOST), 80),
                1,
                None,
            ),
            inet_record(
                Protocol::Tcp6,
                SocketState::Listen,
                (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 443),
                (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
                2,
                None,
            ),
            inet_record(
                Protocol::Tcp,
                SocketState::TimeWait,
                (IpAddr::V4(Ipv4Addr::LOCALHOST), 1001),
                (IpAddr::V4(Ipv4Addr::LOCALHOST), 80),
                3,
                None,
            ),
            inet_record(
                Protocol::Udp,
                SocketState::Unconn,
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53),
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                4,
                None,
            ),
            SocketRecord {
                protocol: Protocol::Unix,
                state: SocketState::Connected,
                recv_q: 0,
                send_q: 0,
                kind: SocketKind::Unix {
                    unix_type: UnixSocketType::Stream,
                    flags: 0,
                    ref_count: 2,
                    path: Some("/run/dbus/system_bus_socket".to_string()),
                },
                uid: 0,
                inode: 5,
                process: None,
            },
        ];

        let stats = SummaryStats::from_records(&records);
        assert_eq!(stats.total, 5);
        assert_eq!(stats.tcp, 2);
        assert_eq!(stats.tcp6, 1);
        assert_eq!(stats.udp, 1);
        assert_eq!(stats.udp6, 0);
        assert_eq!(stats.unix, 1);
        assert_eq!(stats.tcp_established, 1);
        assert_eq!(stats.tcp_listen, 1);
        assert_eq!(stats.tcp_time_wait, 1);
    }
}
