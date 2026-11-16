use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use rs_common::sanitize_control_chars;

use crate::model::{ConnectionKey, LocalSocket, ProcessInfo, Protocol, RemoteSocket};

/// Parsed row from `/proc/net/{tcp,tcp6,udp,udp6}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketEntry {
    pub protocol: Protocol,
    pub local_ip: IpAddr,
    pub local_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub state: u8,
    pub uid: u32,
    pub inode: u64,
}

/// Parsed interface byte and packet counters from `/proc/net/dev`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCounters {
    pub name: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
}

/// Normalizes IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) to `IpAddr::V4`.
fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
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
    Some(normalize_ip(IpAddr::V6(Ipv6Addr::from(bytes))))
}

fn parse_endpoint(token: &str, is_v6: bool) -> Option<(IpAddr, u16)> {
    let (ip_hex, port_hex) = token.split_once(':')?;
    let ip = if is_v6 {
        parse_hex_ipv6(ip_hex)?
    } else {
        parse_hex_ipv4(ip_hex)?
    };
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    Some((ip, port))
}

/// Parses the contents of `/proc/net/{tcp,tcp6,udp,udp6}` into `SocketEntry` records.
///
/// Malformed lines, header lines, and sockets with `inode == 0` are skipped.
pub fn parse_proc_net_table(content: &str, protocol: Protocol, is_v6: bool) -> Vec<SocketEntry> {
    let mut entries = Vec::new();

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
        // Skip tx_queue:rx_queue, tr:tm->when, retrnsmt.
        let Some(_queues) = cols.next() else {
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

        let Some((local_ip, local_port)) = parse_endpoint(local_tok, is_v6) else {
            continue;
        };
        let Some((remote_ip, remote_port)) = parse_endpoint(remote_tok, is_v6) else {
            continue;
        };
        let Ok(state) = u8::from_str_radix(state_tok, 16) else {
            continue;
        };
        let Ok(uid) = uid_tok.parse::<u32>() else {
            continue;
        };
        let Ok(inode) = inode_tok.parse::<u64>() else {
            continue;
        };
        if inode == 0 {
            continue;
        }

        entries.push(SocketEntry {
            protocol,
            local_ip,
            local_port,
            remote_ip,
            remote_port,
            state,
            uid,
            inode,
        });
    }

    entries
}

/// Parses `/proc/net/dev` into per-interface rx/tx byte and packet counters.
pub fn parse_proc_net_dev(content: &str) -> Vec<InterfaceCounters> {
    let mut out = Vec::new();

    for line in content.lines().skip(2) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((raw_name, rest)) = trimmed.split_once(':') else {
            continue;
        };
        let name = sanitize_control_chars(raw_name.trim());
        if name.is_empty() {
            continue;
        }

        let cols: Vec<&str> = rest.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }

        let (Ok(rx_bytes), Ok(rx_packets), Ok(tx_bytes), Ok(tx_packets)) = (
            cols[0].parse::<u64>(),
            cols[1].parse::<u64>(),
            cols[8].parse::<u64>(),
            cols[9].parse::<u64>(),
        ) else {
            continue;
        };

        out.push(InterfaceCounters {
            name,
            rx_bytes,
            rx_packets,
            tx_bytes,
            tx_packets,
        });
    }

    out
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

/// Scans `proc_root` for numeric PID directories and maps socket inodes to owning `ProcessInfo`.
///
/// Vanished PIDs and permission errors (`EACCES`) are ignored gracefully.
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

        let raw_comm = match fs::read_to_string(pid_dir.join("comm")) {
            Ok(s) => s,
            Err(_) => continue,
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

/// Snapshot of active procfs sockets and their attributed owning processes.
#[derive(Debug, Clone, Default)]
pub struct SocketTable {
    entries: Vec<SocketEntry>,
    inode_to_process: HashMap<u64, ProcessInfo>,
    exact: HashMap<ConnectionKey, u64>,
    local: HashMap<(Protocol, IpAddr, u16), u64>,
    wildcard: HashMap<(Protocol, IpAddr, u16), u64>,
}

impl SocketTable {
    /// Builds a `SocketTable` by reading `/proc/net/{tcp,tcp6,udp,udp6}` and scanning PIDs under `proc_root`.
    pub fn from_proc_root(proc_root: &Path) -> Self {
        let mut entries = Vec::new();
        let net_dir = proc_root.join("net");
        let sources = [
            ("tcp", Protocol::Tcp, false),
            ("tcp6", Protocol::Tcp, true),
            ("udp", Protocol::Udp, false),
            ("udp6", Protocol::Udp, true),
        ];

        for (file, proto, is_v6) in sources {
            if let Ok(content) = fs::read_to_string(net_dir.join(file)) {
                entries.extend(parse_proc_net_table(&content, proto, is_v6));
            }
        }

        let inode_to_process = scan_process_sockets(proc_root);
        let mut exact = HashMap::new();
        let mut local = HashMap::new();
        let mut wildcard = HashMap::new();

        for entry in &entries {
            if !inode_to_process.contains_key(&entry.inode) {
                continue;
            }

            let key = ConnectionKey {
                protocol: entry.protocol,
                local: LocalSocket {
                    ip: entry.local_ip,
                    port: entry.local_port,
                },
                remote: RemoteSocket {
                    ip: entry.remote_ip,
                    port: entry.remote_port,
                },
            };
            exact.entry(key).or_insert(entry.inode);

            let is_unbound_remote = entry.remote_port == 0 && entry.remote_ip.is_unspecified();
            let local_key = (entry.protocol, entry.local_ip, entry.local_port);

            if entry.local_ip.is_unspecified() {
                if is_unbound_remote || !wildcard.contains_key(&local_key) {
                    wildcard.insert(local_key, entry.inode);
                }
            } else if is_unbound_remote || !local.contains_key(&local_key) {
                local.insert(local_key, entry.inode);
            }
        }

        Self {
            entries,
            inode_to_process,
            exact,
            local,
            wildcard,
        }
    }

    /// Looks up the owning process for a connection using exact 5-tuple, local endpoint, then wildcard fallback.
    pub fn lookup_connection(
        &self,
        proto: Protocol,
        local_ip: IpAddr,
        local_port: u16,
        remote_ip: IpAddr,
        remote_port: u16,
    ) -> Option<&ProcessInfo> {
        let local_ip = normalize_ip(local_ip);
        let remote_ip = normalize_ip(remote_ip);

        let exact_key = ConnectionKey {
            protocol: proto,
            local: LocalSocket {
                ip: local_ip,
                port: local_port,
            },
            remote: RemoteSocket {
                ip: remote_ip,
                port: remote_port,
            },
        };
        if let Some(inode) = self.exact.get(&exact_key) {
            return self.inode_to_process.get(inode);
        }

        if let Some(inode) = self.local.get(&(proto, local_ip, local_port)) {
            return self.inode_to_process.get(inode);
        }

        let unspec = match local_ip {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        };
        if let Some(inode) = self.wildcard.get(&(proto, unspec, local_port)) {
            return self.inode_to_process.get(inode);
        }

        // Dual-stack IPv6 wildcard sockets (`[::]:port`) also accept IPv4 traffic on Linux.
        if local_ip.is_ipv4()
            && let Some(inode) =
                self.wildcard
                    .get(&(proto, IpAddr::V6(Ipv6Addr::UNSPECIFIED), local_port))
        {
            return self.inode_to_process.get(inode);
        }

        None
    }

    /// Returns all parsed procfs socket entries paired with their attributed process, if known.
    pub fn active_connections(&self) -> Vec<(&SocketEntry, Option<&ProcessInfo>)> {
        self.entries
            .iter()
            .map(|entry| (entry, self.inode_to_process.get(&entry.inode)))
            .collect()
    }
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
                "rsnet_{prefix}_{}_{}",
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
    fn parse_proc_net_tcp_and_udp_v4_handles_valid_and_malformed_lines() {
        let tcp_fixture = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0
   1: 0101A8C0:9C40 08080808:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 67890 1 0000000000000000 20 4 30 10 -1
   2: 00000000:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 0 1 0000000000000000
   3: malformed_line_without_enough_columns
   4: ZZZZZZZZ:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 99999
";

        let entries = parse_proc_net_table(tcp_fixture, Protocol::Tcp, false);
        assert_eq!(entries.len(), 2);

        assert_eq!(
            entries[0],
            SocketEntry {
                protocol: Protocol::Tcp,
                local_ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                local_port: 53,
                remote_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                remote_port: 0,
                state: 0x0A,
                uid: 0,
                inode: 12345,
            }
        );

        assert_eq!(
            entries[1],
            SocketEntry {
                protocol: Protocol::Tcp,
                local_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                local_port: 40000,
                remote_ip: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                remote_port: 443,
                state: 0x01,
                uid: 1000,
                inode: 67890,
            }
        );
    }

    #[test]
    fn parse_proc_net_v6_normalizes_ipv4_mapped_addresses() {
        let tcp6_fixture = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0277 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 23456 1 0000000000000000 100 0 0 10 0
   1: 0000000000000000FFFF00000100007F:1F90 0000000000000000FFFF00000200007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 34567 1 0000000000000000 20 0 0 10 -1
";

        let entries = parse_proc_net_table(tcp6_fixture, Protocol::Tcp, true);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].local_ip, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(entries[0].local_port, 631);
        assert_eq!(entries[0].remote_ip, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(entries[0].inode, 23456);

        assert_eq!(entries[1].local_ip, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(entries[1].local_port, 8080);
        assert_eq!(
            entries[1].remote_ip,
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))
        );
        assert_eq!(entries[1].remote_port, 54321);
        assert_eq!(entries[1].inode, 34567);
    }

    #[test]
    fn parse_proc_net_dev_parses_rx_and_tx_counters() {
        let dev_fixture = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1234567   12345    0    0    0     0          0         0  7654321   54321    0    0    0     0       0          0
  eth0:9876543210  654321   1    2    0     0          0         0 123456789  123456   0    0    0     0       0          0
  bad_line_without_colon 1 2 3
";

        let devs = parse_proc_net_dev(dev_fixture);
        assert_eq!(devs.len(), 2);
        assert_eq!(
            devs[0],
            InterfaceCounters {
                name: "lo".to_string(),
                rx_bytes: 1_234_567,
                rx_packets: 12_345,
                tx_bytes: 7_654_321,
                tx_packets: 54_321,
            }
        );
        assert_eq!(
            devs[1],
            InterfaceCounters {
                name: "eth0".to_string(),
                rx_bytes: 9_876_543_210,
                rx_packets: 654_321,
                tx_bytes: 123_456_789,
                tx_packets: 123_456,
            }
        );
    }

    #[test]
    fn scan_process_sockets_attributes_symlinks_and_sanitizes_strings() {
        let tmp = TempDir::new("scan");
        let proc_root = &tmp.path;

        // Non-numeric dir should be ignored.
        fs::create_dir_all(proc_root.join("net")).expect("create net dir");

        // PID 101: owns socket:[1001] and pipe:[9999], has control chars in comm/cmdline.
        let pid101 = proc_root.join("101");
        fs::create_dir_all(pid101.join("fd")).expect("create fd dir");
        symlink("socket:[1001]", pid101.join("fd/3")).expect("symlink socket");
        symlink("pipe:[9999]", pid101.join("fd/4")).expect("symlink pipe");
        fs::write(pid101.join("comm"), "curl\x1b[31m\n").expect("write comm");
        fs::write(
            pid101.join("cmdline"),
            b"/usr/bin/curl\0https://example.com\x07\0",
        )
        .expect("write cmdline");
        fs::write(
            pid101.join("status"),
            "Name:\tcurl\nUid:\t1000\t1000\t1000\t1000\n",
        )
        .expect("write status");

        // PID 202: has no sockets, should not appear.
        let pid202 = proc_root.join("202");
        fs::create_dir_all(pid202.join("fd")).expect("create fd dir");
        symlink("/dev/null", pid202.join("fd/0")).expect("symlink dev null");
        fs::write(pid202.join("comm"), "sleep\n").expect("write comm");

        // PID 303: missing fd dir (simulates EACCES or vanished PID).
        fs::create_dir_all(proc_root.join("303")).expect("create 303 dir");

        let map = scan_process_sockets(proc_root);
        assert_eq!(map.len(), 1);
        let p = map.get(&1001).expect("inode 1001 present");
        assert_eq!(p.pid, 101);
        assert_eq!(p.name, "curl [31m");
        assert_eq!(p.cmdline, "/usr/bin/curl https://example.com ");
        assert_eq!(p.uid, 1000);
    }

    #[test]
    fn socket_table_lookup_supports_exact_local_wildcard_and_active_connections() {
        let tmp = TempDir::new("table");
        let proc_root = &tmp.path;
        let net_dir = proc_root.join("net");
        fs::create_dir_all(&net_dir).expect("create net dir");

        // 1) Exact TCP established: 192.168.1.10:50000 -> 93.184.216.34:443 (inode 1111)
        // 2) Local-bound UDP socket: 192.168.1.10:5353 -> 0.0.0.0:0 (inode 2222)
        // 3) Wildcard TCP listener: 0.0.0.0:8080 -> 0.0.0.0:0 (inode 3333)
        // 4) Unowned TCP socket: 127.0.0.1:9000 -> 127.0.0.1:9001 (inode 4444)
        let tcp = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0A01A8C0:C350 22D8B85D:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 1111 1 0000000000000000
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1001        0 3333 1 0000000000000000
   2: 0100007F:2328 0100007F:2329 01 00000000:00000000 00:00000000 00000000     0        0 4444 1 0000000000000000
";
        let udp = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0A01A8C0:14E9 00000000:0000 07 00000000:00000000 00:00000000 00000000  1002        0 2222 1 0000000000000000
";
        // Dual-stack IPv6 wildcard listener on [::]:8443 (inode 5555)
        let tcp6 = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:20FB 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1003        0 5555 1 0000000000000000
";
        fs::write(net_dir.join("tcp"), tcp).expect("write tcp");
        fs::write(net_dir.join("udp"), udp).expect("write udp");
        fs::write(net_dir.join("tcp6"), tcp6).expect("write tcp6");

        let add_pid = |pid: u32, name: &str, uid: u32, inode: u64| {
            let pdir = proc_root.join(pid.to_string());
            fs::create_dir_all(pdir.join("fd")).expect("create fd");
            symlink(format!("socket:[{inode}]"), pdir.join("fd/3")).expect("symlink");
            fs::write(pdir.join("comm"), format!("{name}\n")).expect("write comm");
            fs::write(pdir.join("cmdline"), format!("/usr/bin/{name}\0")).expect("write cmdline");
            fs::write(
                pdir.join("status"),
                format!("Uid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .expect("write status");
        };

        add_pid(10, "firefox", 1000, 1111);
        add_pid(20, "avahi", 1002, 2222);
        add_pid(30, "nginx", 1001, 3333);
        add_pid(40, "caddy", 1003, 5555);

        let table = SocketTable::from_proc_root(proc_root);

        // Exact 5-tuple match (also via IPv4-mapped IPv6 address)
        let local_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let remote_ip = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        assert_eq!(
            table
                .lookup_connection(Protocol::Tcp, local_ip, 50000, remote_ip, 443)
                .map(|p| p.name.as_str()),
            Some("firefox")
        );
        let mapped_local = IpAddr::V6("::ffff:192.168.1.10".parse().expect("mapped v6"));
        let mapped_remote = IpAddr::V6("::ffff:93.184.216.34".parse().expect("mapped v6"));
        assert_eq!(
            table
                .lookup_connection(Protocol::Tcp, mapped_local, 50000, mapped_remote, 443)
                .map(|p| p.name.as_str()),
            Some("firefox")
        );

        // Local endpoint fallback match
        assert_eq!(
            table
                .lookup_connection(Protocol::Udp, local_ip, 5353, remote_ip, 5353)
                .map(|p| p.name.as_str()),
            Some("avahi")
        );

        // Wildcard fallback match (0.0.0.0:8080)
        assert_eq!(
            table
                .lookup_connection(Protocol::Tcp, local_ip, 8080, remote_ip, 51234)
                .map(|p| p.name.as_str()),
            Some("nginx")
        );

        // Dual-stack IPv6 wildcard fallback match ([::]:8443 for IPv4 packet)
        assert_eq!(
            table
                .lookup_connection(Protocol::Tcp, local_ip, 8443, remote_ip, 51235)
                .map(|p| p.name.as_str()),
            Some("caddy")
        );

        // Unowned / unknown connection returns None
        assert!(
            table
                .lookup_connection(Protocol::Tcp, local_ip, 12345, remote_ip, 80)
                .is_none()
        );

        // active_connections includes all 5 parsed sockets, with 4 attributed and 1 unattributed
        let active = table.active_connections();
        assert_eq!(active.len(), 5);
        let attributed_count = active.iter().filter(|(_, p)| p.is_some()).count();
        assert_eq!(attributed_count, 4);
    }
}
