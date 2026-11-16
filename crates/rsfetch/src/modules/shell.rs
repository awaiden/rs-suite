use std::fs;
use std::path::Path;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

/// Extracts sanitized shell name and optional version from path and version env hint.
pub fn format_shell(shell_path: &str, version_hint: Option<&str>) -> Option<(String, String)> {
    let trimmed = shell_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let name = Path::new(trimmed)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(trimmed);
    let clean_name = sanitize_control_chars(name.trim());
    if clean_name.is_empty() {
        return None;
    }

    let formatted = match version_hint.map(str::trim).filter(|v| !v.is_empty()) {
        Some(ver) => {
            // Strip trailing build metadata like "(1)-release" from BASH_VERSION.
            let short_ver = ver.split('(').next().unwrap_or(ver).trim();
            let clean_ver = sanitize_control_chars(short_ver);
            if clean_ver.is_empty() {
                clean_name.clone()
            } else {
                format!("{clean_name} {clean_ver}")
            }
        }
        None => clean_name.clone(),
    };

    Some((clean_name, formatted))
}

fn parent_comm() -> Option<String> {
    let ppid = unsafe { libc::getppid() };
    if ppid <= 0 {
        return None;
    }
    fs::read_to_string(format!("/proc/{ppid}/comm"))
        .ok()
        .map(|s| sanitize_control_chars(s.trim()))
        .filter(|s| !s.is_empty())
}

/// Collects the Shell module entry.
pub fn collect() -> Option<ModuleEntry> {
    let shell_env = std::env::var("SHELL").ok();
    let raw_shell = shell_env
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(parent_comm)?;

    let base_name = Path::new(&raw_shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&raw_shell);

    let version_hint = match base_name {
        "bash" => std::env::var("BASH_VERSION").ok(),
        "zsh" => std::env::var("ZSH_VERSION").ok(),
        "fish" => std::env::var("FISH_VERSION").ok(),
        _ => None,
    };

    let (name, value) = format_shell(&raw_shell, version_hint.as_deref())?;

    Some(ModuleEntry {
        id: ModuleId::Shell,
        label: ModuleId::Shell.label().to_string(),
        value,
        json_value: serde_json::json!({
            "name": name,
            "path": sanitize_control_chars(&raw_shell),
            "version": version_hint.map(|v| sanitize_control_chars(&v)),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_shell_with_and_without_version() {
        assert_eq!(
            format_shell("/usr/bin/bash", Some("5.2.37(1)-release")),
            Some(("bash".to_string(), "bash 5.2.37".to_string()))
        );
        assert_eq!(
            format_shell("/bin/zsh", Some("5.9")),
            Some(("zsh".to_string(), "zsh 5.9".to_string()))
        );
        assert_eq!(
            format_shell("/usr/bin/fish", None),
            Some(("fish".to_string(), "fish".to_string()))
        );
        assert_eq!(format_shell("", None), None);
    }
}
