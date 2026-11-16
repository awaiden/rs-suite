use std::fmt;
use std::net::IpAddr;
use std::ops::{Add, AddAssign};

use crate::cli::UnitFamily;

/// Transport protocol for a tracked socket or packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp => f.write_str("TCP"),
            Self::Udp => f.write_str("UDP"),
        }
    }
}

/// Packet or byte flow direction relative to the local host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Download,
    Upload,
}

/// Local socket endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalSocket {
    pub ip: IpAddr,
    pub port: u16,
}

/// Remote socket endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteSocket {
    pub ip: IpAddr,
    pub port: u16,
}

/// Canonical 5-tuple key identifying a network connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionKey {
    pub protocol: Protocol,
    pub local: LocalSocket,
    pub remote: RemoteSocket,
}

/// Metadata for a local OS process owning one or more sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    pub uid: u32,
}

/// Byte and packet counters for ingress (`rx`) and egress (`tx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrafficCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

impl TrafficCounters {
    /// Returns combined ingress and egress bytes using saturating addition.
    pub fn total_bytes(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }

    /// Returns combined ingress and egress packets using saturating addition.
    pub fn total_packets(&self) -> u64 {
        self.rx_packets.saturating_add(self.tx_packets)
    }

    /// Returns true if all byte and packet counters are zero.
    pub fn is_zero(&self) -> bool {
        self.rx_bytes == 0 && self.tx_bytes == 0 && self.rx_packets == 0 && self.tx_packets == 0
    }
}

impl Add for TrafficCounters {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            rx_bytes: self.rx_bytes.saturating_add(rhs.rx_bytes),
            tx_bytes: self.tx_bytes.saturating_add(rhs.tx_bytes),
            rx_packets: self.rx_packets.saturating_add(rhs.rx_packets),
            tx_packets: self.tx_packets.saturating_add(rhs.tx_packets),
        }
    }
}

impl AddAssign for TrafficCounters {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

/// Aggregated traffic attributed to a single process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTraffic {
    pub process: ProcessInfo,
    pub counters: TrafficCounters,
    pub connection_count: usize,
}

/// Traffic attributed to a single 5-tuple connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionTraffic {
    pub key: ConnectionKey,
    pub process: Option<ProcessInfo>,
    pub hostname: Option<String>,
    pub counters: TrafficCounters,
}

/// Aggregated traffic attributed to a remote IP address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTraffic {
    pub ip: IpAddr,
    pub hostname: Option<String>,
    pub counters: TrafficCounters,
    pub connection_count: usize,
}

/// Traffic counters for a single network interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceTraffic {
    pub name: String,
    pub counters: TrafficCounters,
    pub is_up: bool,
}

/// Capture backend status reported in each snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    Live,
    ProcfsFallback(String),
}

/// Complete point-in-time network traffic snapshot for one refresh interval.
#[derive(Debug, Clone, PartialEq)]
pub struct NetSnapshot {
    pub interval_secs: f64,
    pub status: CaptureStatus,
    pub total: TrafficCounters,
    pub interfaces: Vec<InterfaceTraffic>,
    pub processes: Vec<ProcessTraffic>,
    pub connections: Vec<ConnectionTraffic>,
    pub remotes: Vec<RemoteTraffic>,
}

const BIN_BYTE_UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
const BIN_BIT_UNITS: [&str; 5] = ["b", "Kib", "Mib", "Gib", "Tib"];
const SI_BYTE_UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
const SI_BIT_UNITS: [&str; 5] = ["b", "kb", "Mb", "Gb", "Tb"];

const BIN_RATE_UNITS: [&str; 5] = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"];
const BIN_BIT_RATE_UNITS: [&str; 5] = ["b/s", "Kib/s", "Mib/s", "Gib/s", "Tib/s"];
const SI_RATE_UNITS: [&str; 5] = ["B/s", "kB/s", "MB/s", "GB/s", "TB/s"];
const SI_BIT_RATE_UNITS: [&str; 5] = ["b/s", "kb/s", "Mb/s", "Gb/s", "Tb/s"];

fn unit_params(unit: UnitFamily, is_rate: bool) -> (f64, f64, [&'static str; 5]) {
    match (unit, is_rate) {
        (UnitFamily::BinBytes, false) => (1024.0, 1.0, BIN_BYTE_UNITS),
        (UnitFamily::BinBits, false) => (1024.0, 8.0, BIN_BIT_UNITS),
        (UnitFamily::SiBytes, false) => (1000.0, 1.0, SI_BYTE_UNITS),
        (UnitFamily::SiBits, false) => (1000.0, 8.0, SI_BIT_UNITS),
        (UnitFamily::BinBytes, true) => (1024.0, 1.0, BIN_RATE_UNITS),
        (UnitFamily::BinBits, true) => (1024.0, 8.0, BIN_BIT_RATE_UNITS),
        (UnitFamily::SiBytes, true) => (1000.0, 1.0, SI_RATE_UNITS),
        (UnitFamily::SiBits, true) => (1000.0, 8.0, SI_BIT_RATE_UNITS),
    }
}

fn format_scaled(raw_bytes: f64, unit: UnitFamily, is_rate: bool) -> String {
    let (base, multiplier, units) = unit_params(unit, is_rate);
    let sanitized = if raw_bytes.is_finite() && raw_bytes > 0.0 {
        raw_bytes * multiplier
    } else {
        0.0
    };

    let mut scaled = sanitized;
    let mut idx = 0;
    while idx + 1 < units.len() && scaled >= base {
        scaled /= base;
        idx += 1;
    }

    if idx == 0 {
        format!("{scaled:.0} {}", units[0])
    } else {
        format!("{scaled:.1} {}", units[idx])
    }
}

/// Formats a byte-per-second rate using the selected `UnitFamily`.
pub fn format_rate_with_unit(bytes_per_sec: f64, unit: UnitFamily) -> String {
    format_scaled(bytes_per_sec, unit, true)
}

/// Formats a byte count using the selected `UnitFamily`.
pub fn format_bytes_with_unit(bytes: u64, unit: UnitFamily) -> String {
    format_scaled(bytes as f64, unit, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_counters_arithmetic_and_totals() {
        let a = TrafficCounters {
            rx_bytes: 100,
            tx_bytes: 250,
            rx_packets: 2,
            tx_packets: 3,
        };
        let b = TrafficCounters {
            rx_bytes: 50,
            tx_bytes: 75,
            rx_packets: 1,
            tx_packets: 4,
        };
        let mut sum = a + b;
        assert_eq!(sum.rx_bytes, 150);
        assert_eq!(sum.tx_bytes, 325);
        assert_eq!(sum.total_bytes(), 475);
        assert_eq!(sum.total_packets(), 10);
        assert!(!sum.is_zero());

        sum += TrafficCounters::default();
        assert_eq!(sum.total_bytes(), 475);
        assert!(TrafficCounters::default().is_zero());

        let sat = TrafficCounters {
            rx_bytes: u64::MAX,
            tx_bytes: 10,
            rx_packets: u64::MAX,
            tx_packets: 1,
        };
        assert_eq!(sat.total_bytes(), u64::MAX);
        assert_eq!(sat.total_packets(), u64::MAX);
    }

    #[test]
    fn format_bytes_covers_all_unit_families_and_thresholds() {
        assert_eq!(format_bytes_with_unit(0, UnitFamily::BinBytes), "0 B");
        assert_eq!(format_bytes_with_unit(1023, UnitFamily::BinBytes), "1023 B");
        assert_eq!(
            format_bytes_with_unit(1024, UnitFamily::BinBytes),
            "1.0 KiB"
        );
        assert_eq!(
            format_bytes_with_unit(1024 * 1024, UnitFamily::BinBytes),
            "1.0 MiB"
        );

        assert_eq!(format_bytes_with_unit(0, UnitFamily::BinBits), "0 b");
        assert_eq!(format_bytes_with_unit(127, UnitFamily::BinBits), "1016 b");
        assert_eq!(format_bytes_with_unit(128, UnitFamily::BinBits), "1.0 Kib");

        assert_eq!(format_bytes_with_unit(999, UnitFamily::SiBytes), "999 B");
        assert_eq!(format_bytes_with_unit(1000, UnitFamily::SiBytes), "1.0 kB");
        assert_eq!(
            format_bytes_with_unit(1_500_000, UnitFamily::SiBytes),
            "1.5 MB"
        );

        assert_eq!(format_bytes_with_unit(124, UnitFamily::SiBits), "992 b");
        assert_eq!(format_bytes_with_unit(125, UnitFamily::SiBits), "1.0 kb");
        assert_eq!(
            format_bytes_with_unit(125_000_000, UnitFamily::SiBits),
            "1.0 Gb"
        );
    }

    #[test]
    fn format_rate_covers_all_unit_families_and_edge_cases() {
        assert_eq!(format_rate_with_unit(0.0, UnitFamily::BinBytes), "0 B/s");
        assert_eq!(format_rate_with_unit(-42.0, UnitFamily::BinBytes), "0 B/s");
        assert_eq!(format_rate_with_unit(f64::NAN, UnitFamily::SiBits), "0 b/s");
        assert_eq!(
            format_rate_with_unit(f64::INFINITY, UnitFamily::SiBytes),
            "0 B/s"
        );

        assert_eq!(
            format_rate_with_unit(1536.0, UnitFamily::BinBytes),
            "1.5 KiB/s"
        );
        assert_eq!(
            format_rate_with_unit(128.0 * 1024.0, UnitFamily::BinBits),
            "1.0 Mib/s"
        );
        assert_eq!(
            format_rate_with_unit(2_500_000_000.0, UnitFamily::SiBytes),
            "2.5 GB/s"
        );
        assert_eq!(
            format_rate_with_unit(125_000.0, UnitFamily::SiBits),
            "1.0 Mb/s"
        );
        assert_eq!(
            format_rate_with_unit(2.0 * 1024.0_f64.powi(4), UnitFamily::BinBytes),
            "2.0 TiB/s"
        );
    }
}
