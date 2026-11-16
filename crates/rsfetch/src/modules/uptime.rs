use std::fs;

use rs_common::fmt::format_uptime;

use crate::system::{ModuleEntry, ModuleId};

/// Parses `/proc/uptime` and returns uptime in seconds.
pub fn parse_proc_uptime(content: &str) -> Option<f64> {
    let first = content.split_whitespace().next()?;
    let secs: f64 = first.parse().ok()?;
    if secs.is_finite() && secs >= 0.0 {
        Some(secs)
    } else {
        None
    }
}

/// Collects the Uptime module entry.
pub fn collect() -> Option<ModuleEntry> {
    let raw = fs::read_to_string("/proc/uptime").ok()?;
    let secs = parse_proc_uptime(&raw)?;
    let value = format_uptime(secs);

    Some(ModuleEntry {
        id: ModuleId::Uptime,
        label: ModuleId::Uptime.label().to_string(),
        value: value.clone(),
        json_value: serde_json::json!({
            "seconds": secs as u64,
            "formatted": value,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_uptime_and_formats() {
        let secs = parse_proc_uptime("93784.52 180000.10\n").expect("valid uptime");
        assert!((secs - 93784.52).abs() < 0.01);
        assert_eq!(format_uptime(secs), "1d 02:03:04");
    }

    #[test]
    fn rejects_invalid_uptime() {
        assert_eq!(parse_proc_uptime(""), None);
        assert_eq!(parse_proc_uptime("-10.0 0.0"), None);
        assert_eq!(parse_proc_uptime("abc 0.0"), None);
    }
}
