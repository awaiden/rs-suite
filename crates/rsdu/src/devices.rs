use rs_common::proc::unescape_octal;

pub struct Device {
    pub name: String,
    pub mount: String,
    pub size: i64,
    /// Blocks in use (reserved-for-root blocks count as used, like `df`).
    pub used: i64,
    /// Blocks available to unprivileged users.
    pub free: i64,
}

/// Extracts `(device_name, mount_point)` pairs from `/proc/self/mountinfo` content,
/// filtering to block/zfs/nfs filesystems and skipping `/snap/` mounts.
fn parse_mountinfo(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let Some(sep) = p.iter().position(|x| *x == "-") else {
            continue;
        };
        if sep < 6 || p.len() < sep + 3 {
            continue;
        }
        let (fstype, name, mount) = (p[sep + 1], p[sep + 2], unescape_octal(p[4]));
        if mount.contains("/snap/")
            || !(name.starts_with("/dev") || ["zfs", "nfs", "nfs4"].contains(&fstype))
        {
            continue;
        }
        out.push((name.into(), mount));
    }
    out
}

/// Mounted block/zfs/nfs filesystems from /proc/self/mountinfo (snap mounts skipped).
pub fn list() -> std::io::Result<Vec<Device>> {
    if !cfg!(target_os = "linux") {
        return Err(std::io::Error::other(
            "device listing is only supported on Linux",
        ));
    }
    let text = std::fs::read_to_string("/proc/self/mountinfo")?;
    let mut out = Vec::new();
    for (name, mount) in parse_mountinfo(&text) {
        let Ok(st) = rustix::fs::statvfs(mount.as_str()) else {
            continue;
        };
        let bs = st.f_frsize as i64;
        out.push(Device {
            name,
            mount,
            size: bs * st.f_blocks as i64,
            used: bs * (st.f_blocks as i64 - st.f_bfree as i64),
            free: bs * st.f_bavail as i64,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::parse_mountinfo;

    #[test]
    fn parses_mount_points_from_fixture() {
        let fixture = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
23 22 0:21 / /proc rw,nosuid,nodev,noexec - proc proc rw
24 22 8:16 / /mnt/my\\040disk rw - ext4 /dev/sdb1 rw
25 22 0:42 / /snap/core/123 ro - squashfs /dev/loop0 ro
26 22 0:43 / /tank/data rw - zfs tank/data rw
27 22 0:44 / /mnt/share rw - nfs4 10.0.0.1:/export rw
malformed line without separator
";
        let got = parse_mountinfo(fixture);
        assert_eq!(
            got,
            vec![
                ("/dev/nvme0n1p2".to_string(), "/".to_string()),
                ("/dev/sdb1".to_string(), "/mnt/my disk".to_string()),
                ("tank/data".to_string(), "/tank/data".to_string()),
                ("10.0.0.1:/export".to_string(), "/mnt/share".to_string()),
            ]
        );
    }
}
