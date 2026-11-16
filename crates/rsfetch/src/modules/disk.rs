use std::collections::HashSet;
use std::fs;

use rs_common::fmt::format_bytes;
use rs_common::proc::unescape_octal;
use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

const VIRTUAL_FS: &[&str] = &[
    "proc",
    "sysfs",
    "tmpfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "pstore",
    "bpf",
    "tracefs",
    "debugfs",
    "securityfs",
    "hugetlbfs",
    "mqueue",
    "fusectl",
    "configfs",
    "efivarfs",
    "ramfs",
    "autofs",
    "overlay",
    "squashfs",
    "nsfs",
    "binfmt_misc",
    "rpc_pipefs",
    "fuse.gvfsd-fuse",
    "fuse.portal",
];

/// Parsed mount entry from `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub device_id: String,
    pub mount_point: String,
    pub fs_type: String,
}

/// Parses `/proc/self/mountinfo`, filtering virtual filesystems and deduplicating by device.
pub fn parse_mountinfo(content: &str) -> Vec<MountEntry> {
    let mut mounts = Vec::new();
    let mut seen_devices = HashSet::new();

    for line in content.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let mut left_fields = left.split_whitespace();
        let _mount_id = left_fields.next();
        let _parent_id = left_fields.next();
        let Some(dev_id) = left_fields.next() else {
            continue;
        };
        let _root = left_fields.next();
        let Some(raw_mount_point) = left_fields.next() else {
            continue;
        };

        let Some(fs_type) = right.split_whitespace().next() else {
            continue;
        };

        if VIRTUAL_FS.contains(&fs_type) {
            continue;
        }
        if !seen_devices.insert(dev_id.to_string()) {
            continue;
        }

        let mount_point = sanitize_control_chars(&unescape_octal(raw_mount_point));
        let fs_type = sanitize_control_chars(fs_type);
        mounts.push(MountEntry {
            device_id: dev_id.to_string(),
            mount_point,
            fs_type,
        });
    }

    mounts
}

/// Collects the Disk module entry.
pub fn collect() -> Option<ModuleEntry> {
    let raw = fs::read_to_string("/proc/self/mountinfo").ok()?;
    let mounts = parse_mountinfo(&raw);

    let mut formatted_parts = Vec::new();
    let mut json_disks = Vec::new();

    for m in mounts {
        let Ok(stat) = rustix::fs::statvfs(&m.mount_point) else {
            continue;
        };
        let frsize = if stat.f_frsize > 0 {
            stat.f_frsize
        } else {
            stat.f_bsize
        };
        let total = stat.f_blocks.saturating_mul(frsize);
        if total == 0 {
            continue;
        }
        let free = stat.f_bfree.saturating_mul(frsize);
        let used = total.saturating_sub(free);
        let pct = ((used as f64 / total as f64) * 100.0).round() as u64;

        formatted_parts.push(format!(
            "{}: {} / {} ({pct}%) [{}]",
            m.mount_point,
            format_bytes(used),
            format_bytes(total),
            m.fs_type
        ));
        json_disks.push(serde_json::json!({
            "mount": m.mount_point,
            "fs_type": m.fs_type,
            "used_bytes": used,
            "total_bytes": total,
            "percent": pct,
        }));
    }

    if formatted_parts.is_empty() {
        return None;
    }

    Some(ModuleEntry {
        id: ModuleId::Disk,
        label: ModuleId::Disk.label().to_string(),
        value: formatted_parts.join(", "),
        json_value: serde_json::json!(json_disks),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mountinfo_unescapes_and_filters_virtual_fs() {
        let sample = "\
22 1 0:21 / /proc rw,nosuid - proc proc rw
23 1 259:2 / / rw,relatime - ext4 /dev/nvme0n1p2 rw
24 1 259:2 /home /home rw,relatime - ext4 /dev/nvme0n1p2 rw
25 1 259:3 / /mnt/my\\040disk rw,relatime - btrfs /dev/sda1 rw
26 1 0:30 / /tmp rw,nosuid - tmpfs tmpfs rw
";
        let mounts = parse_mountinfo(sample);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].mount_point, "/");
        assert_eq!(mounts[0].fs_type, "ext4");
        assert_eq!(mounts[1].mount_point, "/mnt/my disk");
        assert_eq!(mounts[1].fs_type, "btrfs");
    }
}
