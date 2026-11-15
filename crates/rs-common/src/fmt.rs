/// Size formatting like gdu: binary (KiB) or SI (kB) prefixes, 1 decimal.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fmt {
    pub si: bool,
    pub no_prefix: bool,
    pub kib: bool,
}

impl Fmt {
    /// Formats a byte size according to the configured prefix mode.
    pub fn size(&self, n: i64) -> String {
        if self.no_prefix {
            return n.to_string();
        }
        let (base, units): (f64, [&str; 6]) = if self.si {
            (1000.0, ["kB", "MB", "GB", "TB", "PB", "EB"])
        } else {
            (1024.0, ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"])
        };
        if self.kib {
            return format!("{:.1} {}", n as f64 / base, units[0]);
        }
        let f = n as f64;
        for (i, u) in units.iter().enumerate().rev() {
            let div = base.powi(i as i32 + 1);
            if f.abs() >= div {
                return format!("{:.1} {}", f / div, u);
            }
        }
        format!("{n} B")
    }
}

/// Formats an item count using compact SI suffixes (`k`, `M`, `G`).
pub fn count(n: i64) -> String {
    let f = n as f64;
    match n {
        1_000_000_000.. => format!("{:.1}G", f / 1e9),
        1_000_000.. => format!("{:.1}M", f / 1e6),
        1_000.. => format!("{:.1}k", f / 1e3),
        _ => n.to_string(),
    }
}

/// Formats a KiB value into human-readable KiB, MiB, or GiB.
pub fn format_kib(kib: u64) -> String {
    const MIB: f64 = 1024.0;
    const GIB: f64 = 1024.0 * 1024.0;

    let val = kib as f64;
    if val >= GIB {
        format!("{:.1}GiB", val / GIB)
    } else if val >= MIB {
        format!("{:.0}MiB", val / MIB)
    } else {
        format!("{kib}KiB")
    }
}

/// Formats a byte-per-second rate into human-readable B/s, KiB/s, MiB/s, or GiB/s.
pub fn format_rate(bytes_per_sec: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let rate = bytes_per_sec.max(0.0);
    if rate >= GIB {
        format!("{:.1}GiB/s", rate / GIB)
    } else if rate >= MIB {
        format!("{:.1}MiB/s", rate / MIB)
    } else if rate >= KIB {
        format!("{:.1}KiB/s", rate / KIB)
    } else {
        format!("{rate:.0}B/s")
    }
}

/// Formats uptime seconds into `Xd HH:MM:SS` or `HH:MM:SS`.
pub fn format_uptime(uptime_secs: f64) -> String {
    let total = uptime_secs.max(0.0) as u64;
    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let mins = (total % 3_600) / 60;
    let secs = total % 60;

    if days > 0 {
        format!("{days}d {hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{hours:02}:{mins:02}:{secs:02}")
    }
}

/// Formats a byte count into compact human-readable units (B, K, M, G, T).
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const TIB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= TIB {
        format!("{:.1}T", b / TIB)
    } else if b >= GIB {
        format!("{:.1}G", b / GIB)
    } else if b >= MIB {
        format!("{:.1}M", b / MIB)
    } else if b >= KIB {
        format!("{:.0}K", b / KIB)
    } else {
        format!("{bytes}B")
    }
}

/// Formats CPU time ticks using `clock_ticks_per_sec` as `MM:SS.cc` or `H:MM:SS`.
pub fn format_cpu_time(ticks: u64, clock_ticks_per_sec: u64) -> String {
    let hz = clock_ticks_per_sec.max(1);
    let total_centis = (ticks as u128 * 100 / hz as u128) as u64;
    let total_secs = total_centis / 100;
    let centis = total_centis % 100;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}.{centis:02}")
    }
}

/// Replaces terminal control characters with spaces so untrusted names cannot inject ANSI escapes.
pub fn sanitize_control_chars(s: &str) -> String {
    s.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_size_modes_and_boundaries() {
        let f = Fmt::default();
        assert_eq!(f.size(0), "0 B");
        assert_eq!(f.size(1023), "1023 B");
        assert_eq!(f.size(1024), "1.0 KiB");
        assert_eq!(f.size(110_592), "108.0 KiB");
        assert_eq!(f.size(1024 * 1024), "1.0 MiB");
        assert_eq!(f.size(-2048), "-2.0 KiB");

        let si = Fmt { si: true, ..f };
        assert_eq!(si.size(999), "999 B");
        assert_eq!(si.size(1000), "1.0 kB");
        assert_eq!(si.size(1500), "1.5 kB");
        assert_eq!(si.size(1_000_000), "1.0 MB");

        let kib = Fmt { kib: true, ..f };
        assert_eq!(kib.size(512), "0.5 KiB");
        assert_eq!(kib.size(2048), "2.0 KiB");
        assert_eq!(kib.size(2 * 1024 * 1024), "2048.0 KiB");

        let si_kib = Fmt {
            si: true,
            kib: true,
            ..f
        };
        assert_eq!(si_kib.size(1500), "1.5 kB");

        let raw = Fmt {
            no_prefix: true,
            ..f
        };
        assert_eq!(raw.size(0), "0");
        assert_eq!(raw.size(2048), "2048");
    }

    #[test]
    fn count_formatting_thresholds() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1.0k");
        assert_eq!(count(1_500), "1.5k");
        assert_eq!(count(999_999), "1000.0k");
        assert_eq!(count(1_000_000), "1.0M");
        assert_eq!(count(1_000_000_000), "1.0G");
    }

    #[test]
    fn format_kib_thresholds() {
        assert_eq!(format_kib(0), "0KiB");
        assert_eq!(format_kib(1023), "1023KiB");
        assert_eq!(format_kib(1024), "1MiB");
        assert_eq!(format_kib(512 * 1024), "512MiB");
        assert_eq!(format_kib(1024 * 1024), "1.0GiB");
        assert_eq!(format_kib(3 * 1024 * 1024 / 2), "1.5GiB");
    }

    #[test]
    fn format_rate_edge_cases_and_thresholds() {
        assert_eq!(format_rate(-42.5), "0B/s");
        assert_eq!(format_rate(0.0), "0B/s");
        assert_eq!(format_rate(1023.0), "1023B/s");
        assert_eq!(format_rate(1024.0), "1.0KiB/s");
        assert_eq!(format_rate(1024.0 * 1024.0), "1.0MiB/s");
        assert_eq!(format_rate(1024.0 * 1024.0 * 1024.0), "1.0GiB/s");
    }

    #[test]
    fn format_uptime_with_and_without_days() {
        assert_eq!(format_uptime(-5.0), "00:00:00");
        assert_eq!(format_uptime(0.0), "00:00:00");
        assert_eq!(format_uptime(3661.0), "01:01:01");
        assert_eq!(format_uptime(86_399.0), "23:59:59");
        assert_eq!(format_uptime(86_400.0), "1d 00:00:00");
        assert_eq!(
            format_uptime(2.0 * 86_400.0 + 3.0 * 3_600.0 + 4.0 * 60.0 + 5.0),
            "2d 03:04:05"
        );
    }

    #[test]
    fn format_bytes_thresholds() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(1023), "1023B");
        assert_eq!(format_bytes(1024), "1K");
        assert_eq!(format_bytes(1024 * 1024), "1.0M");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0G");
        assert_eq!(format_bytes(1024_u64.pow(4)), "1.0T");
    }

    #[test]
    fn format_cpu_time_with_and_without_hours() {
        assert_eq!(format_cpu_time(0, 100), "00:00.00");
        assert_eq!(format_cpu_time(1234, 100), "00:12.34");
        assert_eq!(format_cpu_time(359_999, 100), "59:59.99");
        assert_eq!(format_cpu_time(360_000, 100), "1:00:00");
        // Zero hz clamps to 1 rather than dividing by zero.
        assert_eq!(format_cpu_time(65, 0), "01:05.00");
    }

    #[test]
    fn sanitize_control_chars_replaces_control_characters() {
        assert_eq!(sanitize_control_chars("normal_name.txt"), "normal_name.txt");
        assert_eq!(
            sanitize_control_chars("a\x1b[31mb\nc\td\x7f"),
            "a [31mb c d "
        );
        assert_eq!(sanitize_control_chars("café-🦀"), "café-🦀");
    }
}
