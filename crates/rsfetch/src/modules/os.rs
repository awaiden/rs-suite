use std::fs;

use rs_common::sanitize_control_chars;

use crate::modules::kernel;
use crate::system::{ModuleEntry, ModuleId};

/// Parsed fields from `/etc/os-release`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsRelease {
    pub pretty_name: Option<String>,
    pub name: Option<String>,
    pub id: Option<String>,
    pub id_like: Vec<String>,
    pub version_id: Option<String>,
}

/// Unquotes and unescapes a value from `/etc/os-release`.
fn unquote_os_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        if trimmed.starts_with('"') {
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                } else {
                    out.push(c);
                }
            }
            return sanitize_control_chars(&out);
        }
        return sanitize_control_chars(inner);
    }
    sanitize_control_chars(trimmed)
}

/// Parses `/etc/os-release` content into an [`OsRelease`] struct.
pub fn parse_os_release(content: &str) -> OsRelease {
    let mut info = OsRelease::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = unquote_os_value(val);
        if val.is_empty() {
            continue;
        }
        match key {
            "PRETTY_NAME" => info.pretty_name = Some(val),
            "NAME" => info.name = Some(val),
            "ID" => info.id = Some(val.to_lowercase()),
            "ID_LIKE" => {
                info.id_like = val
                    .split_whitespace()
                    .map(|s| s.to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "VERSION_ID" => info.version_id = Some(val),
            _ => {}
        }
    }
    info
}

/// Reads `/etc/os-release` (with `/usr/lib/os-release` fallback).
pub fn read_os_release() -> OsRelease {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(content) = fs::read_to_string(path) {
            return parse_os_release(&content);
        }
    }
    OsRelease::default()
}

/// Formats the OS display string from [`OsRelease`] and machine architecture.
pub fn format_os_string(info: &OsRelease, arch: Option<&str>) -> String {
    let base = if let Some(pretty) = &info.pretty_name {
        pretty.clone()
    } else if let Some(name) = &info.name {
        match &info.version_id {
            Some(ver) => format!("{name} {ver}"),
            None => name.clone(),
        }
    } else {
        "Linux".to_string()
    };

    let base = sanitize_control_chars(base.trim());
    match arch.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => {
            let clean_arch = sanitize_control_chars(a);
            if base.contains(&clean_arch) {
                base
            } else {
                format!("{base} {clean_arch}")
            }
        }
        None => base,
    }
}

/// Collects the OS module entry.
pub fn collect() -> Option<ModuleEntry> {
    let info = read_os_release();
    let arch = kernel::uname().map(|u| u.machine);
    let value = format_os_string(&info, arch.as_deref());

    Some(ModuleEntry {
        id: ModuleId::Os,
        label: ModuleId::Os.label().to_string(),
        value,
        json_value: serde_json::json!({
            "pretty_name": info.pretty_name,
            "name": info.name,
            "id": info.id,
            "id_like": info.id_like,
            "version_id": info.version_id,
            "arch": arch,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arch_os_release() {
        let raw = r#"
NAME="Arch Linux"
PRETTY_NAME="Arch Linux"
ID=arch
BUILD_ID=rolling
"#;
        let info = parse_os_release(raw);
        assert_eq!(info.name.as_deref(), Some("Arch Linux"));
        assert_eq!(info.pretty_name.as_deref(), Some("Arch Linux"));
        assert_eq!(info.id.as_deref(), Some("arch"));
        assert!(info.id_like.is_empty());
        assert_eq!(format_os_string(&info, Some("x86_64")), "Arch Linux x86_64");
    }

    #[test]
    fn parses_ubuntu_with_id_like_and_escapes() {
        let raw = r#"
# Comment
NAME="Ubuntu"
VERSION_ID="24.04"
ID=ubuntu
ID_LIKE="debian"
PRETTY_NAME="Ubuntu 24.04.1 LTS"
"#;
        let info = parse_os_release(raw);
        assert_eq!(info.id.as_deref(), Some("ubuntu"));
        assert_eq!(info.id_like, vec!["debian"]);
        assert_eq!(info.version_id.as_deref(), Some("24.04"));
        assert_eq!(
            format_os_string(&info, Some("aarch64")),
            "Ubuntu 24.04.1 LTS aarch64"
        );
    }

    #[test]
    fn handles_empty_os_release_gracefully() {
        let info = parse_os_release("");
        assert_eq!(format_os_string(&info, Some("x86_64")), "Linux x86_64");
        assert_eq!(format_os_string(&info, None), "Linux");
    }
}
