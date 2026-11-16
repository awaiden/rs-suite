use std::time::{SystemTime, UNIX_EPOCH};

use rs_common::sanitize_control_chars;
use serde_json::{Value, json};

use crate::cli::UnitFamily;
use crate::model::{CaptureStatus, NetSnapshot, format_bytes_with_unit, format_rate_with_unit};

/// Returns the current UNIX timestamp in whole seconds.
pub fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn format_metric(bytes: u64, unit: UnitFamily, cumulative: bool) -> String {
    if cumulative {
        format_bytes_with_unit(bytes, unit)
    } else {
        format_rate_with_unit(bytes as f64, unit)
    }
}

fn capture_mode_label(status: &CaptureStatus) -> &'static str {
    match status {
        CaptureStatus::Live => "LIVE",
        CaptureStatus::ProcfsFallback(_) => "PROCFS-ONLY",
    }
}

/// Formats a `NetSnapshot` as line-oriented `--raw` text output.
pub fn format_raw_snapshot(
    snapshot: &NetSnapshot,
    timestamp_secs: u64,
    unit: UnitFamily,
    cumulative: bool,
    no_resolve: bool,
) -> String {
    let mut lines = Vec::new();
    let mode = capture_mode_label(&snapshot.status);
    let total_rx = format_metric(snapshot.total.rx_bytes, unit, cumulative);
    let total_tx = format_metric(snapshot.total.tx_bytes, unit, cumulative);

    match &snapshot.status {
        CaptureStatus::Live => {
            lines.push(format!(
                "ts={timestamp_secs} type=summary mode={mode} rx={total_rx} tx={total_tx}"
            ));
        }
        CaptureStatus::ProcfsFallback(reason) => {
            let safe_reason = sanitize_control_chars(reason);
            lines.push(format!(
                "ts={timestamp_secs} type=summary mode={mode} rx={total_rx} tx={total_tx} reason=\"{safe_reason}\""
            ));
        }
    }

    for iface in &snapshot.interfaces {
        let name = sanitize_control_chars(&iface.name);
        let rx = format_metric(iface.counters.rx_bytes, unit, cumulative);
        let tx = format_metric(iface.counters.tx_bytes, unit, cumulative);
        lines.push(format!(
            "ts={timestamp_secs} type=interface name={name} up={} rx={rx} tx={tx}",
            iface.is_up
        ));
    }

    for proc_item in &snapshot.processes {
        let name = sanitize_control_chars(&proc_item.process.name);
        let rx = format_metric(proc_item.counters.rx_bytes, unit, cumulative);
        let tx = format_metric(proc_item.counters.tx_bytes, unit, cumulative);
        lines.push(format!(
            "ts={timestamp_secs} type=process pid={} name={name} conns={} rx={rx} tx={tx}",
            proc_item.process.pid, proc_item.connection_count
        ));
    }

    for conn in &snapshot.connections {
        let remote_host = if no_resolve {
            conn.key.remote.ip.to_string()
        } else {
            conn.hostname
                .as_deref()
                .map(sanitize_control_chars)
                .unwrap_or_else(|| conn.key.remote.ip.to_string())
        };
        let pid_str = conn
            .process
            .as_ref()
            .map_or_else(|| "-".to_string(), |p| p.pid.to_string());
        let proc_name = conn
            .process
            .as_ref()
            .map_or_else(|| "-".to_string(), |p| sanitize_control_chars(&p.name));
        let rx = format_metric(conn.counters.rx_bytes, unit, cumulative);
        let tx = format_metric(conn.counters.tx_bytes, unit, cumulative);
        lines.push(format!(
            "ts={timestamp_secs} type=connection proto={} local={}:{} remote={remote_host}:{} pid={pid_str} proc={proc_name} rx={rx} tx={tx}",
            conn.key.protocol,
            conn.key.local.ip,
            conn.key.local.port,
            conn.key.remote.port
        ));
    }

    for remote in &snapshot.remotes {
        let host = if no_resolve {
            "-".to_string()
        } else {
            remote
                .hostname
                .as_deref()
                .map(sanitize_control_chars)
                .unwrap_or_else(|| "-".to_string())
        };
        let rx = format_metric(remote.counters.rx_bytes, unit, cumulative);
        let tx = format_metric(remote.counters.tx_bytes, unit, cumulative);
        lines.push(format!(
            "ts={timestamp_secs} type=remote ip={} host={host} conns={} rx={rx} tx={tx}",
            remote.ip, remote.connection_count
        ));
    }

    lines.join("\n")
}

/// Serializes a `NetSnapshot` to a single-line JSON string for `--json` mode.
pub fn format_json_snapshot(
    snapshot: &NetSnapshot,
    timestamp_secs: u64,
    cumulative: bool,
    no_resolve: bool,
) -> Result<String, serde_json::Error> {
    let (mode, fallback_reason) = match &snapshot.status {
        CaptureStatus::Live => ("live", Value::Null),
        CaptureStatus::ProcfsFallback(reason) => (
            "procfs_fallback",
            Value::String(sanitize_control_chars(reason)),
        ),
    };

    let interfaces: Vec<Value> = snapshot
        .interfaces
        .iter()
        .map(|iface| {
            json!({
                "name": sanitize_control_chars(&iface.name),
                "is_up": iface.is_up,
                "rx_bytes": iface.counters.rx_bytes,
                "tx_bytes": iface.counters.tx_bytes,
                "rx_packets": iface.counters.rx_packets,
                "tx_packets": iface.counters.tx_packets,
            })
        })
        .collect();

    let processes: Vec<Value> = snapshot
        .processes
        .iter()
        .map(|proc_item| {
            json!({
                "pid": proc_item.process.pid,
                "name": sanitize_control_chars(&proc_item.process.name),
                "cmdline": sanitize_control_chars(&proc_item.process.cmdline),
                "uid": proc_item.process.uid,
                "connection_count": proc_item.connection_count,
                "rx_bytes": proc_item.counters.rx_bytes,
                "tx_bytes": proc_item.counters.tx_bytes,
                "rx_packets": proc_item.counters.rx_packets,
                "tx_packets": proc_item.counters.tx_packets,
            })
        })
        .collect();

    let connections: Vec<Value> = snapshot
        .connections
        .iter()
        .map(|conn| {
            let hostname = if no_resolve {
                Value::Null
            } else {
                conn.hostname
                    .as_deref()
                    .map(|h| Value::String(sanitize_control_chars(h)))
                    .unwrap_or(Value::Null)
            };
            let pid = conn
                .process
                .as_ref()
                .map_or(Value::Null, |p| Value::from(p.pid));
            let process_name = conn.process.as_ref().map_or(Value::Null, |p| {
                Value::String(sanitize_control_chars(&p.name))
            });

            json!({
                "protocol": conn.key.protocol.to_string(),
                "local_ip": conn.key.local.ip.to_string(),
                "local_port": conn.key.local.port,
                "remote_ip": conn.key.remote.ip.to_string(),
                "remote_port": conn.key.remote.port,
                "hostname": hostname,
                "pid": pid,
                "process_name": process_name,
                "rx_bytes": conn.counters.rx_bytes,
                "tx_bytes": conn.counters.tx_bytes,
                "rx_packets": conn.counters.rx_packets,
                "tx_packets": conn.counters.tx_packets,
            })
        })
        .collect();

    let remotes: Vec<Value> = snapshot
        .remotes
        .iter()
        .map(|remote| {
            let hostname = if no_resolve {
                Value::Null
            } else {
                remote
                    .hostname
                    .as_deref()
                    .map(|h| Value::String(sanitize_control_chars(h)))
                    .unwrap_or(Value::Null)
            };
            json!({
                "ip": remote.ip.to_string(),
                "hostname": hostname,
                "connection_count": remote.connection_count,
                "rx_bytes": remote.counters.rx_bytes,
                "tx_bytes": remote.counters.tx_bytes,
                "rx_packets": remote.counters.rx_packets,
                "tx_packets": remote.counters.tx_packets,
            })
        })
        .collect();

    let payload = json!({
        "timestamp": timestamp_secs,
        "interval_secs": snapshot.interval_secs,
        "mode": mode,
        "fallback_reason": fallback_reason,
        "cumulative": cumulative,
        "total": {
            "rx_bytes": snapshot.total.rx_bytes,
            "tx_bytes": snapshot.total.tx_bytes,
            "rx_packets": snapshot.total.rx_packets,
            "tx_packets": snapshot.total.tx_packets,
        },
        "interfaces": interfaces,
        "processes": processes,
        "connections": connections,
        "remotes": remotes,
    });

    serde_json::to_string(&payload)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::model::{
        ConnectionKey, ConnectionTraffic, InterfaceTraffic, LocalSocket, ProcessInfo,
        ProcessTraffic, Protocol, RemoteSocket, RemoteTraffic, TrafficCounters,
    };

    fn fixture_snapshot() -> NetSnapshot {
        let local_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let remote_ip = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        NetSnapshot {
            interval_secs: 1.0,
            status: CaptureStatus::Live,
            total: TrafficCounters {
                rx_bytes: 2048,
                tx_bytes: 1024,
                rx_packets: 12,
                tx_packets: 8,
            },
            interfaces: vec![InterfaceTraffic {
                name: "eth0".to_string(),
                counters: TrafficCounters {
                    rx_bytes: 2048,
                    tx_bytes: 1024,
                    rx_packets: 12,
                    tx_packets: 8,
                },
                is_up: true,
            }],
            processes: vec![ProcessTraffic {
                process: ProcessInfo {
                    pid: 4242,
                    name: "curl\x1b[31m".to_string(),
                    cmdline: "curl https://example.com".to_string(),
                    uid: 1000,
                },
                counters: TrafficCounters {
                    rx_bytes: 2048,
                    tx_bytes: 1024,
                    rx_packets: 12,
                    tx_packets: 8,
                },
                connection_count: 1,
            }],
            connections: vec![ConnectionTraffic {
                key: ConnectionKey {
                    protocol: Protocol::Tcp,
                    local: LocalSocket {
                        ip: local_ip,
                        port: 49152,
                    },
                    remote: RemoteSocket {
                        ip: remote_ip,
                        port: 443,
                    },
                },
                process: Some(ProcessInfo {
                    pid: 4242,
                    name: "curl".to_string(),
                    cmdline: "curl https://example.com".to_string(),
                    uid: 1000,
                }),
                hostname: Some("example.com".to_string()),
                counters: TrafficCounters {
                    rx_bytes: 2048,
                    tx_bytes: 1024,
                    rx_packets: 12,
                    tx_packets: 8,
                },
            }],
            remotes: vec![RemoteTraffic {
                ip: remote_ip,
                hostname: Some("example.com".to_string()),
                counters: TrafficCounters {
                    rx_bytes: 2048,
                    tx_bytes: 1024,
                    rx_packets: 12,
                    tx_packets: 8,
                },
                connection_count: 1,
            }],
        }
    }

    #[test]
    fn raw_output_formats_all_sections_and_sanitizes_control_chars() {
        let snap = fixture_snapshot();
        let text = format_raw_snapshot(&snap, 1_700_000_000, UnitFamily::BinBytes, false, false);

        assert!(text.contains("ts=1700000000 type=summary mode=LIVE rx=2.0 KiB/s tx=1.0 KiB/s"));
        assert!(text.contains("type=interface name=eth0 up=true rx=2.0 KiB/s tx=1.0 KiB/s"));
        assert!(text.contains("type=process pid=4242 name=curl [31m conns=1"));
        assert!(!text.contains('\x1b'));
        assert!(
            text.contains("type=connection proto=TCP local=10.0.0.2:49152 remote=example.com:443")
        );
        assert!(text.contains("type=remote ip=93.184.216.34 host=example.com conns=1"));
    }

    #[test]
    fn raw_output_respects_cumulative_and_no_resolve() {
        let mut snap = fixture_snapshot();
        snap.status = CaptureStatus::ProcfsFallback("permission denied".to_string());

        let text = format_raw_snapshot(&snap, 42, UnitFamily::SiBytes, true, true);
        assert!(text.contains("mode=PROCFS-ONLY"));
        assert!(text.contains("reason=\"permission denied\""));
        assert!(text.contains("rx=2.0 kB tx=1.0 kB"));
        assert!(text.contains("remote=93.184.216.34:443"));
        assert!(text.contains("host=-"));
    }

    #[test]
    fn json_output_serializes_valid_ndjson_with_all_fields() {
        let snap = fixture_snapshot();
        let raw_json =
            format_json_snapshot(&snap, 1_700_000_000, false, false).expect("json should format");
        assert!(!raw_json.contains('\n'));

        let parsed: Value = serde_json::from_str(&raw_json).expect("json should parse");
        assert_eq!(parsed["timestamp"], 1_700_000_000);
        assert_eq!(parsed["mode"], "live");
        assert_eq!(parsed["total"]["rx_bytes"], 2048);
        assert_eq!(parsed["interfaces"][0]["name"], "eth0");
        assert_eq!(parsed["processes"][0]["pid"], 4242);
        assert_eq!(parsed["processes"][0]["name"], "curl [31m");
        assert_eq!(parsed["connections"][0]["hostname"], "example.com");
        assert_eq!(parsed["remotes"][0]["ip"], "93.184.216.34");
    }

    #[test]
    fn json_output_respects_no_resolve_and_fallback() {
        let mut snap = fixture_snapshot();
        snap.status = CaptureStatus::ProcfsFallback("EPERM".to_string());
        let raw_json = format_json_snapshot(&snap, 10, true, true).expect("json should format");
        let parsed: Value = serde_json::from_str(&raw_json).expect("json should parse");

        assert_eq!(parsed["mode"], "procfs_fallback");
        assert_eq!(parsed["fallback_reason"], "EPERM");
        assert_eq!(parsed["cumulative"], true);
        assert!(parsed["connections"][0]["hostname"].is_null());
        assert!(parsed["remotes"][0]["hostname"].is_null());
    }
}
