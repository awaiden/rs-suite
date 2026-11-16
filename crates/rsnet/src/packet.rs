use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fmt;
use std::io;
use std::mem::{size_of, zeroed};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use rs_common::sanitize_control_chars;

use crate::model::{CaptureStatus, Direction, Protocol};

const ETH_HEADER_LEN: usize = 14;
const ETH_TYPE_IPV4: u16 = 0x0800;
const ETH_TYPE_VLAN: u16 = 0x8100;
const ETH_TYPE_IPV6: u16 = 0x86DD;

const IPV4_MIN_HEADER_LEN: usize = 20;
const IPV6_HEADER_LEN: usize = 40;
const TCP_MIN_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

const IP_PROTO_HOPOPT: u8 = 0;
const IP_PROTO_TCP: u8 = 6;
const IP_PROTO_UDP: u8 = 17;
const IP_PROTO_ROUTING: u8 = 43;
const IP_PROTO_FRAGMENT: u8 = 44;
const IP_PROTO_DSTOPTS: u8 = 60;

const MAX_VLAN_TAGS: usize = 4;
const MAX_IPV6_EXT_HEADERS: usize = 16;
const ARPHRD_RAWIP: u16 = 519;
const RECV_BUF_SIZE: usize = 65_536;

/// Parsed L3/L4 packet metadata extracted from a raw frame or IP packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedPacket {
    pub protocol: Protocol,
    pub src_ip: IpAddr,
    pub src_port: u16,
    pub dst_ip: IpAddr,
    pub dst_port: u16,
    pub len: u64,
    pub direction: Option<Direction>,
    pub ifindex: u32,
}

/// Parses an Ethernet II (with optional 802.1Q VLAN tags) or raw IPv4/IPv6 packet.
pub fn parse_packet(data: &[u8], has_ethernet: bool) -> Option<ParsedPacket> {
    if data.is_empty() {
        return None;
    }

    let total_len = data.len() as u64;

    if has_ethernet {
        if data.len() < ETH_HEADER_LEN {
            return None;
        }
        let mut ethertype = u16::from_be_bytes([data[12], data[13]]);
        let mut offset = ETH_HEADER_LEN;

        for _ in 0..MAX_VLAN_TAGS {
            if ethertype != ETH_TYPE_VLAN {
                break;
            }
            // 802.1Q tag: 2 bytes TCI + 2 bytes encapsulated EtherType.
            if offset + 4 > data.len() {
                return None;
            }
            ethertype = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            offset += 4;
        }

        match ethertype {
            ETH_TYPE_IPV4 => parse_ipv4(&data[offset..], total_len),
            ETH_TYPE_IPV6 => parse_ipv6(&data[offset..], total_len),
            _ => None,
        }
    } else {
        match data[0] >> 4 {
            4 => parse_ipv4(data, total_len),
            6 => parse_ipv6(data, total_len),
            _ => None,
        }
    }
}

fn parse_ipv4(ip_bytes: &[u8], total_len: u64) -> Option<ParsedPacket> {
    if ip_bytes.len() < IPV4_MIN_HEADER_LEN {
        return None;
    }
    if (ip_bytes[0] >> 4) != 4 {
        return None;
    }

    let ihl = ((ip_bytes[0] & 0x0F) as usize) * 4;
    if ihl < IPV4_MIN_HEADER_LEN || ihl > ip_bytes.len() {
        return None;
    }

    let ip_total_len = u16::from_be_bytes([ip_bytes[2], ip_bytes[3]]) as usize;
    if ip_total_len > 0 && ip_total_len < ihl {
        return None;
    }

    let src_ip = IpAddr::V4(Ipv4Addr::new(
        ip_bytes[12],
        ip_bytes[13],
        ip_bytes[14],
        ip_bytes[15],
    ));
    let dst_ip = IpAddr::V4(Ipv4Addr::new(
        ip_bytes[16],
        ip_bytes[17],
        ip_bytes[18],
        ip_bytes[19],
    ));

    let transport = &ip_bytes[ihl..];
    parse_transport(ip_bytes[9], src_ip, dst_ip, transport, total_len)
}

fn parse_ipv6(ip_bytes: &[u8], total_len: u64) -> Option<ParsedPacket> {
    if ip_bytes.len() < IPV6_HEADER_LEN {
        return None;
    }
    if (ip_bytes[0] >> 4) != 6 {
        return None;
    }

    let src_octets: [u8; 16] = ip_bytes[8..24].try_into().ok()?;
    let dst_octets: [u8; 16] = ip_bytes[24..40].try_into().ok()?;
    let src_ip = IpAddr::V6(Ipv6Addr::from(src_octets));
    let dst_ip = IpAddr::V6(Ipv6Addr::from(dst_octets));

    let mut next_header = ip_bytes[6];
    let mut offset = IPV6_HEADER_LEN;

    for _ in 0..MAX_IPV6_EXT_HEADERS {
        match next_header {
            IP_PROTO_TCP | IP_PROTO_UDP => {
                return parse_transport(
                    next_header,
                    src_ip,
                    dst_ip,
                    &ip_bytes[offset..],
                    total_len,
                );
            }
            IP_PROTO_HOPOPT | IP_PROTO_ROUTING | IP_PROTO_DSTOPTS => {
                if offset + 2 > ip_bytes.len() {
                    return None;
                }
                let next = ip_bytes[offset];
                let hdr_len = (ip_bytes[offset + 1] as usize + 1) * 8;
                if offset + hdr_len > ip_bytes.len() {
                    return None;
                }
                offset += hdr_len;
                next_header = next;
            }
            IP_PROTO_FRAGMENT => {
                if offset + 8 > ip_bytes.len() {
                    return None;
                }
                let frag_offset =
                    u16::from_be_bytes([ip_bytes[offset + 2], ip_bytes[offset + 3]]) & 0xFFF8;
                if frag_offset != 0 {
                    // Non-initial fragments do not contain L4 port headers.
                    return None;
                }
                next_header = ip_bytes[offset];
                offset += 8;
            }
            _ => return None,
        }
    }

    None
}

fn parse_transport(
    proto_num: u8,
    src_ip: IpAddr,
    dst_ip: IpAddr,
    transport: &[u8],
    total_len: u64,
) -> Option<ParsedPacket> {
    let protocol = match proto_num {
        IP_PROTO_TCP => {
            if transport.len() < TCP_MIN_HEADER_LEN {
                return None;
            }
            let data_offset = ((transport[12] >> 4) as usize) * 4;
            if data_offset < TCP_MIN_HEADER_LEN || data_offset > transport.len() {
                return None;
            }
            Protocol::Tcp
        }
        IP_PROTO_UDP => {
            if transport.len() < UDP_HEADER_LEN {
                return None;
            }
            Protocol::Udp
        }
        _ => return None,
    };

    let src_port = u16::from_be_bytes([transport[0], transport[1]]);
    let dst_port = u16::from_be_bytes([transport[2], transport[3]]);

    Some(ParsedPacket {
        protocol,
        src_ip,
        src_port,
        dst_ip,
        dst_port,
        len: total_len,
        direction: None,
        ifindex: 0,
    })
}

/// Errors returned when opening an `AF_PACKET` raw capture socket.
#[derive(Debug)]
pub enum CaptureError {
    PermissionDenied(String),
    InterfaceNotFound(String),
    Io(io::Error),
}

impl CaptureError {
    /// Converts this capture error into a `CaptureStatus::ProcfsFallback` reason.
    pub fn into_status(self) -> CaptureStatus {
        CaptureStatus::ProcfsFallback(self.to_string())
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied(msg) => f.write_str(msg),
            Self::InterfaceNotFound(msg) => f.write_str(msg),
            Self::Io(err) => write!(f, "raw capture error: {err}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Linux `AF_PACKET` raw socket packet capture with interface-index caching.
pub struct PacketCapture {
    fd: OwnedFd,
    ifname_cache: HashMap<u32, String>,
    buf: Vec<u8>,
}

impl PacketCapture {
    /// Opens an `AF_PACKET` (`SOCK_RAW`, `ETH_P_ALL`) socket, optionally bound to `interface`.
    pub fn open(interface: Option<&str>) -> Result<Self, CaptureError> {
        let protocol = (libc::ETH_P_ALL as u16).to_be() as libc::c_int;
        let raw_fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, protocol) };
        if raw_fd < 0 {
            return Err(classify_os_error(io::Error::last_os_error()));
        }

        // SAFETY: `raw_fd` was just returned by `libc::socket` and is >= 0.
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

        let tv = libc::timeval {
            tv_sec: 0,
            tv_usec: 50_000,
        };
        let rc = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&raw const tv).cast::<libc::c_void>(),
                size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(classify_os_error(io::Error::last_os_error()));
        }

        if let Some(iface) = interface {
            let c_name = CString::new(iface).map_err(|_| {
                CaptureError::InterfaceNotFound(format!("invalid interface name: {iface}"))
            })?;
            let ifindex = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
            if ifindex == 0 {
                return Err(CaptureError::InterfaceNotFound(format!(
                    "interface '{iface}' not found"
                )));
            }

            // SAFETY: `sockaddr_ll` is a POD C struct; zero-initialization is valid.
            let mut sll: libc::sockaddr_ll = unsafe { zeroed() };
            sll.sll_family = libc::AF_PACKET as u16;
            sll.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
            sll.sll_ifindex = ifindex as libc::c_int;

            let bind_rc = unsafe {
                libc::bind(
                    fd.as_raw_fd(),
                    (&raw const sll).cast::<libc::sockaddr>(),
                    size_of::<libc::sockaddr_ll>() as libc::socklen_t,
                )
            };
            if bind_rc < 0 {
                return Err(classify_os_error(io::Error::last_os_error()));
            }
        }

        Ok(Self {
            fd,
            ifname_cache: HashMap::new(),
            buf: vec![0u8; RECV_BUF_SIZE],
        })
    }

    /// Receives up to `max_packets` parsed packets, stopping early on `SO_RCVTIMEO` timeout.
    pub fn recv_batch(&mut self, max_packets: usize) -> io::Result<Vec<ParsedPacket>> {
        let mut packets = Vec::new();
        for _ in 0..max_packets {
            match self.recv_one()? {
                Some(pkt) => packets.push(pkt),
                None => break,
            }
        }
        Ok(packets)
    }

    /// Receives a single packet with a 50ms socket timeout, returning `Ok(None)` on timeout.
    pub fn recv_one(&mut self) -> io::Result<Option<ParsedPacket>> {
        loop {
            // SAFETY: `sockaddr_ll` is a POD C struct.
            let mut sll: libc::sockaddr_ll = unsafe { zeroed() };
            let mut addr_len = size_of::<libc::sockaddr_ll>() as libc::socklen_t;

            let n = unsafe {
                libc::recvfrom(
                    self.fd.as_raw_fd(),
                    self.buf.as_mut_ptr().cast::<libc::c_void>(),
                    self.buf.len(),
                    0,
                    (&raw mut sll).cast::<libc::sockaddr>(),
                    &raw mut addr_len,
                )
            };

            if n < 0 {
                let err = io::Error::last_os_error();
                match err.kind() {
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => return Ok(None),
                    io::ErrorKind::Interrupted => continue,
                    _ => return Err(err),
                }
            }

            let bytes_len = (n as usize).min(self.buf.len());
            let has_ethernet =
                sll.sll_hatype != libc::ARPHRD_NONE && sll.sll_hatype != ARPHRD_RAWIP;
            let direction = if sll.sll_pkttype == libc::PACKET_OUTGOING {
                Direction::Upload
            } else {
                Direction::Download
            };
            let ifindex = sll.sll_ifindex.max(0) as u32;

            if let Some(mut pkt) = parse_packet(&self.buf[..bytes_len], has_ethernet) {
                pkt.direction = Some(direction);
                pkt.ifindex = ifindex;
                return Ok(Some(pkt));
            }
        }
    }

    /// Resolves and caches an interface name from its kernel `ifindex`.
    pub fn interface_name(&mut self, ifindex: u32) -> Option<&str> {
        if ifindex == 0 {
            return None;
        }
        if let std::collections::hash_map::Entry::Vacant(e) = self.ifname_cache.entry(ifindex) {
            let mut name_buf = [0 as libc::c_char; libc::IF_NAMESIZE];
            let ptr = unsafe { libc::if_indextoname(ifindex, name_buf.as_mut_ptr()) };
            if ptr.is_null() {
                return None;
            }
            // SAFETY: `if_indextoname` writes a NUL-terminated string into `name_buf`.
            let raw_name = unsafe { CStr::from_ptr(name_buf.as_ptr()) }.to_string_lossy();
            let clean = sanitize_control_chars(&raw_name);
            if clean.is_empty() {
                return None;
            }
            e.insert(clean);
        }
        self.ifname_cache.get(&ifindex).map(String::as_str)
    }
}

fn classify_os_error(err: io::Error) -> CaptureError {
    match err.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => CaptureError::PermissionDenied(
            "insufficient privileges for AF_PACKET capture (requires root or CAP_NET_RAW)"
                .to_string(),
        ),
        _ => CaptureError::Io(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ethernet_header(ethertype: u16) -> Vec<u8> {
        let mut hdr = vec![
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, // dst mac
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, // src mac
        ];
        hdr.extend_from_slice(&ethertype.to_be_bytes());
        hdr
    }

    fn ipv4_header(
        ihl_bytes: usize,
        proto: u8,
        src: [u8; 4],
        dst: [u8; 4],
        payload_len: usize,
    ) -> Vec<u8> {
        let ihl_words = (ihl_bytes / 4) as u8;
        let total_len = (ihl_bytes + payload_len) as u16;
        let mut hdr = vec![0u8; ihl_bytes];
        hdr[0] = (4 << 4) | (ihl_words & 0x0F);
        hdr[2..4].copy_from_slice(&total_len.to_be_bytes());
        hdr[8] = 64; // TTL
        hdr[9] = proto;
        hdr[12..16].copy_from_slice(&src);
        hdr[16..20].copy_from_slice(&dst);
        hdr
    }

    fn ipv6_header(next_header: u8, src: [u8; 16], dst: [u8; 16], payload_len: usize) -> Vec<u8> {
        let mut hdr = vec![0u8; 40];
        hdr[0] = 6 << 4;
        hdr[4..6].copy_from_slice(&(payload_len as u16).to_be_bytes());
        hdr[6] = next_header;
        hdr[7] = 64;
        hdr[8..24].copy_from_slice(&src);
        hdr[24..40].copy_from_slice(&dst);
        hdr
    }

    fn tcp_header(src_port: u16, dst_port: u16) -> Vec<u8> {
        let mut hdr = vec![0u8; 20];
        hdr[0..2].copy_from_slice(&src_port.to_be_bytes());
        hdr[2..4].copy_from_slice(&dst_port.to_be_bytes());
        hdr[12] = 5 << 4; // 20-byte data offset
        hdr
    }

    fn udp_header(src_port: u16, dst_port: u16) -> Vec<u8> {
        let mut hdr = vec![0u8; 8];
        hdr[0..2].copy_from_slice(&src_port.to_be_bytes());
        hdr[2..4].copy_from_slice(&dst_port.to_be_bytes());
        hdr[4..6].copy_from_slice(&8u16.to_be_bytes());
        hdr
    }

    #[test]
    fn parse_ethernet_ipv4_tcp_and_options() {
        let mut pkt = ethernet_header(ETH_TYPE_IPV4);
        let tcp = tcp_header(443, 52314);
        // 24-byte IPv4 header (4 bytes of IP options).
        let ip = ipv4_header(
            24,
            IP_PROTO_TCP,
            [192, 168, 1, 10],
            [93, 184, 216, 34],
            tcp.len(),
        );
        pkt.extend_from_slice(&ip);
        pkt.extend_from_slice(&tcp);

        let parsed = parse_packet(&pkt, true).expect("valid Ethernet+IPv4+TCP");
        assert_eq!(parsed.protocol, Protocol::Tcp);
        assert_eq!(parsed.src_ip, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)));
        assert_eq!(parsed.dst_ip, IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(parsed.src_port, 443);
        assert_eq!(parsed.dst_port, 52314);
        assert_eq!(parsed.len, pkt.len() as u64);
        assert_eq!(parsed.direction, None);
        assert_eq!(parsed.ifindex, 0);
    }

    #[test]
    fn parse_vlan_tagged_ipv4_udp() {
        let mut pkt = ethernet_header(ETH_TYPE_VLAN);
        // 802.1Q TCI (VLAN 100) + inner EtherType IPv4
        pkt.extend_from_slice(&100u16.to_be_bytes());
        pkt.extend_from_slice(&ETH_TYPE_IPV4.to_be_bytes());

        let udp = udp_header(5353, 53);
        let ip = ipv4_header(20, IP_PROTO_UDP, [10, 0, 0, 5], [1, 1, 1, 1], udp.len());
        pkt.extend_from_slice(&ip);
        pkt.extend_from_slice(&udp);

        let parsed = parse_packet(&pkt, true).expect("valid VLAN+IPv4+UDP");
        assert_eq!(parsed.protocol, Protocol::Udp);
        assert_eq!(parsed.src_port, 5353);
        assert_eq!(parsed.dst_port, 53);
        assert_eq!(parsed.src_ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)));
        assert_eq!(parsed.dst_ip, IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)));
    }

    #[test]
    fn parse_raw_ipv6_with_extension_headers() {
        let src = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).octets();
        let dst = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2).octets();

        // Hop-by-Hop (8B) -> Routing (16B) -> Fragment (8B, offset=0) -> Destination (8B) -> TCP (20B)
        let mut ext = Vec::new();
        // Hop-by-Hop: next=43, len=0 (8 bytes)
        ext.extend_from_slice(&[IP_PROTO_ROUTING, 0, 0, 0, 0, 0, 0, 0]);
        // Routing: next=44, len=1 (16 bytes)
        ext.extend_from_slice(&[
            IP_PROTO_FRAGMENT,
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]);
        // Fragment: next=60, reserved=0, frag_offset=0
        ext.extend_from_slice(&[IP_PROTO_DSTOPTS, 0, 0, 0, 0, 0, 0, 1]);
        // Destination Options: next=6 (TCP), len=0 (8 bytes)
        ext.extend_from_slice(&[IP_PROTO_TCP, 0, 0, 0, 0, 0, 0, 0]);

        let tcp = tcp_header(8080, 443);
        let mut pkt = ipv6_header(IP_PROTO_HOPOPT, src, dst, ext.len() + tcp.len());
        pkt.extend_from_slice(&ext);
        pkt.extend_from_slice(&tcp);

        let parsed = parse_packet(&pkt, false).expect("valid raw IPv6 with extension headers");
        assert_eq!(parsed.protocol, Protocol::Tcp);
        assert_eq!(parsed.src_ip, IpAddr::V6(Ipv6Addr::from(src)));
        assert_eq!(parsed.dst_ip, IpAddr::V6(Ipv6Addr::from(dst)));
        assert_eq!(parsed.src_port, 8080);
        assert_eq!(parsed.dst_port, 443);
        assert_eq!(parsed.len, pkt.len() as u64);
    }

    #[test]
    fn parse_packet_rejects_truncated_and_malformed_inputs() {
        // Empty
        assert_eq!(parse_packet(&[], true), None);
        assert_eq!(parse_packet(&[], false), None);

        // Truncated Ethernet
        assert_eq!(parse_packet(&[0u8; 13], true), None);

        // Unknown EtherType (ARP 0x0806)
        let mut arp = ethernet_header(0x0806);
        arp.extend_from_slice(&[0u8; 28]);
        assert_eq!(parse_packet(&arp, true), None);

        // Truncated VLAN tag
        let mut bad_vlan = ethernet_header(ETH_TYPE_VLAN);
        bad_vlan.extend_from_slice(&[0x00, 0x01]);
        assert_eq!(parse_packet(&bad_vlan, true), None);

        // IPv4 with invalid IHL < 20
        let mut bad_ihl = vec![0u8; 28];
        bad_ihl[0] = (4 << 4) | 3; // IHL=12B
        bad_ihl[9] = IP_PROTO_UDP;
        assert_eq!(parse_packet(&bad_ihl, false), None);

        // Truncated TCP (< 20B)
        let mut trunc_tcp = ipv4_header(20, IP_PROTO_TCP, [1, 2, 3, 4], [5, 6, 7, 8], 10);
        trunc_tcp.extend_from_slice(&[0u8; 10]);
        assert_eq!(parse_packet(&trunc_tcp, false), None);

        // Truncated UDP (< 8B)
        let mut trunc_udp = ipv4_header(20, IP_PROTO_UDP, [1, 2, 3, 4], [5, 6, 7, 8], 4);
        trunc_udp.extend_from_slice(&[0u8; 4]);
        assert_eq!(parse_packet(&trunc_udp, false), None);

        // Non-initial IPv6 fragment (frag_offset != 0)
        let src = [0u8; 16];
        let dst = [0u8; 16];
        let mut non_init_frag = ipv6_header(IP_PROTO_FRAGMENT, src, dst, 28);
        non_init_frag.extend_from_slice(&[IP_PROTO_TCP, 0, 0x00, 0x08, 0, 0, 0, 1]);
        non_init_frag.extend_from_slice(&tcp_header(80, 80));
        assert_eq!(parse_packet(&non_init_frag, false), None);
    }

    #[test]
    fn capture_open_nonexistent_interface_or_eperm_does_not_panic() {
        // Without CAP_NET_RAW this returns PermissionDenied -> ProcfsFallback;
        // with CAP_NET_RAW a nonexistent interface returns InterfaceNotFound -> ProcfsFallback.
        match PacketCapture::open(Some("rsnet_nonexistent_iface0")) {
            Ok(_) => panic!("nonexistent interface must not open"),
            Err(err) => {
                let status = err.into_status();
                assert!(matches!(status, CaptureStatus::ProcfsFallback(_)));
            }
        }
    }
}
