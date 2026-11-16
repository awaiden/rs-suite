use std::ffi::CStr;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

/// Parsed fields from `uname(2)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UtsName {
    pub sysname: String,
    pub nodename: String,
    pub release: String,
    pub machine: String,
}

/// Queries the kernel via `libc::uname` and sanitizes all fields.
pub(crate) fn uname() -> Option<UtsName> {
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::uname(&mut uts) };
    if rc != 0 {
        return None;
    }

    let to_clean = |buf: &[libc::c_char]| -> String {
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
        sanitize_control_chars(s.trim())
    };

    Some(UtsName {
        sysname: to_clean(&uts.sysname),
        nodename: to_clean(&uts.nodename),
        release: to_clean(&uts.release),
        machine: to_clean(&uts.machine),
    })
}

/// Formats kernel display string from `sysname` and `release`.
pub fn format_kernel(sysname: &str, release: &str) -> String {
    let sys = sanitize_control_chars(sysname.trim());
    let rel = sanitize_control_chars(release.trim());
    if sys.is_empty() {
        rel
    } else if rel.is_empty() {
        sys
    } else {
        format!("{sys} {rel}")
    }
}

/// Collects the Kernel module entry.
pub fn collect() -> Option<ModuleEntry> {
    let uts = uname()?;
    let value = format_kernel(&uts.sysname, &uts.release);
    if value.is_empty() {
        return None;
    }

    Some(ModuleEntry {
        id: ModuleId::Kernel,
        label: ModuleId::Kernel.label().to_string(),
        value,
        json_value: serde_json::json!({
            "sysname": uts.sysname,
            "release": uts.release,
            "arch": uts.machine,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_kernel_and_sanitizes_control_chars() {
        assert_eq!(
            format_kernel("Linux", "6.11.5-arch1-1"),
            "Linux 6.11.5-arch1-1"
        );
        assert_eq!(format_kernel("Linux\x1b[31m", "6.1\n"), "Linux [31m 6.1");
        assert_eq!(format_kernel("", "6.1.0"), "6.1.0");
    }

    #[test]
    fn uname_returns_non_empty_sysname() {
        let uts = uname().expect("uname should succeed on Linux");
        assert!(!uts.sysname.is_empty());
        assert!(!uts.release.is_empty());
    }
}
