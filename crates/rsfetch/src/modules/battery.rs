use std::fs;
use std::path::Path;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

/// Formats battery capacity and status strings.
pub fn format_battery(capacity_raw: &str, status_raw: Option<&str>) -> Option<(u8, String)> {
    let cap: u8 = capacity_raw.trim().parse().ok()?;
    let cap = cap.min(100);
    let status = status_raw
        .map(|s| sanitize_control_chars(s.trim()))
        .filter(|s| !s.is_empty());

    let text = match status {
        Some(ref st) => format!("{cap}% ({st})"),
        None => format!("{cap}%"),
    };
    Some((cap, text))
}

/// Collects the Battery module entry from `/sys/class/power_supply/BAT*`.
pub fn collect() -> Option<ModuleEntry> {
    let entries = fs::read_dir(Path::new("/sys/class/power_supply")).ok()?;
    let mut bats: Vec<String> = Vec::new();
    let mut json_bats: Vec<serde_json::Value> = Vec::new();

    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("BAT"))
        .map(|e| e.path())
        .collect();
    paths.sort();

    for path in paths {
        let Ok(cap_raw) = fs::read_to_string(path.join("capacity")) else {
            continue;
        };
        let status_raw = fs::read_to_string(path.join("status")).ok();
        let Some((cap, text)) = format_battery(&cap_raw, status_raw.as_deref()) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_control_chars)
            .unwrap_or_else(|| "BAT".to_string());
        bats.push(text);
        json_bats.push(serde_json::json!({
            "name": name,
            "capacity": cap,
            "status": status_raw.map(|s| sanitize_control_chars(s.trim())),
        }));
    }

    if bats.is_empty() {
        return None;
    }

    Some(ModuleEntry {
        id: ModuleId::Battery,
        label: ModuleId::Battery.label().to_string(),
        value: bats.join(", "),
        json_value: serde_json::json!(json_bats),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_battery_capacity_and_status() {
        assert_eq!(
            format_battery("85\n", Some("Discharging\n")),
            Some((85, "85% (Discharging)".to_string()))
        );
        assert_eq!(format_battery("105", None), Some((100, "100%".to_string())));
        assert_eq!(format_battery("invalid", Some("Full")), None);
    }
}
