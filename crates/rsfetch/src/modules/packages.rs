use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::system::{ModuleEntry, ModuleId};

fn count_dirs(path: &Path) -> usize {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .count()
}

fn count_files(path: &Path) -> usize {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type()
                .map(|t| t.is_file() || t.is_symlink())
                .unwrap_or(false)
        })
        .count()
}

/// Counts installed Debian packages from `/var/lib/dpkg/status` content.
pub fn count_dpkg_status<R: BufRead>(reader: R) -> usize {
    reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.trim() == "Status: install ok installed")
        .count()
}

/// Counts installed Alpine packages from `/lib/apk/db/installed` content.
pub fn count_apk_installed<R: BufRead>(reader: R) -> usize {
    reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.starts_with("P:"))
        .count()
}

fn count_pacman() -> usize {
    count_dirs(Path::new("/var/lib/pacman/local"))
}

fn count_dpkg() -> usize {
    let Ok(file) = File::open("/var/lib/dpkg/status") else {
        return 0;
    };
    count_dpkg_status(BufReader::new(file))
}

fn count_apk() -> usize {
    let Ok(file) = File::open("/lib/apk/db/installed") else {
        return 0;
    };
    count_apk_installed(BufReader::new(file))
}

fn count_flatpak() -> usize {
    let mut total = count_dirs(Path::new("/var/lib/flatpak/app"));
    if let Ok(home) = std::env::var("HOME") {
        let user_app = PathBuf::from(home).join(".local/share/flatpak/app");
        total += count_dirs(&user_app);
    }
    total
}

fn count_snap() -> usize {
    let Ok(entries) = fs::read_dir("/snap") else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s != "bin" && s != "README" && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
        })
        .count()
}

fn count_nix() -> usize {
    let mut candidates = vec![
        PathBuf::from("/run/current-system/sw/bin"),
        PathBuf::from("/nix/var/nix/profiles/default/sw/bin"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".nix-profile/bin"));
    }
    for path in candidates {
        let n = count_files(&path);
        if n > 0 {
            return n;
        }
    }
    0
}

fn count_cargo() -> usize {
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        return count_files(&PathBuf::from(cargo_home).join("bin"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return count_files(&PathBuf::from(home).join(".cargo/bin"));
    }
    0
}

fn count_rpm() -> usize {
    if !Path::new("/var/lib/rpm").exists() {
        return 0;
    }
    // Fast format string emits 1 byte per package without sorting/formatting names.
    if let Ok(out) = Command::new("rpm").args(["-qa", "--qf", "."]).output() {
        if out.status.success() {
            return out.stdout.len();
        }
    }
    0
}

/// Formats package manager counts into a display string.
pub fn format_package_counts(counts: &BTreeMap<&'static str, usize>) -> Option<String> {
    let parts: Vec<String> = counts
        .iter()
        .filter(|&(_, &n)| n > 0)
        .map(|(name, n)| format!("{n} ({name})"))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

type PackageCheck = (&'static str, fn() -> usize);

/// Collects the Packages module entry.
pub fn collect() -> Option<ModuleEntry> {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let checks: [PackageCheck; 8] = [
        ("pacman", count_pacman),
        ("dpkg", count_dpkg),
        ("rpm", count_rpm),
        ("apk", count_apk),
        ("flatpak", count_flatpak),
        ("snap", count_snap),
        ("nix", count_nix),
        ("cargo", count_cargo),
    ];

    for (name, f) in checks {
        let n = f();
        if n > 0 {
            counts.insert(name, n);
        }
    }

    let value = format_package_counts(&counts)?;
    let total: usize = counts.values().sum();

    Some(ModuleEntry {
        id: ModuleId::Packages,
        label: ModuleId::Packages.label().to_string(),
        value,
        json_value: serde_json::json!({
            "total": total,
            "managers": counts,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn counts_dpkg_installed_packages() {
        let sample = "\
Package: bash
Status: install ok installed
Version: 5.2

Package: old-pkg
Status: deinstall ok config-files
Version: 1.0

Package: coreutils
Status: install ok installed
Version: 9.4
";
        assert_eq!(count_dpkg_status(Cursor::new(sample)), 2);
    }

    #[test]
    fn counts_apk_installed_packages() {
        let sample = "\
C:Q1abc=
P:musl
V:1.2.5-r0

C:Q1def=
P:busybox
V:1.36.1-r0
";
        assert_eq!(count_apk_installed(Cursor::new(sample)), 2);
    }

    #[test]
    fn formats_package_counts_or_returns_none_when_empty() {
        let mut counts = BTreeMap::new();
        assert_eq!(format_package_counts(&counts), None);
        counts.insert("pacman", 1042);
        counts.insert("cargo", 5);
        assert_eq!(
            format_package_counts(&counts).as_deref(),
            Some("5 (cargo), 1042 (pacman)")
        );
    }
}
