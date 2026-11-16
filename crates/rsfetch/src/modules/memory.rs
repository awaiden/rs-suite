use std::fs;

use rs_common::fmt::format_kib;

use crate::system::{ModuleEntry, ModuleId};

/// Parsed memory metrics (in KiB) from `/proc/meminfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    pub total_kib: u64,
    pub available_kib: u64,
    pub used_kib: u64,
    pub swap_total_kib: u64,
    pub swap_used_kib: u64,
}

/// Parses `/proc/meminfo` content into [`MemInfo`].
pub fn parse_meminfo(content: &str) -> Option<MemInfo> {
    let mut mem_total: Option<u64> = None;
    let mut mem_available: Option<u64> = None;
    let mut mem_free: u64 = 0;
    let mut buffers: u64 = 0;
    let mut cached: u64 = 0;
    let mut swap_total: u64 = 0;
    let mut swap_free: u64 = 0;

    for line in content.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let val: u64 = rest
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);

        match key.trim() {
            "MemTotal" => mem_total = Some(val),
            "MemAvailable" => mem_available = Some(val),
            "MemFree" => mem_free = val,
            "Buffers" => buffers = val,
            "Cached" => cached = val,
            "SwapTotal" => swap_total = val,
            "SwapFree" => swap_free = val,
            _ => {}
        }
    }

    let total_kib = mem_total.filter(|&v| v > 0)?;
    let available_kib = mem_available
        .unwrap_or_else(|| mem_free.saturating_add(buffers).saturating_add(cached))
        .min(total_kib);
    let used_kib = total_kib.saturating_sub(available_kib);
    let swap_used_kib = swap_total.saturating_sub(swap_free);

    Some(MemInfo {
        total_kib,
        available_kib,
        used_kib,
        swap_total_kib: swap_total,
        swap_used_kib,
    })
}

/// Formats [`MemInfo`] using `rs_common::fmt::format_kib`.
pub fn format_meminfo(info: &MemInfo) -> (String, u64) {
    let pct = if info.total_kib > 0 {
        ((info.used_kib as f64 / info.total_kib as f64) * 100.0).round() as u64
    } else {
        0
    };
    let text = format!(
        "{} / {} ({pct}%)",
        format_kib(info.used_kib),
        format_kib(info.total_kib)
    );
    (text, pct)
}

/// Collects the Memory module entry.
pub fn collect() -> Option<ModuleEntry> {
    let raw = fs::read_to_string("/proc/meminfo").ok()?;
    let info = parse_meminfo(&raw)?;
    let (value, pct) = format_meminfo(&info);

    Some(ModuleEntry {
        id: ModuleId::Memory,
        label: ModuleId::Memory.label().to_string(),
        value,
        json_value: serde_json::json!({
            "used_kib": info.used_kib,
            "total_kib": info.total_kib,
            "available_kib": info.available_kib,
            "swap_used_kib": info.swap_used_kib,
            "swap_total_kib": info.swap_total_kib,
            "percent": pct,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo_and_formats_kib() {
        let sample = "\
MemTotal:       16384000 kB
MemFree:         2000000 kB
MemAvailable:    8192000 kB
Buffers:          500000 kB
Cached:          4000000 kB
SwapTotal:       4194304 kB
SwapFree:        3145728 kB
";
        let info = parse_meminfo(sample).expect("meminfo should parse");
        assert_eq!(info.total_kib, 16_384_000);
        assert_eq!(info.used_kib, 8_192_000);
        assert_eq!(info.swap_used_kib, 1_048_576);
        let (text, pct) = format_meminfo(&info);
        assert_eq!(pct, 50);
        assert_eq!(text, "7.8GiB / 15.6GiB (50%)");
    }
}
