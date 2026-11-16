use std::ffi::CStr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

/// Safe RAII guard for `libc::getifaddrs`.
struct IfAddrsGuard(*mut libc::ifaddrs);

impl Drop for IfAddrsGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
}

/// Returns `true` if an IPv6 address is non-loopback and non-link-local.
pub fn is_usable_ipv6(addr: Ipv6Addr) -> bool {
    !addr.is_loopback() && (addr.segments()[0] & 0xffc0) != 0xfe80
}

/// Enumerates non-loopback `AF_INET` and `AF_INET6` interface addresses.
pub fn enumerate_local_ips() -> Vec<(String, IpAddr)> {
    let mut head: *mut libc::ifaddrs = ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Vec::new();
    }
    let _guard = IfAddrsGuard(head);

    let mut results = Vec::new();
    let mut cur = head;

    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;

        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        if (ifa.ifa_flags & libc::IFF_LOOPBACK as u32) != 0 {
            continue;
        }

        let family = unsafe { (*ifa.ifa_addr).sa_family } as i32;
        let iface =
            sanitize_control_chars(&unsafe { CStr::from_ptr(ifa.ifa_name) }.to_string_lossy());

        match family {
            libc::AF_INET => {
                let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                if !ip.is_loopback() {
                    results.push((iface, IpAddr::V4(ip)));
                }
            }
            libc::AF_INET6 => {
                let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                let ip = Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                if is_usable_ipv6(ip) {
                    results.push((iface, IpAddr::V6(ip)));
                }
            }
            _ => {}
        }
    }

    results
}

/// Formats `(interface, ip)` pairs into a display string.
pub fn format_ip_entries(entries: &[(String, IpAddr)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let parts: Vec<String> = entries
        .iter()
        .map(|(iface, ip)| format!("{ip} ({iface})"))
        .collect();
    Some(parts.join(", "))
}

/// Collects the Local IP module entry.
pub fn collect() -> Option<ModuleEntry> {
    let entries = enumerate_local_ips();
    let value = format_ip_entries(&entries)?;
    let json_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|(iface, ip)| {
            serde_json::json!({
                "interface": iface,
                "ip": ip.to_string(),
            })
        })
        .collect();

    Some(ModuleEntry {
        id: ModuleId::LocalIp,
        label: ModuleId::LocalIp.label().to_string(),
        value,
        json_value: serde_json::json!(json_entries),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_link_local_and_formats_entries() {
        assert!(!is_usable_ipv6("fe80::1".parse().unwrap()));
        assert!(!is_usable_ipv6("::1".parse().unwrap()));
        assert!(is_usable_ipv6("2001:db8::1".parse().unwrap()));

        let entries = vec![
            ("wlan0".to_string(), "192.168.1.42".parse().unwrap()),
            ("eth0".to_string(), "10.0.0.5".parse().unwrap()),
        ];
        assert_eq!(
            format_ip_entries(&entries).as_deref(),
            Some("192.168.1.42 (wlan0), 10.0.0.5 (eth0)")
        );
        assert_eq!(format_ip_entries(&[]), None);
    }
}
