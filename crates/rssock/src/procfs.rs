use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use rs_common::sanitize_control_chars;

use crate::model::{
    Endpoint, ProcessInfo, Protocol, SocketKind, SocketRecord, SocketState, UnixSocketType,
};

/// Options controlling which socket families and metadata are collected from procfs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectOptions {
    pub tcp: bool,
    pub udp: bool,
    pub unix: bool,
    pub with_processes: bool,
}

impl Default for CollectOptions {
    fn default() -> Self {
        Self {
            tcp: true,
            udp: true,
            unix: true,
            with_processes: true,
        }
    }
}

fn parse_hex_ipv4(hex: &str) -> Option<IpAddr> {
    if hex.len() != 8 {
        return None;
    }
    let word = u32::from_str_radix(hex, 16).ok()?;
    Some(IpAddr::V4(Ipv4Addr::from(word.to_le_bytes())))
}

fn parse_hex_ipv6(hex: &str) -> Option<IpAddr> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for i in 0..4 {
        let start = i * 8;
        let word = u32::from_str_radix(&hex[start..start + 8], 16).ok()?;
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    Some(IpAddr::V6(Ipv6Addr::from(bytes)))
}

fn parse_endpoint(token: &str, is_v6: bool) -> Option<Endpoint> {
    let (ip_hex, port_hex) = token.split_once(':')?;
    let ip = if is_v6 {
        parse_hex_ipv6(ip_hex)?
    } else {
        parse_hex_ipv4(ip_hex)?
    };
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    Some(Endpoint { ip, port })
}

fn parse_queues(token: &str) -> Option<(u32, u32)> {
    let (tx_hex, rx_hex) = token.split_once(':')?;
    let send_q = u32::from_str_radix(tx_hex, 16).ok()?;
    let recv_q = u32::from_str_radix(rx_hex, 16).ok()?;
    Some((send_q, recv_q))
}

/// Parses `/proc/net/{tcp,tcp6,udp,udp6}` content into [`SocketRecord`] entries.
///
/// Header and malformed lines are skipped. Sockets with `inode == 0` (such as
/// `TIME-WAIT` sockets) are preserved.
#[must_use]
pub fn parse_proc_net_inet(content: &str, protocol: Protocol, is_v6: bool) -> Vec<SocketRecord> {
    let mut records = Vec::new();

    for line in content.lines() {
        let mut cols = line.split_whitespace();
        let Some(_sl) = cols.next() else {
            continue;
        };
        let Some(local_tok) = cols.next() else {
            continue;
        };
        let Some(remote_tok) = cols.next() else {
            continue;
        };
        let Some(state_tok) = cols.next() else {
            continue;
        };
        let Some(queues_tok) = cols.next() else {
            continue;
        };
        let Some(_timer) = cols.next() else {
            continue;
        };
        let Some(_retrans) = cols.next() else {
            continue;
        };
        let Some(uid_tok) = cols.next() else {
            continue;
        };
        let Some(_timeout) = cols.next() else {
            continue;
        };
        let Some(inode_tok) = cols.next() else {
            continue;
        };

        let Some(local) = parse_endpoint(local_tok, is_v6) else {
            continue;
        };
        let Some(remote) = parse_endpoint(remote_tok, is_v6) else {
            continue;
        };
        let Ok(raw_state) = u8::from_str_radix(state_tok, 16) else {
            continue;
        };
        let Some((send_q, recv_q)) = parse_queues(queues_tok) else {
            continue;
        };
        let Ok(uid) = uid_tok.parse::<u32>() else {
            continue;
        };
        let Ok(inode) = inode_tok.parse::<u64>() else {
            continue;
        };

        records.push(SocketRecord {
            protocol,
            state: SocketState::from_inet_state(protocol, raw_state),
            recv_q,
            send_q,
            kind: SocketKind::Inet { local, remote },
            uid,
            inode,
            process: None,
        });
    }

    records
}

fn next_token(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.find(|c: char| c.is_ascii_whitespace()) {
        Some(idx) => Some((&trimmed[..idx], &trimmed[idx..])),
        None => Some((trimmed, "")),
    }
}

/// Parses `/proc/net/unix` content into [`SocketRecord`] entries.
///
/// Header and malformed lines are skipped. Socket paths (including spaces and
/// `@abstract` names) are preserved and sanitized with [`sanitize_control_chars`].
#[must_use]
pub fn parse_proc_net_unix(content: &str) -> Vec<SocketRecord> {
    let mut records = Vec::new();

    for line in content.lines() {
        let Some((_num, rest)) = next_token(line) else {
            continue;
        };
        let Some((ref_count_tok, rest)) = next_token(rest) else {
            continue;
        };
        let Some((proto_tok, rest)) = next_token(rest) else {
            continue;
        };
        let Some((flags_tok, rest)) = next_token(rest) else {
            continue;
        };
        let Some((type_tok, rest)) = next_token(rest) else {
            continue;
        };
        let Some((state_tok, rest)) = next_token(rest) else {
            continue;
        };
        let Some((inode_tok, rest)) = next_token(rest) else {
            continue;
        };

        let Ok(ref_count) = u32::from_str_radix(ref_count_tok, 16) else {
            continue;
        };
        if u32::from_str_radix(proto_tok, 16).is_err() {
            continue;
        }
        let Ok(flags) = u32::from_str_radix(flags_tok, 16) else {
            continue;
        };
        let Ok(raw_type) = u16::from_str_radix(type_tok, 16) else {
            continue;
        };
        let Ok(raw_state) = u8::from_str_radix(state_tok, 16) else {
            continue;
        };
        let Ok(inode) = inode_tok.parse::<u64>() else {
            continue;
        };

        let raw_path = rest
            .trim_start_matches(|c: char| c.is_ascii_whitespace())
            .trim_end_matches(['\r', '\n']);
        let path = if raw_path.is_empty() {
            None
        } else {
            Some(sanitize_control_chars(raw_path))
        };

        records.push(SocketRecord {
            protocol: Protocol::Unix,
            state: SocketState::from_unix_state(raw_state, flags),
            recv_q: 0,
            send_q: 0,
            kind: SocketKind::Unix {
                unix_type: UnixSocketType::from_raw(raw_type),
                flags,
                ref_count,
                path,
            },
            uid: 0,
            inode,
            process: None,
        });
    }

    records
}

fn parse_socket_inode(target: &Path) -> Option<u64> {
    let s = target.to_str()?;
    let inner = s.strip_prefix("socket:[")?.strip_suffix(']')?;
    inner.parse::<u64>().ok().filter(|&inode| inode != 0)
}

fn parse_status_uid(status_content: &str) -> Option<u32> {
    for line in status_content.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            return rest.split_whitespace().next()?.parse::<u32>().ok();
        }
    }
    None
}

fn read_process_uid(pid_dir: &Path) -> u32 {
    if let Ok(status) = fs::read_to_string(pid_dir.join("status"))
        && let Some(uid) = parse_status_uid(&status)
    {
        return uid;
    }

    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(pid_dir) {
        use std::os::unix::fs::MetadataExt;
        return meta.uid();
    }

    0
}

fn format_cmdline(raw: &[u8], fallback_name: &str) -> String {
    let joined = raw
        .split(|&b| b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        fallback_name.to_string()
    } else {
        sanitize_control_chars(trimmed)
    }
}

/// Scans `proc_root` for numeric PID directories and maps socket inodes to [`ProcessInfo`].
///
/// Vanished PIDs and permission errors (`EACCES`) are ignored gracefully.
#[must_use]
pub fn scan_process_sockets(proc_root: &Path) -> HashMap<u64, ProcessInfo> {
    let mut inode_map = HashMap::new();
    let Ok(entries) = fs::read_dir(proc_root) else {
        return inode_map;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid_str) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };

        let pid_dir = entry.path();
        let Ok(fd_entries) = fs::read_dir(pid_dir.join("fd")) else {
            continue;
        };

        let mut pid_inodes = Vec::new();
        for fd_entry in fd_entries.flatten() {
            if let Ok(target) = fs::read_link(fd_entry.path())
                && let Some(inode) = parse_socket_inode(&target)
            {
                pid_inodes.push(inode);
            }
        }

        if pid_inodes.is_empty() {
            continue;
        }

        let Ok(raw_comm) = fs::read_to_string(pid_dir.join("comm")) else {
            continue;
        };
        let name = sanitize_control_chars(raw_comm.trim_end_matches(['\r', '\n']));
        let cmdline = match fs::read(pid_dir.join("cmdline")) {
            Ok(bytes) => format_cmdline(&bytes, &name),
            Err(_) => name.clone(),
        };
        let uid = read_process_uid(&pid_dir);

        let info = ProcessInfo {
            pid,
            name,
            cmdline,
            uid,
        };

        for inode in pid_inodes {
            inode_map.insert(inode, info.clone());
        }
    }

    inode_map
}

/// Collects socket records from a procfs root directory according to `opts`.
#[must_use]
pub fn collect_sockets(proc_root: &Path, opts: CollectOptions) -> Vec<SocketRecord> {
    let mut records = Vec::new();

    if opts.tcp {
        if let Ok(content) = fs::read_to_string(proc_root.join("net/tcp")) {
            records.extend(parse_proc_net_inet(&content, Protocol::Tcp, false));
        }
        if let Ok(content) = fs::read_to_string(proc_root.join("net/tcp6")) {
            records.extend(parse_proc_net_inet(&content, Protocol::Tcp6, true));
        }
    }

    if opts.udp {
        if let Ok(content) = fs::read_to_string(proc_root.join("net/udp")) {
            records.extend(parse_proc_net_inet(&content, Protocol::Udp, false));
        }
        if let Ok(content) = fs::read_to_string(proc_root.join("net/udp6")) {
            records.extend(parse_proc_net_inet(&content, Protocol::Udp6, true));
        }
    }

    if opts.unix
        && let Ok(content) = fs::read_to_string(proc_root.join("net/unix"))
    {
        records.extend(parse_proc_net_unix(&content));
    }

    if opts.with_processes && !records.is_empty() {
        let proc_map = scan_process_sockets(proc_root);
        for rec in &mut records {
            if rec.inode != 0
                && let Some(info) = proc_map.get(&rec.inode)
            {
                rec.process = Some(info.clone());
            }
        }
    }

    records
}

/// Collects socket records from `/proc` on the local system.
#[must_use]
pub fn collect_system_sockets(opts: CollectOptions) -> Vec<SocketRecord> {
    collect_sockets(Path::new("/proc"), opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rssock_{prefix}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn parse_inet_v4_endianness_queues_and_time_wait_zero_inode() {
        let tcp_fixture = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0035 00000000:0000 0A 00000010:00000020 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0
   1: 0101A8C0:9C40 08080808:01BB 06 00000000:00000001 00:00000000 00000000  1000        0 0 1 0000000000000000 20 4 30 10 -1
   2: malformed line
";
        let records = parse_proc_net_inet(tcp_fixture, Protocol::Tcp, false);
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].protocol, Protocol::Tcp);
        assert_eq!(records[0].state, SocketState::Listen);
        assert_eq!(records[0].send_q, 0x10);
        assert_eq!(records[0].recv_q, 0x20);
        assert_eq!(records[0].uid, 0);
        assert_eq!(records[0].inode, 12345);
        assert_eq!(
            records[0].kind,
            SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    port: 53,
                },
                remote: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    port: 0,
                },
            }
        );

        assert_eq!(records[1].state, SocketState::TimeWait);
        assert_eq!(records[1].send_q, 0);
        assert_eq!(records[1].recv_q, 1);
        assert_eq!(records[1].uid, 1000);
        assert_eq!(records[1].inode, 0);
        assert_eq!(
            records[1].kind,
            SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                    port: 40000,
                },
                remote: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                    port: 443,
                },
            }
        );
    }

    #[test]
    fn parse_inet_v6_word_endianness_and_udp_unconn() {
        let udp6_fixture = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:14E9 00000000000000000000000000000000:0000 07 00000002:00000004 00:00000000 00000000   101        0 55555 2 0000000000000000 0
   1: B80D0120000000000000000001000000:01BB 00000000000000000000000000000000:0000 01 00000000:00000000 00:00000000 00000000  1000        0 55556 2 0000000000000000 0
";
        let records = parse_proc_net_inet(udp6_fixture, Protocol::Udp6, true);
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].protocol, Protocol::Udp6);
        assert_eq!(records[0].state, SocketState::Unconn);
        assert_eq!(records[0].send_q, 2);
        assert_eq!(records[0].recv_q, 4);
        assert_eq!(
            records[0].kind,
            SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
                    port: 5353,
                },
                remote: Endpoint {
                    ip: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    port: 0,
                },
            }
        );

        assert_eq!(records[1].state, SocketState::Established);
        assert_eq!(
            records[1].kind,
            SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V6("2001:db8::1".parse().expect("valid v6")),
                    port: 443,
                },
                remote: Endpoint {
                    ip: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    port: 0,
                },
            }
        );
    }

    #[test]
    fn parse_unix_handles_empty_spaces_abstract_listening_and_sanitization() {
        let unix_fixture = "\
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 10001 /run/my socket/app\x1b[31m.sock
0000000000000000: 00000003 00000000 00000000 0002 01 10002
0000000000000000: 00000002 00000000 00010000 0005 01 10003 @/tmp/.X11-unix/X0
0000000000000000: 00000002 00000000 00000000 0001 03 10004 /var/run/dbus/system_bus_socket
";
        let records = parse_proc_net_unix(unix_fixture);
        assert_eq!(records.len(), 4);

        assert_eq!(records[0].protocol, Protocol::Unix);
        assert_eq!(records[0].state, SocketState::Listen);
        assert_eq!(records[0].inode, 10001);
        assert_eq!(
            records[0].kind,
            SocketKind::Unix {
                unix_type: UnixSocketType::Stream,
                flags: 0x0001_0000,
                ref_count: 2,
                path: Some("/run/my socket/app [31m.sock".to_string()),
            }
        );

        assert_eq!(records[1].state, SocketState::Unconn);
        assert_eq!(records[1].inode, 10002);
        assert_eq!(
            records[1].kind,
            SocketKind::Unix {
                unix_type: UnixSocketType::Dgram,
                flags: 0,
                ref_count: 3,
                path: None,
            }
        );

        assert_eq!(records[2].state, SocketState::Listen);
        assert_eq!(records[2].inode, 10003);
        assert_eq!(
            records[2].kind,
            SocketKind::Unix {
                unix_type: UnixSocketType::SeqPacket,
                flags: 0x0001_0000,
                ref_count: 2,
                path: Some("@/tmp/.X11-unix/X0".to_string()),
            }
        );

        assert_eq!(records[3].state, SocketState::Connected);
        assert_eq!(records[3].inode, 10004);
    }

    #[test]
    fn collect_sockets_attributes_processes_and_sanitizes_control_chars() {
        let tmp = TempDir::new("collect");
        let net_dir = tmp.path.join("net");
        fs::create_dir_all(&net_dir).expect("create net dir");

        fs::write(
            net_dir.join("tcp"),
            "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 424242 1 0000000000000000 100 0 0 10 0
   1: 0100007F:1F91 0100007F:C350 06 00000000:00000000 00:00000000 00000000  1000        0 0 1 0000000000000000 100 0 0 10 0
",
        )
        .expect("write tcp");

        fs::write(
            net_dir.join("unix"),
            "\
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 424243 /tmp/app.sock
",
        )
        .expect("write unix");

        let pid_dir = tmp.path.join("1234");
        let fd_dir = pid_dir.join("fd");
        fs::create_dir_all(&fd_dir).expect("create fd dir");
        symlink("socket:[424242]", fd_dir.join("3")).expect("symlink 3");
        symlink("socket:[424243]", fd_dir.join("4")).expect("symlink 4");
        symlink("/dev/null", fd_dir.join("0")).expect("symlink 0");

        fs::write(pid_dir.join("comm"), "my\x1bapp\n").expect("write comm");
        fs::write(
            pid_dir.join("cmdline"),
            b"/usr/bin/my\x07app\0--port\08080\0",
        )
        .expect("write cmdline");
        fs::write(
            pid_dir.join("status"),
            "Name:\tmyapp\nUid:\t1000\t1000\t1000\t1000\n",
        )
        .expect("write status");

        // Non-PID directory and vanished/incomplete PID directory should be ignored.
        fs::create_dir_all(tmp.path.join("self")).expect("create self");
        fs::create_dir_all(tmp.path.join("9999/fd")).expect("create 9999/fd");
        symlink("socket:[777777]", tmp.path.join("9999/fd/3")).expect("symlink 9999");

        let records = collect_sockets(&tmp.path, CollectOptions::default());
        assert_eq!(records.len(), 3);

        let tcp_listen = records
            .iter()
            .find(|r| r.inode == 424242)
            .expect("tcp listen");
        let proc = tcp_listen.process.as_ref().expect("process info");
        assert_eq!(proc.pid, 1234);
        assert_eq!(proc.name, "my app");
        assert_eq!(proc.cmdline, "/usr/bin/my app --port 8080");
        assert_eq!(proc.uid, 1000);

        let tcp_tw = records.iter().find(|r| r.inode == 0).expect("time-wait");
        assert!(tcp_tw.process.is_none());

        let unix_sock = records
            .iter()
            .find(|r| r.inode == 424243)
            .expect("unix sock");
        assert_eq!(unix_sock.process.as_ref().map(|p| p.pid), Some(1234));

        // Verify selective protocol flags and with_processes = false.
        let unix_only = collect_sockets(
            &tmp.path,
            CollectOptions {
                tcp: false,
                udp: false,
                unix: true,
                with_processes: false,
            },
        );
        assert_eq!(unix_only.len(), 1);
        assert_eq!(unix_only[0].protocol, Protocol::Unix);
        assert!(unix_only[0].process.is_none());
    }
}
