use rs_common::sanitize_control_chars;
use serde_json::{Value, json};

use crate::model::{SocketKind, SocketRecord, SocketState, SummaryStats, format_endpoint};

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_MAGENTA: &str = "\x1b[35m";

fn colorize(text: &str, code: &str, no_color: bool) -> String {
    if no_color || text.is_empty() {
        text.to_string()
    } else {
        format!("{code}{text}{ANSI_RESET}")
    }
}

fn state_color(state: SocketState) -> Option<&'static str> {
    match state {
        SocketState::Established | SocketState::Connected => Some(ANSI_GREEN),
        SocketState::Listen => Some(ANSI_CYAN),
        SocketState::TimeWait
        | SocketState::SynSent
        | SocketState::SynRecv
        | SocketState::NewSynRecv
        | SocketState::FinWait1
        | SocketState::FinWait2
        | SocketState::CloseWait
        | SocketState::LastAck
        | SocketState::Closing
        | SocketState::Connecting
        | SocketState::Disconnecting => Some(ANSI_YELLOW),
        SocketState::Unconn | SocketState::Close | SocketState::Unknown(_) => Some(ANSI_DIM),
    }
}

fn pad_colored(
    plain: &str,
    width: usize,
    right_align: bool,
    color: Option<&str>,
    no_color: bool,
) -> String {
    let pad = width.saturating_sub(plain.len());
    let styled = match color {
        Some(code) => colorize(plain, code, no_color),
        None => plain.to_string(),
    };
    if right_align {
        format!("{}{styled}", " ".repeat(pad))
    } else {
        format!("{styled}{}", " ".repeat(pad))
    }
}

fn format_local_addr(rec: &SocketRecord, numeric: bool) -> String {
    match &rec.kind {
        SocketKind::Inet { local, .. } => format_endpoint(local, rec.protocol, numeric),
        SocketKind::Unix { path, .. } => path
            .as_deref()
            .map(sanitize_control_chars)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "*".to_string()),
    }
}

fn format_peer_addr(rec: &SocketRecord, numeric: bool) -> String {
    match &rec.kind {
        SocketKind::Inet { remote, .. } => format_endpoint(remote, rec.protocol, numeric),
        SocketKind::Unix { .. } => "*".to_string(),
    }
}

fn format_process_col(rec: &SocketRecord) -> String {
    match &rec.process {
        Some(proc_info) => {
            let safe_name = sanitize_control_chars(&proc_info.name);
            format!(
                "users:((\"{safe_name}\",pid={},uid={}))",
                proc_info.pid, proc_info.uid
            )
        }
        None => String::new(),
    }
}

/// Formats `SummaryStats` in an `ss -s` style summary report.
#[must_use]
pub fn format_summary(stats: &SummaryStats, no_color: bool) -> String {
    let tcp_total = stats.tcp + stats.tcp6;
    let udp_total = stats.udp + stats.udp6;

    let total_label = colorize("Total:", ANSI_BOLD, no_color);
    let tcp_label = colorize("TCP:", ANSI_BOLD, no_color);
    let table_header = colorize(
        &format!(
            "{:<9} {:<9} {:<9} {:<9}",
            "Transport", "Total", "IP", "IPv6"
        ),
        ANSI_BOLD,
        no_color,
    );

    let lines = [
        format!("{total_label} {}", stats.total),
        format!(
            "{tcp_label}   {} (estab {}, listen {}, time-wait {}, syn-sent {}, syn-recv {}, fin-wait {}, close-wait {}, other {})",
            tcp_total,
            stats.tcp_established,
            stats.tcp_listen,
            stats.tcp_time_wait,
            stats.tcp_syn_sent,
            stats.tcp_syn_recv,
            stats.tcp_fin_wait,
            stats.tcp_close_wait,
            stats.tcp_other
        ),
        String::new(),
        table_header,
        format!(
            "{:<9} {:<9} {:<9} {:<9}",
            "TCP", tcp_total, stats.tcp, stats.tcp6
        ),
        format!(
            "{:<9} {:<9} {:<9} {:<9}",
            "UDP", udp_total, stats.udp, stats.udp6
        ),
        format!("{:<9} {:<9} {:<9} {:<9}", "UNIX", stats.unix, "-", "-"),
    ];

    lines.join("\n")
}

struct TableRow {
    netid: String,
    state: String,
    state_raw: SocketState,
    recv_q: String,
    send_q: String,
    local: String,
    peer: String,
    process: String,
}

/// Formats socket records as an aligned `ss`-style table.
#[must_use]
pub fn format_table(
    records: &[SocketRecord],
    show_processes: bool,
    numeric: bool,
    no_color: bool,
) -> String {
    let headers = [
        "Netid",
        "State",
        "Recv-Q",
        "Send-Q",
        "Local Address:Port",
        "Peer Address:Port",
        "Process",
    ];

    let rows: Vec<TableRow> = records
        .iter()
        .map(|rec| TableRow {
            netid: rec.protocol.as_str().to_string(),
            state: rec.state.to_string(),
            state_raw: rec.state,
            recv_q: rec.recv_q.to_string(),
            send_q: rec.send_q.to_string(),
            local: format_local_addr(rec, numeric),
            peer: format_peer_addr(rec, numeric),
            process: if show_processes {
                format_process_col(rec)
            } else {
                String::new()
            },
        })
        .collect();

    let mut widths = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
        headers[4].len(),
        headers[5].len(),
    ];

    for row in &rows {
        widths[0] = widths[0].max(row.netid.len());
        widths[1] = widths[1].max(row.state.len());
        widths[2] = widths[2].max(row.recv_q.len());
        widths[3] = widths[3].max(row.send_q.len());
        widths[4] = widths[4].max(row.local.len());
        widths[5] = widths[5].max(row.peer.len());
    }

    let mut lines = Vec::with_capacity(rows.len() + 1);

    let header_cols = [
        pad_colored(headers[0], widths[0], false, Some(ANSI_BOLD), no_color),
        pad_colored(headers[1], widths[1], false, Some(ANSI_BOLD), no_color),
        pad_colored(headers[2], widths[2], true, Some(ANSI_BOLD), no_color),
        pad_colored(headers[3], widths[3], true, Some(ANSI_BOLD), no_color),
        pad_colored(headers[4], widths[4], false, Some(ANSI_BOLD), no_color),
        pad_colored(headers[5], widths[5], false, Some(ANSI_BOLD), no_color),
    ];
    let mut header_line = header_cols.join("  ");
    if show_processes {
        header_line.push_str("  ");
        header_line.push_str(&colorize(headers[6], ANSI_BOLD, no_color));
    }
    lines.push(header_line.trim_end().to_string());

    for row in &rows {
        let mut cols = vec![
            pad_colored(&row.netid, widths[0], false, Some(ANSI_CYAN), no_color),
            pad_colored(
                &row.state,
                widths[1],
                false,
                state_color(row.state_raw),
                no_color,
            ),
            pad_colored(&row.recv_q, widths[2], true, None, no_color),
            pad_colored(&row.send_q, widths[3], true, None, no_color),
            pad_colored(&row.local, widths[4], false, None, no_color),
            pad_colored(&row.peer, widths[5], false, None, no_color),
        ];

        if show_processes && !row.process.is_empty() {
            cols.push(colorize(&row.process, ANSI_MAGENTA, no_color));
        }

        lines.push(cols.join("  ").trim_end().to_string());
    }

    lines.join("\n")
}

/// Serializes socket records to a JSON string using `serde_json::json!`.
#[must_use]
pub fn format_json_sockets(records: &[SocketRecord], numeric: bool) -> String {
    let sockets: Vec<Value> = records
        .iter()
        .map(|rec| {
            let process = rec.process.as_ref().map(|p| {
                json!({
                    "pid": p.pid,
                    "name": sanitize_control_chars(&p.name),
                    "cmdline": sanitize_control_chars(&p.cmdline),
                    "uid": p.uid,
                })
            });

            match &rec.kind {
                SocketKind::Inet { local, remote } => json!({
                    "protocol": rec.protocol.as_str(),
                    "state": rec.state.to_string(),
                    "recv_q": rec.recv_q,
                    "send_q": rec.send_q,
                    "local_address": local.ip.to_string(),
                    "local_port": local.port,
                    "local": format_endpoint(local, rec.protocol, numeric),
                    "remote_address": remote.ip.to_string(),
                    "remote_port": remote.port,
                    "remote": format_endpoint(remote, rec.protocol, numeric),
                    "uid": rec.uid,
                    "inode": rec.inode,
                    "process": process,
                }),
                SocketKind::Unix {
                    unix_type,
                    flags,
                    ref_count,
                    path,
                } => json!({
                    "protocol": rec.protocol.as_str(),
                    "state": rec.state.to_string(),
                    "recv_q": rec.recv_q,
                    "send_q": rec.send_q,
                    "unix_type": unix_type.to_string(),
                    "flags": flags,
                    "ref_count": ref_count,
                    "path": path.as_deref().map(sanitize_control_chars),
                    "uid": rec.uid,
                    "inode": rec.inode,
                    "process": process,
                }),
            }
        })
        .collect();

    let payload = json!({
        "count": sockets.len(),
        "sockets": sockets,
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
}

/// Serializes `SummaryStats` to a JSON string using `serde_json::json!`.
#[must_use]
pub fn format_json_summary(stats: &SummaryStats) -> String {
    let payload = json!({
        "total": stats.total,
        "tcp": stats.tcp,
        "tcp6": stats.tcp6,
        "udp": stats.udp,
        "udp6": stats.udp6,
        "unix": stats.unix,
        "tcp_states": {
            "established": stats.tcp_established,
            "listen": stats.tcp_listen,
            "time_wait": stats.tcp_time_wait,
            "syn_sent": stats.tcp_syn_sent,
            "syn_recv": stats.tcp_syn_recv,
            "fin_wait": stats.tcp_fin_wait,
            "close_wait": stats.tcp_close_wait,
            "other": stats.tcp_other,
        }
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;
    use crate::model::{Endpoint, ProcessInfo, Protocol, UnixSocketType};

    fn sample_tcp_record() -> SocketRecord {
        SocketRecord {
            protocol: Protocol::Tcp,
            state: SocketState::Established,
            recv_q: 0,
            send_q: 128,
            kind: SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                    port: 51234,
                },
                remote: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
                    port: 443,
                },
            },
            uid: 1000,
            inode: 4242,
            process: Some(ProcessInfo {
                pid: 1234,
                name: "curl\x1b[31m".to_string(),
                cmdline: "curl https://example.com\n".to_string(),
                uid: 1000,
            }),
        }
    }

    fn sample_unix_record() -> SocketRecord {
        SocketRecord {
            protocol: Protocol::Unix,
            state: SocketState::Listen,
            recv_q: 0,
            send_q: 0,
            kind: SocketKind::Unix {
                unix_type: UnixSocketType::Stream,
                flags: 0x0001_0000,
                ref_count: 2,
                path: Some("/run/dbus/system_bus_socket\x07".to_string()),
            },
            uid: 0,
            inode: 9999,
            process: Some(ProcessInfo {
                pid: 500,
                name: "dbus-daemon".to_string(),
                cmdline: "/usr/bin/dbus-daemon".to_string(),
                uid: 0,
            }),
        }
    }

    fn sample_listen_v6() -> SocketRecord {
        SocketRecord {
            protocol: Protocol::Tcp6,
            state: SocketState::Listen,
            recv_q: 0,
            send_q: 4096,
            kind: SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    port: 22,
                },
                remote: Endpoint {
                    ip: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    port: 0,
                },
            },
            uid: 0,
            inode: 777,
            process: None,
        }
    }

    #[test]
    fn table_alignment_numeric_and_service_names() {
        let records = vec![sample_tcp_record(), sample_listen_v6()];

        let resolved = format_table(&records, true, false, true);
        assert!(resolved.contains("93.184.216.34:https"));
        assert!(resolved.contains("*:ssh"));
        assert!(resolved.contains("users:((\"curl [31m\",pid=1234,uid=1000))"));
        assert!(!resolved.contains('\x1b'));

        let numeric = format_table(&records, false, true, true);
        assert!(numeric.contains("93.184.216.34:443"));
        assert!(numeric.contains("*:22"));
        assert!(!numeric.contains("Process"));

        // Every line should have aligned columns when no_color is true.
        let lines: Vec<&str> = numeric.lines().collect();
        assert_eq!(lines.len(), 3);
        let peer_hdr_idx = lines[0].find("Peer Address:Port").unwrap();
        let peer_row0_idx = lines[1].find("93.184.216.34:443").unwrap();
        let peer_row1_idx = lines[2].find("*:*").unwrap();
        assert_eq!(peer_hdr_idx, peer_row0_idx);
        assert_eq!(peer_hdr_idx, peer_row1_idx);
    }

    #[test]
    fn table_no_color_vs_color() {
        let records = vec![sample_tcp_record()];

        let plain = format_table(&records, false, true, true);
        assert!(!plain.contains("\x1b["));

        let colored = format_table(&records, false, true, false);
        assert!(colored.contains("\x1b["));
    }

    #[test]
    fn summary_formatting_plain_and_colored() {
        let stats = SummaryStats {
            total: 15,
            tcp: 6,
            tcp6: 3,
            udp: 2,
            udp6: 1,
            unix: 3,
            tcp_established: 4,
            tcp_listen: 3,
            tcp_time_wait: 1,
            tcp_syn_sent: 0,
            tcp_syn_recv: 0,
            tcp_fin_wait: 1,
            tcp_close_wait: 0,
            tcp_other: 0,
        };

        let plain = format_summary(&stats, true);
        assert!(!plain.contains("\x1b["));
        assert!(plain.contains("Total: 15"));
        assert!(plain.contains("TCP:   9 (estab 4, listen 3, time-wait 1"));
        assert!(plain.contains("UDP       3         2         1"));
        assert!(plain.contains("UNIX      3         -         -"));

        let colored = format_summary(&stats, false);
        assert!(colored.contains("\x1b["));
    }

    #[test]
    fn json_sockets_schema_and_sanitization() {
        let records = vec![sample_tcp_record(), sample_unix_record()];
        let out = format_json_sockets(&records, false);
        let parsed: Value = serde_json::from_str(&out).expect("valid JSON");

        assert_eq!(parsed["count"], 2);
        let sockets = parsed["sockets"].as_array().unwrap();
        assert_eq!(sockets.len(), 2);

        assert_eq!(sockets[0]["protocol"], "tcp");
        assert_eq!(sockets[0]["state"], "ESTAB");
        assert_eq!(sockets[0]["local_port"], 51234);
        assert_eq!(sockets[0]["remote_port"], 443);
        assert_eq!(sockets[0]["remote"], "93.184.216.34:https");
        assert_eq!(sockets[0]["process"]["pid"], 1234);
        assert_eq!(sockets[0]["process"]["name"], "curl [31m");

        assert_eq!(sockets[1]["protocol"], "unix");
        assert_eq!(sockets[1]["unix_type"], "STREAM");
        assert_eq!(sockets[1]["path"], "/run/dbus/system_bus_socket ");

        let out_numeric = format_json_sockets(&records, true);
        let parsed_numeric: Value = serde_json::from_str(&out_numeric).unwrap();
        assert_eq!(parsed_numeric["sockets"][0]["remote"], "93.184.216.34:443");
    }

    #[test]
    fn json_summary_schema() {
        let stats = SummaryStats {
            total: 10,
            tcp: 5,
            tcp6: 1,
            udp: 2,
            udp6: 0,
            unix: 2,
            tcp_established: 3,
            tcp_listen: 2,
            tcp_time_wait: 1,
            tcp_syn_sent: 0,
            tcp_syn_recv: 0,
            tcp_fin_wait: 0,
            tcp_close_wait: 0,
            tcp_other: 0,
        };

        let out = format_json_summary(&stats);
        let parsed: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["total"], 10);
        assert_eq!(parsed["tcp"], 5);
        assert_eq!(parsed["tcp6"], 1);
        assert_eq!(parsed["udp"], 2);
        assert_eq!(parsed["udp6"], 0);
        assert_eq!(parsed["unix"], 2);
        assert_eq!(parsed["tcp_states"]["established"], 3);
        assert_eq!(parsed["tcp_states"]["listen"], 2);
        assert_eq!(parsed["tcp_states"]["time_wait"], 1);
    }
}
