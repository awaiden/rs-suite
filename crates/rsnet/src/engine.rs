use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::fs;
use std::mem::size_of;
use std::net::IpAddr;
use std::path::Path;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use rs_common::sanitize_control_chars;

use crate::cli::SortColumn;
use crate::model::{
    CaptureStatus, ConnectionKey, ConnectionTraffic, Direction, InterfaceTraffic, LocalSocket,
    NetSnapshot, ProcessInfo, ProcessTraffic, RemoteSocket, RemoteTraffic, TrafficCounters,
};
use crate::packet::ParsedPacket;
use crate::procfs::{InterfaceCounters, SocketTable, parse_proc_net_dev};

const WINDOW_SECS: usize = 5;
const MAX_DNS_CACHE_ENTRIES: usize = 4096;
const DNS_QUEUE_CAPACITY: usize = 256;

/// Returns the synthetic `ProcessInfo` used for traffic that cannot be attributed to a PID.
pub fn unknown_process() -> ProcessInfo {
    ProcessInfo {
        pid: 0,
        name: "<unknown>".to_string(),
        cmdline: String::new(),
        uid: 0,
    }
}

/// Non-blocking, bounded reverse-DNS cache backed by a background worker thread.
pub struct DnsCache {
    enabled: bool,
    cache: Arc<Mutex<HashMap<IpAddr, Option<String>>>>,
    tx: Option<SyncSender<IpAddr>>,
}

impl DnsCache {
    /// Creates a new `DnsCache`. When `no_resolve` is true, no worker thread is spawned.
    pub fn new(no_resolve: bool) -> Self {
        Self::with_resolver(!no_resolve, reverse_lookup_libc)
    }

    fn with_resolver<F>(enabled: bool, resolver: F) -> Self
    where
        F: Fn(IpAddr) -> Option<String> + Send + 'static,
    {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        if !enabled {
            return Self {
                enabled: false,
                cache,
                tx: None,
            };
        }

        let (tx, rx) = mpsc::sync_channel::<IpAddr>(DNS_QUEUE_CAPACITY);
        let worker_cache = Arc::clone(&cache);
        let _ = thread::Builder::new()
            .name("rsnet-dns".to_string())
            .spawn(move || {
                while let Ok(ip) = rx.recv() {
                    let resolved = resolver(ip).map(|s| sanitize_control_chars(&s));
                    if let Ok(mut guard) = worker_cache.lock() {
                        guard.insert(ip, resolved);
                    }
                }
            });

        Self {
            enabled: true,
            cache,
            tx: Some(tx),
        }
    }

    /// Returns a cached hostname if available, or queues a non-blocking background lookup.
    pub fn resolve(&self, ip: IpAddr) -> Option<String> {
        if !self.enabled || ip.is_unspecified() {
            return None;
        }

        let mut guard = self.cache.lock().ok()?;
        if let Some(entry) = guard.get(&ip) {
            return entry.clone();
        }

        if guard.len() >= MAX_DNS_CACHE_ENTRIES {
            return None;
        }

        // Insert a `None` placeholder so duplicate requests for the same IP are not queued.
        guard.insert(ip, None);
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(ip);
        }
        None
    }

    /// Returns the number of entries currently stored in the DNS cache.
    pub fn len(&self) -> usize {
        self.cache.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Returns true if the DNS cache has no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn reverse_lookup_libc(ip: IpAddr) -> Option<String> {
    let mut host = [0 as libc::c_char; libc::NI_MAXHOST as usize];
    let rc = match ip {
        IpAddr::V4(v4) => {
            let sa = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: 0,
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(v4.octets()),
                },
                sin_zero: [0; 8],
            };
            unsafe {
                libc::getnameinfo(
                    (&raw const sa).cast::<libc::sockaddr>(),
                    size_of::<libc::sockaddr_in>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
        IpAddr::V6(v6) => {
            let sa = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: 0,
                sin6_flowinfo: 0,
                sin6_addr: libc::in6_addr {
                    s6_addr: v6.octets(),
                },
                sin6_scope_id: 0,
            };
            unsafe {
                libc::getnameinfo(
                    (&raw const sa).cast::<libc::sockaddr>(),
                    size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
    };

    if rc != 0 {
        return None;
    }

    // SAFETY: `getnameinfo` succeeded and wrote a NUL-terminated string into `host`.
    let raw = unsafe { CStr::from_ptr(host.as_ptr()) }.to_string_lossy();
    let clean = sanitize_control_chars(&raw);
    if clean.is_empty() { None } else { Some(clean) }
}

#[derive(Debug, Clone, Default)]
struct TrafficBucket {
    sec: u64,
    total: TrafficCounters,
    connections: HashMap<ConnectionKey, TrafficCounters>,
}

#[derive(Debug, Clone, Default)]
struct InterfaceState {
    prev_raw: Option<InterfaceCounters>,
    cumulative: TrafficCounters,
    rate: TrafficCounters,
    is_up: bool,
}

/// Rolling 5-second bandwidth aggregation and process-attribution engine.
pub struct BandwidthEngine {
    start_time: Instant,
    interface_filter: Option<String>,
    status: CaptureStatus,
    dns: DnsCache,
    socket_table: SocketTable,
    buckets: VecDeque<TrafficBucket>,
    cumulative_total: TrafficCounters,
    cumulative_connections: HashMap<ConnectionKey, TrafficCounters>,
    connection_processes: HashMap<ConnectionKey, Option<ProcessInfo>>,
    interfaces: HashMap<String, InterfaceState>,
    last_iface_refresh: Option<Instant>,
}

impl BandwidthEngine {
    /// Creates a new `BandwidthEngine` anchored at `Instant::now()`.
    pub fn new(interface_filter: Option<String>, no_resolve: bool, status: CaptureStatus) -> Self {
        Self::with_start_time(interface_filter, no_resolve, status, Instant::now())
    }

    /// Creates a new `BandwidthEngine` anchored at an explicit `start_time` (useful for tests).
    pub fn with_start_time(
        interface_filter: Option<String>,
        no_resolve: bool,
        status: CaptureStatus,
        start_time: Instant,
    ) -> Self {
        Self {
            start_time,
            interface_filter,
            status,
            dns: DnsCache::new(no_resolve),
            socket_table: SocketTable::default(),
            buckets: VecDeque::with_capacity(WINDOW_SECS),
            cumulative_total: TrafficCounters::default(),
            cumulative_connections: HashMap::new(),
            connection_processes: HashMap::new(),
            interfaces: HashMap::new(),
            last_iface_refresh: None,
        }
    }

    /// Updates the capture status reported in snapshots.
    pub fn set_status(&mut self, status: CaptureStatus) {
        self.status = status;
    }

    /// Replaces the active `SocketTable` and refreshes cached connection process attributions.
    pub fn set_socket_table(&mut self, table: SocketTable) {
        self.socket_table = table;
        for (key, proc_slot) in &mut self.connection_processes {
            if let Some(found) = self.socket_table.lookup_connection(
                key.protocol,
                key.local.ip,
                key.local.port,
                key.remote.ip,
                key.remote.port,
            ) {
                *proc_slot = Some(found.clone());
            }
        }
    }

    /// Refreshes the `SocketTable` from `proc_root` (typically `/proc`).
    pub fn refresh_sockets(&mut self, proc_root: &Path) {
        let table = SocketTable::from_proc_root(proc_root);
        self.set_socket_table(table);
    }

    /// Reads `/proc/net/dev` under `proc_root` and updates interface deltas and rates.
    pub fn refresh_interfaces_from_proc(&mut self, proc_root: &Path, now: Instant) {
        if let Ok(content) = fs::read_to_string(proc_root.join("net/dev")) {
            let parsed = parse_proc_net_dev(&content);
            self.refresh_interfaces(parsed, now);
        }
    }

    /// Updates interface deltas and per-second rates from parsed `/proc/net/dev` counters.
    pub fn refresh_interfaces(&mut self, counters: Vec<InterfaceCounters>, now: Instant) {
        let dt_secs = self
            .last_iface_refresh
            .map(|prev| now.saturating_duration_since(prev).as_secs_f64().max(0.001))
            .unwrap_or(1.0);
        self.last_iface_refresh = Some(now);

        for entry in counters {
            if let Some(filter) = &self.interface_filter
                && &entry.name != filter
            {
                continue;
            }

            let state = self.interfaces.entry(entry.name.clone()).or_default();
            state.is_up = true;

            if let Some(prev) = &state.prev_raw {
                let delta = TrafficCounters {
                    rx_bytes: entry.rx_bytes.saturating_sub(prev.rx_bytes),
                    tx_bytes: entry.tx_bytes.saturating_sub(prev.tx_bytes),
                    rx_packets: entry.rx_packets.saturating_sub(prev.rx_packets),
                    tx_packets: entry.tx_packets.saturating_sub(prev.tx_packets),
                };
                state.cumulative += delta;
                state.rate = TrafficCounters {
                    rx_bytes: ((delta.rx_bytes as f64) / dt_secs).round() as u64,
                    tx_bytes: ((delta.tx_bytes as f64) / dt_secs).round() as u64,
                    rx_packets: ((delta.rx_packets as f64) / dt_secs).round() as u64,
                    tx_packets: ((delta.tx_packets as f64) / dt_secs).round() as u64,
                };
            }
            state.prev_raw = Some(entry);
        }
    }

    /// Ingests a batch of parsed packets at `Instant::now()`.
    pub fn ingest_packets(&mut self, packets: &[ParsedPacket]) {
        self.ingest_packets_at(packets, Instant::now());
    }

    /// Ingests a batch of parsed packets at an explicit timestamp `now`.
    pub fn ingest_packets_at(&mut self, packets: &[ParsedPacket], now: Instant) {
        let sec = now.saturating_duration_since(self.start_time).as_secs();
        self.prune_expired_buckets(sec);

        if packets.is_empty() {
            return;
        }

        let need_new_bucket = self.buckets.back().is_none_or(|b| b.sec != sec);
        if need_new_bucket {
            self.buckets.push_back(TrafficBucket {
                sec,
                ..TrafficBucket::default()
            });
            while self.buckets.len() > WINDOW_SECS {
                self.buckets.pop_front();
            }
        }

        for pkt in packets {
            let (direction, key) = classify_packet(&self.socket_table, pkt);
            let delta = match direction {
                Direction::Download => TrafficCounters {
                    rx_bytes: pkt.len,
                    tx_bytes: 0,
                    rx_packets: 1,
                    tx_packets: 0,
                },
                Direction::Upload => TrafficCounters {
                    rx_bytes: 0,
                    tx_bytes: pkt.len,
                    rx_packets: 0,
                    tx_packets: 1,
                },
            };

            if let Some(bucket) = self.buckets.back_mut() {
                bucket.total += delta;
                *bucket.connections.entry(key).or_default() += delta;
            }

            self.cumulative_total += delta;
            *self.cumulative_connections.entry(key).or_default() += delta;

            self.connection_processes.entry(key).or_insert_with(|| {
                self.socket_table
                    .lookup_connection(
                        key.protocol,
                        key.local.ip,
                        key.local.port,
                        key.remote.ip,
                        key.remote.port,
                    )
                    .cloned()
            });
        }
    }

    /// Returns the rolling 5-second average `(rx_bps, tx_bps)` at `now`.
    pub fn rate_bps(&self, now: Instant) -> (f64, f64) {
        let (sum, window_secs) = self.window_sum(now);
        (
            sum.rx_bytes as f64 / window_secs,
            sum.tx_bytes as f64 / window_secs,
        )
    }

    /// Returns cumulative byte and packet totals since engine creation.
    pub fn cumulative_totals(&self) -> TrafficCounters {
        self.cumulative_total
    }

    /// Produces a sorted and filtered `NetSnapshot` at `Instant::now()`.
    pub fn snapshot(
        &self,
        sort: SortColumn,
        filter: Option<&str>,
        show_total: bool,
    ) -> NetSnapshot {
        self.snapshot_at(sort, filter, show_total, Instant::now())
    }

    /// Produces a sorted and filtered `NetSnapshot` at an explicit timestamp `now`.
    pub fn snapshot_at(
        &self,
        sort: SortColumn,
        filter: Option<&str>,
        show_total: bool,
        now: Instant,
    ) -> NetSnapshot {
        let filter_lower = filter
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase());

        let interfaces = self.build_interfaces(show_total);

        if matches!(self.status, CaptureStatus::ProcfsFallback(_)) {
            return self.build_procfs_fallback_snapshot(interfaces, sort, filter_lower.as_deref());
        }

        let (total, conn_counters) = if show_total {
            (self.cumulative_total, self.cumulative_connections.clone())
        } else {
            let (sum, window_secs) = self.window_sum(now);
            let total_rate = scale_counters(sum, window_secs);
            let mut per_conn: HashMap<ConnectionKey, TrafficCounters> = HashMap::new();
            let current_sec = now.saturating_duration_since(self.start_time).as_secs();
            for bucket in &self.buckets {
                if bucket.sec + (WINDOW_SECS as u64) > current_sec {
                    for (key, counters) in &bucket.connections {
                        *per_conn.entry(*key).or_default() += *counters;
                    }
                }
            }
            for counters in per_conn.values_mut() {
                *counters = scale_counters(*counters, window_secs);
            }
            (total_rate, per_conn)
        };

        let mut connections = Vec::with_capacity(conn_counters.len());
        let mut proc_map: HashMap<(u32, String), (ProcessInfo, TrafficCounters, usize)> =
            HashMap::new();
        let mut remote_map: HashMap<IpAddr, (TrafficCounters, usize)> = HashMap::new();

        for (key, counters) in conn_counters {
            let proc_info = self.connection_processes.get(&key).cloned().flatten();
            let hostname = self.dns.resolve(key.remote.ip);

            let proc_key_info = proc_info.clone().unwrap_or_else(unknown_process);
            let proc_entry = proc_map
                .entry((proc_key_info.pid, proc_key_info.name.clone()))
                .or_insert_with(|| (proc_key_info, TrafficCounters::default(), 0));
            proc_entry.1 += counters;
            proc_entry.2 += 1;

            let remote_entry = remote_map
                .entry(key.remote.ip)
                .or_insert_with(|| (TrafficCounters::default(), 0));
            remote_entry.0 += counters;
            remote_entry.1 += 1;

            connections.push(ConnectionTraffic {
                key,
                process: proc_info,
                hostname,
                counters,
            });
        }

        let mut processes: Vec<ProcessTraffic> = proc_map
            .into_values()
            .map(|(process, counters, connection_count)| ProcessTraffic {
                process,
                counters,
                connection_count,
            })
            .collect();

        let mut remotes: Vec<RemoteTraffic> = remote_map
            .into_iter()
            .map(|(ip, (counters, connection_count))| RemoteTraffic {
                ip,
                hostname: self.dns.resolve(ip),
                counters,
                connection_count,
            })
            .collect();

        apply_filter_and_sort(
            &mut processes,
            &mut connections,
            &mut remotes,
            sort,
            filter_lower.as_deref(),
        );

        NetSnapshot {
            interval_secs: 1.0,
            status: self.status.clone(),
            total,
            interfaces,
            processes,
            connections,
            remotes,
        }
    }

    fn build_procfs_fallback_snapshot(
        &self,
        interfaces: Vec<InterfaceTraffic>,
        sort: SortColumn,
        filter_lower: Option<&str>,
    ) -> NetSnapshot {
        let mut total = TrafficCounters::default();
        for iface in &interfaces {
            if iface.name == "lo" && self.interface_filter.as_deref() != Some("lo") {
                continue;
            }
            total += iface.counters;
        }

        let active = self.socket_table.active_connections();
        let mut connections = Vec::with_capacity(active.len());
        let mut proc_map: HashMap<(u32, String), (ProcessInfo, usize)> = HashMap::new();
        let mut remote_map: HashMap<IpAddr, usize> = HashMap::new();

        for (entry, proc_opt) in active {
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
            let proc_info = proc_opt.cloned();
            let proc_key_info = proc_info.clone().unwrap_or_else(unknown_process);
            let proc_entry = proc_map
                .entry((proc_key_info.pid, proc_key_info.name.clone()))
                .or_insert_with(|| (proc_key_info, 0));
            proc_entry.1 += 1;

            if !entry.remote_ip.is_unspecified() {
                *remote_map.entry(entry.remote_ip).or_default() += 1;
            }

            connections.push(ConnectionTraffic {
                key,
                process: proc_info,
                hostname: self.dns.resolve(entry.remote_ip),
                counters: TrafficCounters::default(),
            });
        }

        let mut processes: Vec<ProcessTraffic> = proc_map
            .into_values()
            .map(|(process, connection_count)| ProcessTraffic {
                process,
                counters: TrafficCounters::default(),
                connection_count,
            })
            .collect();

        let mut remotes: Vec<RemoteTraffic> = remote_map
            .into_iter()
            .map(|(ip, connection_count)| RemoteTraffic {
                ip,
                hostname: self.dns.resolve(ip),
                counters: TrafficCounters::default(),
                connection_count,
            })
            .collect();

        apply_filter_and_sort(
            &mut processes,
            &mut connections,
            &mut remotes,
            sort,
            filter_lower,
        );

        NetSnapshot {
            interval_secs: 1.0,
            status: self.status.clone(),
            total,
            interfaces,
            processes,
            connections,
            remotes,
        }
    }

    fn build_interfaces(&self, show_total: bool) -> Vec<InterfaceTraffic> {
        let mut out: Vec<InterfaceTraffic> = self
            .interfaces
            .iter()
            .map(|(name, state)| InterfaceTraffic {
                name: name.clone(),
                counters: if show_total {
                    state.cumulative
                } else {
                    state.rate
                },
                is_up: state.is_up,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn prune_expired_buckets(&mut self, current_sec: u64) {
        while let Some(front) = self.buckets.front() {
            if front.sec + (WINDOW_SECS as u64) <= current_sec {
                self.buckets.pop_front();
            } else {
                break;
            }
        }
    }

    fn window_sum(&self, now: Instant) -> (TrafficCounters, f64) {
        let current_sec = now.saturating_duration_since(self.start_time).as_secs();
        let mut sum = TrafficCounters::default();
        for bucket in &self.buckets {
            if bucket.sec + (WINDOW_SECS as u64) > current_sec {
                sum += bucket.total;
            }
        }
        let elapsed_secs = ((current_sec + 1) as f64).clamp(1.0, WINDOW_SECS as f64);
        (sum, elapsed_secs)
    }
}

fn classify_packet(socket_table: &SocketTable, pkt: &ParsedPacket) -> (Direction, ConnectionKey) {
    let direction = match pkt.direction {
        Some(dir) => dir,
        None => {
            if socket_table
                .lookup_connection(
                    pkt.protocol,
                    pkt.src_ip,
                    pkt.src_port,
                    pkt.dst_ip,
                    pkt.dst_port,
                )
                .is_some()
            {
                Direction::Upload
            } else {
                Direction::Download
            }
        }
    };

    let key = match direction {
        Direction::Upload => ConnectionKey {
            protocol: pkt.protocol,
            local: LocalSocket {
                ip: pkt.src_ip,
                port: pkt.src_port,
            },
            remote: RemoteSocket {
                ip: pkt.dst_ip,
                port: pkt.dst_port,
            },
        },
        Direction::Download => ConnectionKey {
            protocol: pkt.protocol,
            local: LocalSocket {
                ip: pkt.dst_ip,
                port: pkt.dst_port,
            },
            remote: RemoteSocket {
                ip: pkt.src_ip,
                port: pkt.src_port,
            },
        },
    };

    (direction, key)
}

fn scale_counters(sum: TrafficCounters, window_secs: f64) -> TrafficCounters {
    TrafficCounters {
        rx_bytes: (sum.rx_bytes as f64 / window_secs).round() as u64,
        tx_bytes: (sum.tx_bytes as f64 / window_secs).round() as u64,
        rx_packets: (sum.rx_packets as f64 / window_secs).round() as u64,
        tx_packets: (sum.tx_packets as f64 / window_secs).round() as u64,
    }
}

fn apply_filter_and_sort(
    processes: &mut Vec<ProcessTraffic>,
    connections: &mut Vec<ConnectionTraffic>,
    remotes: &mut Vec<RemoteTraffic>,
    sort: SortColumn,
    filter_lower: Option<&str>,
) {
    if let Some(q) = filter_lower {
        processes.retain(|p| {
            p.process.name.to_ascii_lowercase().contains(q)
                || p.process.cmdline.to_ascii_lowercase().contains(q)
                || p.process.pid.to_string().contains(q)
        });
        connections.retain(|c| {
            c.key.local.ip.to_string().contains(q)
                || c.key.remote.ip.to_string().contains(q)
                || c.key.local.port.to_string().contains(q)
                || c.key.remote.port.to_string().contains(q)
                || c.key.protocol.to_string().to_ascii_lowercase().contains(q)
                || c.hostname
                    .as_deref()
                    .is_some_and(|h| h.to_ascii_lowercase().contains(q))
                || c.process.as_ref().is_some_and(|p| {
                    p.name.to_ascii_lowercase().contains(q) || p.pid.to_string().contains(q)
                })
        });
        remotes.retain(|r| {
            r.ip.to_string().contains(q)
                || r.hostname
                    .as_deref()
                    .is_some_and(|h| h.to_ascii_lowercase().contains(q))
        });
    }

    processes.sort_by(|a, b| match sort {
        SortColumn::Total => b
            .counters
            .total_bytes()
            .cmp(&a.counters.total_bytes())
            .then_with(|| a.process.name.cmp(&b.process.name))
            .then_with(|| a.process.pid.cmp(&b.process.pid)),
        SortColumn::Rx => b
            .counters
            .rx_bytes
            .cmp(&a.counters.rx_bytes)
            .then_with(|| a.process.name.cmp(&b.process.name)),
        SortColumn::Tx => b
            .counters
            .tx_bytes
            .cmp(&a.counters.tx_bytes)
            .then_with(|| a.process.name.cmp(&b.process.name)),
        SortColumn::Pid => a
            .process
            .pid
            .cmp(&b.process.pid)
            .then_with(|| a.process.name.cmp(&b.process.name)),
        SortColumn::Name => a
            .process
            .name
            .to_ascii_lowercase()
            .cmp(&b.process.name.to_ascii_lowercase())
            .then_with(|| a.process.pid.cmp(&b.process.pid)),
    });

    connections.sort_by(|a, b| match sort {
        SortColumn::Total => b
            .counters
            .total_bytes()
            .cmp(&a.counters.total_bytes())
            .then_with(|| a.key.cmp(&b.key)),
        SortColumn::Rx => b
            .counters
            .rx_bytes
            .cmp(&a.counters.rx_bytes)
            .then_with(|| a.key.cmp(&b.key)),
        SortColumn::Tx => b
            .counters
            .tx_bytes
            .cmp(&a.counters.tx_bytes)
            .then_with(|| a.key.cmp(&b.key)),
        SortColumn::Pid => a
            .process
            .as_ref()
            .map_or(u32::MAX, |p| p.pid)
            .cmp(&b.process.as_ref().map_or(u32::MAX, |p| p.pid))
            .then_with(|| a.key.cmp(&b.key)),
        SortColumn::Name => conn_sort_name(a)
            .cmp(&conn_sort_name(b))
            .then_with(|| a.key.cmp(&b.key)),
    });

    remotes.sort_by(|a, b| match sort {
        SortColumn::Total => b
            .counters
            .total_bytes()
            .cmp(&a.counters.total_bytes())
            .then_with(|| a.ip.cmp(&b.ip)),
        SortColumn::Rx => b
            .counters
            .rx_bytes
            .cmp(&a.counters.rx_bytes)
            .then_with(|| a.ip.cmp(&b.ip)),
        SortColumn::Tx => b
            .counters
            .tx_bytes
            .cmp(&a.counters.tx_bytes)
            .then_with(|| a.ip.cmp(&b.ip)),
        SortColumn::Pid => a.ip.cmp(&b.ip),
        SortColumn::Name => remote_sort_name(a)
            .cmp(&remote_sort_name(b))
            .then_with(|| a.ip.cmp(&b.ip)),
    });
}

fn conn_sort_name(conn: &ConnectionTraffic) -> String {
    if let Some(proc_info) = &conn.process {
        proc_info.name.to_ascii_lowercase()
    } else if let Some(host) = &conn.hostname {
        host.to_ascii_lowercase()
    } else {
        conn.key.remote.ip.to_string()
    }
}

fn remote_sort_name(remote: &RemoteTraffic) -> String {
    remote
        .hostname
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| remote.ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Protocol;
    use std::net::Ipv4Addr;
    use std::os::unix::fs::symlink;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
                "rsnet_engine_{prefix}_{}_{}",
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

    fn make_packet(
        protocol: Protocol,
        src_ip: IpAddr,
        src_port: u16,
        dst_ip: IpAddr,
        dst_port: u16,
        len: u64,
        direction: Option<Direction>,
    ) -> ParsedPacket {
        ParsedPacket {
            protocol,
            src_ip,
            src_port,
            dst_ip,
            dst_port,
            len,
            direction,
            ifindex: 2,
        }
    }

    #[test]
    fn rolling_window_averaging_and_cumulative_totals() {
        let t0 = Instant::now();
        let mut engine = BandwidthEngine::with_start_time(None, true, CaptureStatus::Live, t0);

        let local = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let remote = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));

        // Feed 1000 bytes/sec download and 500 bytes/sec upload for seconds 0..5
        for s in 0..5 {
            let now = t0 + Duration::from_secs(s);
            let pkts = vec![
                make_packet(
                    Protocol::Tcp,
                    remote,
                    443,
                    local,
                    50000,
                    1000,
                    Some(Direction::Download),
                ),
                make_packet(
                    Protocol::Tcp,
                    local,
                    50000,
                    remote,
                    443,
                    500,
                    Some(Direction::Upload),
                ),
            ];
            engine.ingest_packets_at(&pkts, now);
        }

        let (rx_bps, tx_bps) = engine.rate_bps(t0 + Duration::from_secs(4));
        assert!((rx_bps - 1000.0).abs() < 0.01);
        assert!((tx_bps - 500.0).abs() < 0.01);

        let totals = engine.cumulative_totals();
        assert_eq!(totals.rx_bytes, 5000);
        assert_eq!(totals.tx_bytes, 2500);
        assert_eq!(totals.rx_packets, 5);
        assert_eq!(totals.tx_packets, 5);

        // At t0 + 7s with no new traffic, only buckets 3 and 4 remain in [3..=7] -> 2000 / 5 = 400 B/s
        let (rx_decay, tx_decay) = engine.rate_bps(t0 + Duration::from_secs(7));
        assert!((rx_decay - 400.0).abs() < 0.01);
        assert!((tx_decay - 200.0).abs() < 0.01);

        // Cumulative totals remain unchanged
        assert_eq!(engine.cumulative_totals().rx_bytes, 5000);
    }

    #[test]
    fn process_attribution_and_unknown_traffic() {
        let tmp = TempDir::new("proc_attr");
        let net_dir = tmp.path.join("net");
        fs::create_dir_all(&net_dir).unwrap();
        // 192.168.1.10:50000 -> 93.184.216.34:443, inode 9999
        let tcp_content = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
            0: 0A01A8C0:C350 22D8B85D:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 9999 1 00000000 0\n";
        fs::write(net_dir.join("tcp"), tcp_content).unwrap();

        let fd_dir = tmp.path.join("4242/fd");
        fs::create_dir_all(&fd_dir).unwrap();
        fs::write(tmp.path.join("4242/comm"), "curl\n").unwrap();
        fs::write(tmp.path.join("4242/cmdline"), "curl\0https://example.com\0").unwrap();
        fs::write(
            tmp.path.join("4242/status"),
            "Uid:\t1000\t1000\t1000\t1000\n",
        )
        .unwrap();
        symlink("socket:[9999]", fd_dir.join("3")).unwrap();

        let t0 = Instant::now();
        let mut engine = BandwidthEngine::with_start_time(None, true, CaptureStatus::Live, t0);
        engine.refresh_sockets(&tmp.path);

        let local = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let known_remote = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        let unknown_remote = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));

        let pkts = vec![
            make_packet(
                Protocol::Tcp,
                known_remote,
                443,
                local,
                50000,
                2048,
                Some(Direction::Download),
            ),
            make_packet(
                Protocol::Udp,
                unknown_remote,
                53,
                local,
                55555,
                512,
                Some(Direction::Download),
            ),
        ];
        engine.ingest_packets_at(&pkts, t0);

        let snap = engine.snapshot_at(SortColumn::Total, None, true, t0);
        assert_eq!(snap.processes.len(), 2);
        assert_eq!(snap.processes[0].process.name, "curl");
        assert_eq!(snap.processes[0].process.pid, 4242);
        assert_eq!(snap.processes[0].counters.rx_bytes, 2048);

        assert_eq!(snap.processes[1].process.name, "<unknown>");
        assert_eq!(snap.processes[1].process.pid, 0);
        assert_eq!(snap.processes[1].counters.rx_bytes, 512);
    }

    #[test]
    fn interface_filtering_and_procfs_fallback_snapshot() {
        let t0 = Instant::now();
        let mut engine = BandwidthEngine::with_start_time(
            Some("eth0".to_string()),
            true,
            CaptureStatus::ProcfsFallback("EPERM".to_string()),
            t0,
        );

        engine.refresh_interfaces(
            vec![
                InterfaceCounters {
                    name: "lo".to_string(),
                    rx_bytes: 100,
                    rx_packets: 1,
                    tx_bytes: 100,
                    tx_packets: 1,
                },
                InterfaceCounters {
                    name: "eth0".to_string(),
                    rx_bytes: 1000,
                    rx_packets: 10,
                    tx_bytes: 500,
                    tx_packets: 5,
                },
            ],
            t0,
        );

        engine.refresh_interfaces(
            vec![
                InterfaceCounters {
                    name: "lo".to_string(),
                    rx_bytes: 200,
                    rx_packets: 2,
                    tx_bytes: 200,
                    tx_packets: 2,
                },
                InterfaceCounters {
                    name: "eth0".to_string(),
                    rx_bytes: 3000,
                    rx_packets: 30,
                    tx_bytes: 1500,
                    tx_packets: 15,
                },
            ],
            t0 + Duration::from_secs(1),
        );

        let snap = engine.snapshot_at(SortColumn::Total, None, false, t0 + Duration::from_secs(1));
        assert_eq!(snap.interfaces.len(), 1);
        assert_eq!(snap.interfaces[0].name, "eth0");
        assert_eq!(snap.interfaces[0].counters.rx_bytes, 2000);
        assert_eq!(snap.interfaces[0].counters.tx_bytes, 1000);
        assert_eq!(snap.total.rx_bytes, 2000);
        assert_eq!(snap.total.tx_bytes, 1000);
    }

    #[test]
    fn sorting_and_filtering_work_across_tables() {
        let t0 = Instant::now();
        let mut engine = BandwidthEngine::with_start_time(None, true, CaptureStatus::Live, t0);
        let local = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let r1 = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let r2 = IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4));

        engine.ingest_packets_at(
            &[
                make_packet(
                    Protocol::Udp,
                    r1,
                    53,
                    local,
                    10001,
                    100,
                    Some(Direction::Download),
                ),
                make_packet(
                    Protocol::Tcp,
                    local,
                    10002,
                    r2,
                    443,
                    500,
                    Some(Direction::Upload),
                ),
            ],
            t0,
        );

        let by_tx = engine.snapshot_at(SortColumn::Tx, None, true, t0);
        assert_eq!(by_tx.remotes[0].ip, r2);
        assert_eq!(by_tx.remotes[1].ip, r1);

        let filtered = engine.snapshot_at(SortColumn::Total, Some("1.1.1.1"), true, t0);
        assert_eq!(filtered.remotes.len(), 1);
        assert_eq!(filtered.remotes[0].ip, r1);
        assert_eq!(filtered.connections.len(), 1);
    }

    #[test]
    fn dns_cache_respects_no_resolve_and_caches_background_results() {
        let disabled = DnsCache::new(true);
        assert_eq!(
            disabled.resolve(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            None
        );
        assert!(disabled.is_empty());

        let cache = DnsCache::with_resolver(true, |ip| Some(format!("host-{ip}")));
        let ip = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        // First call is non-blocking and returns None while queuing lookup.
        assert_eq!(cache.resolve(ip), None);
        for _ in 0..50 {
            if let Some(name) = cache.resolve(ip) {
                assert_eq!(name, "host-93.184.216.34");
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("background DNS worker did not populate cache in time");
    }
}
