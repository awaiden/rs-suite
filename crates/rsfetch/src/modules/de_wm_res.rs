use std::collections::HashSet;
use std::fs;
use std::path::Path;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

const KNOWN_WMS: &[(&str, &str)] = &[
    ("hyprland", "Hyprland"),
    ("sway", "sway"),
    ("kwin_wayland", "KWin"),
    ("kwin_x11", "KWin"),
    ("gnome-shell", "GNOME Shell"),
    ("mutter", "Mutter"),
    ("i3", "i3"),
    ("openbox", "Openbox"),
    ("bspwm", "bspwm"),
    ("dwm", "dwm"),
    ("xfwm4", "xfwm4"),
    ("wayfire", "Wayfire"),
    ("river", "river"),
    ("labwc", "labwc"),
];

/// Parses the preferred resolution (first line) from a DRM `modes` file.
pub fn parse_drm_mode(content: &str) -> Option<String> {
    let first = content.lines().next()?.trim();
    if first.is_empty() || !first.contains('x') {
        return None;
    }
    Some(sanitize_control_chars(first))
}

/// Collects connected display resolutions from `/sys/class/drm/*/modes`.
pub fn collect_resolution() -> Option<ModuleEntry> {
    let mut modes: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let modes_path = path.join("modes");
            if !modes_path.exists() {
                continue;
            }
            // If a status file exists, only include connected outputs.
            let status_path = path.join("status");
            if status_path.exists() {
                let status = fs::read_to_string(&status_path).unwrap_or_default();
                if status.trim() != "connected" {
                    continue;
                }
            }
            if let Ok(raw) = fs::read_to_string(&modes_path) {
                if let Some(m) = parse_drm_mode(&raw) {
                    if seen.insert(m.clone()) {
                        modes.push(m);
                    }
                }
            }
        }
    }

    if modes.is_empty() {
        return None;
    }

    let value = modes.join(", ");
    Some(ModuleEntry {
        id: ModuleId::Resolution,
        label: ModuleId::Resolution.label().to_string(),
        value,
        json_value: serde_json::json!(modes),
    })
}

/// Normalizes a desktop environment string from `$XDG_CURRENT_DESKTOP` or `$DESKTOP_SESSION`.
pub fn format_de_name(raw: &str) -> Option<String> {
    let clean = sanitize_control_chars(raw.trim());
    if clean.is_empty() {
        return None;
    }
    // Handle values like "ubuntu:GNOME" by taking the last non-empty segment.
    let normalized = clean
        .split(':')
        .rfind(|s| !s.trim().is_empty())
        .map(str::trim)
        .unwrap_or(&clean);
    Some(normalized.to_string())
}

/// Collects the Desktop Environment module entry.
pub fn collect_de() -> Option<ModuleEntry> {
    let raw = [
        "XDG_CURRENT_DESKTOP",
        "DESKTOP_SESSION",
        "XDG_SESSION_DESKTOP",
    ]
    .into_iter()
    .find_map(|var| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))?;

    let value = format_de_name(&raw)?;
    Some(ModuleEntry {
        id: ModuleId::De,
        label: ModuleId::De.label().to_string(),
        value: value.clone(),
        json_value: serde_json::json!({ "name": value }),
    })
}

/// Detects session protocol (`Wayland` or `X11`) from environment variables.
pub fn detect_session_protocol(
    wayland_display: Option<&str>,
    display: Option<&str>,
    session_type: Option<&str>,
) -> Option<&'static str> {
    if wayland_display.is_some_and(|v| !v.trim().is_empty())
        || session_type.is_some_and(|v| v.trim().eq_ignore_ascii_case("wayland"))
    {
        return Some("Wayland");
    }
    if display.is_some_and(|v| !v.trim().is_empty())
        || session_type.is_some_and(|v| v.trim().eq_ignore_ascii_case("x11"))
    {
        return Some("X11");
    }
    None
}

/// Matches a known WM/compositor from an iterator of process `comm` strings.
pub fn detect_wm_from_comms<'a>(comms: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    for comm in comms {
        let lower = comm.trim().to_lowercase();
        if let Some((_, display)) = KNOWN_WMS.iter().find(|(pat, _)| lower == *pat) {
            return Some(*display);
        }
    }
    None
}

fn scan_proc_for_wm() -> Option<&'static str> {
    let entries = fs::read_dir(Path::new("/proc")).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let comm_path = entry.path().join("comm");
        if let Ok(comm) = fs::read_to_string(comm_path) {
            if let Some(wm) = detect_wm_from_comms([comm.as_str()]) {
                return Some(wm);
            }
        }
    }
    None
}

/// Collects the Window Manager module entry.
pub fn collect_wm() -> Option<ModuleEntry> {
    let wm = scan_proc_for_wm()?;
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let display = std::env::var("DISPLAY").ok();
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let proto = detect_session_protocol(
        wayland.as_deref(),
        display.as_deref(),
        session_type.as_deref(),
    );

    let value = match proto {
        Some(p) => format!("{wm} ({p})"),
        None => wm.to_string(),
    };

    Some(ModuleEntry {
        id: ModuleId::Wm,
        label: ModuleId::Wm.label().to_string(),
        value,
        json_value: serde_json::json!({
            "name": wm,
            "protocol": proto,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_drm_modes_first_line() {
        assert_eq!(
            parse_drm_mode("2560x1440\n1920x1080\n"),
            Some("2560x1440".to_string())
        );
        assert_eq!(parse_drm_mode(""), None);
    }

    #[test]
    fn formats_de_and_detects_wm_and_protocol() {
        assert_eq!(format_de_name("ubuntu:GNOME").as_deref(), Some("GNOME"));
        assert_eq!(
            detect_wm_from_comms(["systemd", "bash", "Hyprland"]),
            Some("Hyprland")
        );
        assert_eq!(
            detect_session_protocol(Some("wayland-1"), None, None),
            Some("Wayland")
        );
        assert_eq!(
            detect_session_protocol(None, Some(":0"), Some("x11")),
            Some("X11")
        );
    }
}
