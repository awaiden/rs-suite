use std::fs::Metadata;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};

use crate::cli::Args;

/// Comparison operator for file size filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeCmp {
    AtLeast,
    AtMost,
    Exact,
}

/// Filter matching file sizes (e.g. `+10M`, `-1k`, `100B`, `+1Gi`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeFilter {
    pub cmp: SizeCmp,
    pub bytes: u64,
}

impl SizeFilter {
    /// Parse a size specification such as `+10M`, `-1k`, `100B`, or `+2Gi`.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let s = spec.trim();
        if s.is_empty() {
            return Err("empty size filter".to_string());
        }

        let (cmp, rest) = if let Some(r) = s.strip_prefix('+') {
            (SizeCmp::AtLeast, r)
        } else if let Some(r) = s.strip_prefix('-') {
            (SizeCmp::AtMost, r)
        } else {
            (SizeCmp::Exact, s)
        };

        let rest = rest.trim();
        let split_idx = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());

        let (num_str, unit_str) = rest.split_at(split_idx);
        if num_str.is_empty() {
            return Err(format!("invalid size value in '{spec}'"));
        }

        let multiplier: u64 = match unit_str.trim().to_ascii_lowercase().as_str() {
            "" | "b" => 1,
            "k" | "kb" => 1_000,
            "m" | "mb" => 1_000_000,
            "g" | "gb" => 1_000_000_000,
            "t" | "tb" => 1_000_000_000_000,
            "ki" | "kib" => 1_024,
            "mi" | "mib" => 1_024 * 1_024,
            "gi" | "gib" => 1_024 * 1_024 * 1_024,
            "ti" | "tib" => 1_024 * 1_024 * 1_024 * 1_024,
            other => return Err(format!("unknown size unit '{other}' in '{spec}'")),
        };

        let bytes = if num_str.contains('.') {
            let val: f64 = num_str
                .parse()
                .map_err(|_| format!("invalid number '{num_str}' in '{spec}'"))?;
            if !val.is_finite() || val < 0.0 {
                return Err(format!("invalid size '{num_str}' in '{spec}'"));
            }
            (val * multiplier as f64).round() as u64
        } else {
            let val: u64 = num_str
                .parse()
                .map_err(|_| format!("invalid integer '{num_str}' in '{spec}'"))?;
            val.checked_mul(multiplier)
                .ok_or_else(|| format!("size overflow in '{spec}'"))?
        };

        Ok(Self { cmp, bytes })
    }

    /// Return true if `size` satisfies this filter.
    pub fn matches(&self, size: u64) -> bool {
        match self.cmp {
            SizeCmp::AtLeast => size >= self.bytes,
            SizeCmp::AtMost => size <= self.bytes,
            SizeCmp::Exact => size == self.bytes,
        }
    }
}

/// Filter matching file modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFilter {
    /// Modified at or after the cutoff timestamp.
    Within(SystemTime),
    /// Modified at or before the cutoff timestamp.
    Before(SystemTime),
}

impl TimeFilter {
    /// Parse a `--changed-within` specification relative to `now`.
    pub fn parse_within(spec: &str, now: SystemTime) -> Result<Self, String> {
        let cutoff = parse_time_or_duration(spec, now)?;
        Ok(Self::Within(cutoff))
    }

    /// Parse a `--changed-before` specification relative to `now`.
    pub fn parse_before(spec: &str, now: SystemTime) -> Result<Self, String> {
        let cutoff = parse_time_or_duration(spec, now)?;
        Ok(Self::Before(cutoff))
    }

    /// Return true if `modified` satisfies this time filter.
    pub fn matches(&self, modified: SystemTime) -> bool {
        match self {
            Self::Within(cutoff) => modified >= *cutoff,
            Self::Before(cutoff) => modified <= *cutoff,
        }
    }
}

fn parse_time_or_duration(spec: &str, now: SystemTime) -> Result<SystemTime, String> {
    let s = spec.trim();
    if s.is_empty() {
        return Err("empty time specification".to_string());
    }

    // Try ISO-8601 date / datetime first (YYYY-MM-DD or YYYY-MM-DD[T ]HH:MM:SS).
    if s.len() >= 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-') {
        if let Some(ts) = parse_iso_datetime(s) {
            return Ok(ts);
        }
    }

    // Otherwise parse as a relative duration like `10m`, `2d`, `1w`, `30s`.
    let split_idx = s
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| format!("missing duration unit in '{spec}'"))?;
    let (num_str, unit_str) = s.split_at(split_idx);
    if num_str.is_empty() {
        return Err(format!("invalid duration '{spec}'"));
    }
    let count: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid duration number in '{spec}'"))?;

    let secs_per_unit: u64 = match unit_str.trim().to_ascii_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600,
        "d" | "day" | "days" => 86_400,
        "w" | "wk" | "wks" | "week" | "weeks" => 604_800,
        "y" | "yr" | "yrs" | "year" | "years" => 31_536_000,
        other => return Err(format!("unknown time unit '{other}' in '{spec}'")),
    };

    let delta = Duration::from_secs(
        count
            .checked_mul(secs_per_unit)
            .ok_or_else(|| format!("duration overflow in '{spec}'"))?,
    );

    now.checked_sub(delta)
        .ok_or_else(|| format!("duration '{spec}' underflows UNIX epoch"))
}

fn parse_iso_datetime(s: &str) -> Option<SystemTime> {
    let date_part = &s[..10];
    let year: i64 = date_part[0..4].parse().ok()?;
    let month: u32 = date_part[5..7].parse().ok()?;
    let day: u32 = date_part[8..10].parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (hour, min, sec) = if s.len() == 10 {
        (0u64, 0u64, 0u64)
    } else {
        let sep = s.as_bytes()[10];
        if sep != b'T' && sep != b' ' {
            return None;
        }
        let time_part = &s[11..];
        let mut parts = time_part.split(':');
        let h: u64 = parts.next()?.parse().ok()?;
        let m: u64 = parts.next()?.parse().ok()?;
        let sec_str = parts.next().unwrap_or("0");
        let sec: u64 = sec_str.trim_end_matches('Z').parse().ok()?;
        if parts.next().is_some() || h > 23 || m > 59 || sec > 59 {
            return None;
        }
        (h, m, sec)
    };

    let days = days_from_civil(year, month, day);
    if days < 0 {
        return None;
    }
    let total_secs = (days as u64) * 86_400 + hour * 3_600 + min * 60 + sec;
    Some(UNIX_EPOCH + Duration::from_secs(total_secs))
}

/// Howard Hinnant's civil-from-days inverse: days since 1970-01-01.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u64;
    let m = month as u64;
    let d = day as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + (doe as i64) - 719_468
}

/// Filter matching filesystem entry types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTypeFilter {
    File,
    Directory,
    Symlink,
    Executable,
    Empty,
    Socket,
    Pipe,
}

impl FileTypeFilter {
    /// Parse a `-t/--type` value (`f`, `d`, `l`, `x`, `e`, `s`, `p` or long names).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "f" | "file" => Ok(Self::File),
            "d" | "dir" | "directory" => Ok(Self::Directory),
            "l" | "symlink" => Ok(Self::Symlink),
            "x" | "exec" | "executable" => Ok(Self::Executable),
            "e" | "empty" => Ok(Self::Empty),
            "s" | "socket" => Ok(Self::Socket),
            "p" | "fifo" | "pipe" => Ok(Self::Pipe),
            other => Err(format!("unknown file type '{other}'")),
        }
    }

    /// Return true if `path` and its `metadata` match this file type.
    pub fn matches(&self, path: &Path, metadata: &Metadata) -> bool {
        let ft = metadata.file_type();
        match self {
            Self::File => ft.is_file(),
            Self::Directory => ft.is_dir(),
            Self::Symlink => ft.is_symlink(),
            Self::Executable => is_executable(metadata),
            Self::Empty => {
                if ft.is_file() {
                    metadata.len() == 0
                } else if ft.is_dir() {
                    std::fs::read_dir(path)
                        .map(|mut entries| entries.next().is_none())
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            Self::Socket => is_socket(&ft),
            Self::Pipe => is_pipe(&ft),
        }
    }
}

#[cfg(unix)]
fn is_executable(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.is_file() && (metadata.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_executable(metadata: &Metadata) -> bool {
    metadata.is_file()
}

#[cfg(unix)]
fn is_socket(ft: &std::fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    ft.is_socket()
}

#[cfg(not(unix))]
fn is_socket(_ft: &std::fs::FileType) -> bool {
    false
}

#[cfg(unix)]
fn is_pipe(ft: &std::fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    ft.is_fifo()
}

#[cfg(not(unix))]
fn is_pipe(_ft: &std::fs::FileType) -> bool {
    false
}

/// Pattern matcher supporting regex, glob, and fixed-string modes with smart-case.
#[derive(Debug, Clone)]
pub enum PatternMatcher {
    Regex(Regex),
    Glob(GlobMatcher),
    Fixed {
        needle: String,
        case_insensitive: bool,
    },
}

impl PatternMatcher {
    /// Build a `PatternMatcher` with smart-case (case-insensitive unless uppercase is present,
    /// overridden by `case_sensitive` or `ignore_case`).
    pub fn new(
        pattern: &str,
        glob: bool,
        fixed_strings: bool,
        case_sensitive: bool,
        ignore_case: bool,
    ) -> Result<Self, String> {
        let case_insensitive = if ignore_case {
            true
        } else if case_sensitive {
            false
        } else {
            !pattern.chars().any(char::is_uppercase)
        };

        if fixed_strings {
            let needle = if case_insensitive {
                pattern.to_lowercase()
            } else {
                pattern.to_string()
            };
            return Ok(Self::Fixed {
                needle,
                case_insensitive,
            });
        }

        if glob {
            let matcher = GlobBuilder::new(pattern)
                .case_insensitive(case_insensitive)
                .build()
                .map_err(|e| format!("invalid glob pattern '{pattern}': {e}"))?
                .compile_matcher();
            return Ok(Self::Glob(matcher));
        }

        let regex = RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| format!("invalid regex pattern '{pattern}': {e}"))?;
        Ok(Self::Regex(regex))
    }

    /// Return true if `haystack` matches the pattern.
    pub fn matches(&self, haystack: &str) -> bool {
        match self {
            Self::Regex(re) => re.is_match(haystack),
            Self::Glob(glob) => glob.is_match(haystack),
            Self::Fixed {
                needle,
                case_insensitive,
            } => {
                if *case_insensitive {
                    haystack.to_lowercase().contains(needle)
                } else {
                    haystack.contains(needle)
                }
            }
        }
    }
}

/// Combined entry filter constructed from CLI arguments.
#[derive(Debug, Clone, Default)]
pub struct EntryFilter {
    pub pattern: Option<PatternMatcher>,
    pub full_path: bool,
    pub file_types: Vec<FileTypeFilter>,
    pub extensions: Vec<String>,
    pub size_filters: Vec<SizeFilter>,
    pub time_filters: Vec<TimeFilter>,
}

impl EntryFilter {
    /// Construct an `EntryFilter` from parsed CLI `Args`.
    pub fn from_args(args: &Args) -> Result<Self, String> {
        let pattern = match &args.pattern {
            Some(pat) if !pat.is_empty() => Some(PatternMatcher::new(
                pat,
                args.glob,
                args.fixed_strings,
                args.case_sensitive,
                args.ignore_case,
            )?),
            _ => None,
        };

        let file_types = args
            .file_type
            .iter()
            .map(|t| FileTypeFilter::parse(t))
            .collect::<Result<Vec<_>, _>>()?;

        let extensions = args
            .extension
            .iter()
            .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();

        let size_filters = args
            .size
            .iter()
            .map(|s| SizeFilter::parse(s))
            .collect::<Result<Vec<_>, _>>()?;

        let now = SystemTime::now();
        let mut time_filters = Vec::new();
        if let Some(within) = &args.changed_within {
            time_filters.push(TimeFilter::parse_within(within, now)?);
        }
        if let Some(before) = &args.changed_before {
            time_filters.push(TimeFilter::parse_before(before, now)?);
        }

        Ok(Self {
            pattern,
            full_path: args.full_path,
            file_types,
            extensions,
            size_filters,
            time_filters,
        })
    }

    /// Return true if evaluating this filter requires filesystem metadata.
    pub fn needs_metadata(&self) -> bool {
        !self.file_types.is_empty()
            || !self.size_filters.is_empty()
            || !self.time_filters.is_empty()
    }

    /// Return true if `path` (and optional `metadata_opt`) satisfies all configured filters.
    pub fn matches(&self, path: &Path, metadata_opt: Option<&Metadata>) -> bool {
        if let Some(matcher) = &self.pattern {
            let target = if self.full_path {
                path.to_string_lossy()
            } else {
                match path.file_name() {
                    Some(name) => name.to_string_lossy(),
                    None => path.to_string_lossy(),
                }
            };
            if !matcher.matches(&target) {
                return false;
            }
        }

        if !self.extensions.is_empty() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            match ext {
                Some(ref e) if self.extensions.iter().any(|want| want == e) => {}
                _ => return false,
            }
        }

        if !self.needs_metadata() {
            return true;
        }

        let owned_meta;
        let meta = match metadata_opt {
            Some(m) => m,
            None => match std::fs::symlink_metadata(path) {
                Ok(m) => {
                    owned_meta = m;
                    &owned_meta
                }
                Err(_) => return false,
            },
        };

        if !self.file_types.is_empty() && !self.file_types.iter().any(|ft| ft.matches(path, meta)) {
            return false;
        }

        if !self.size_filters.is_empty() {
            let len = meta.len();
            if !self.size_filters.iter().all(|sf| sf.matches(len)) {
                return false;
            }
        }

        if !self.time_filters.is_empty() {
            let Ok(mtime) = meta.modified() else {
                return false;
            };
            if !self.time_filters.iter().all(|tf| tf.matches(mtime)) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn size_filter_parses_si_and_binary_units() {
        let f1 = SizeFilter::parse("+10M").unwrap();
        assert_eq!(f1.cmp, SizeCmp::AtLeast);
        assert_eq!(f1.bytes, 10_000_000);
        assert!(f1.matches(10_000_000));
        assert!(!f1.matches(9_999_999));

        let f2 = SizeFilter::parse("-1ki").unwrap();
        assert_eq!(f2.cmp, SizeCmp::AtMost);
        assert_eq!(f2.bytes, 1024);
        assert!(f2.matches(1024));
        assert!(!f2.matches(1025));

        let f3 = SizeFilter::parse("100B").unwrap();
        assert_eq!(f3.cmp, SizeCmp::Exact);
        assert_eq!(f3.bytes, 100);
        assert!(f3.matches(100));
        assert!(!f3.matches(101));

        assert!(SizeFilter::parse("").is_err());
        assert!(SizeFilter::parse("+10Z").is_err());
    }

    #[test]
    fn time_filter_parses_durations_and_dates() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let within = TimeFilter::parse_within("2h", now).unwrap();
        assert!(within.matches(now - Duration::from_secs(3_600)));
        assert!(!within.matches(now - Duration::from_secs(7_201)));

        let before = TimeFilter::parse_before("1d", now).unwrap();
        assert!(before.matches(now - Duration::from_secs(86_400)));
        assert!(!before.matches(now - Duration::from_secs(100)));

        let date_filter = TimeFilter::parse_within("2025-01-01", now).unwrap();
        match date_filter {
            TimeFilter::Within(ts) => {
                // 2025-01-01 00:00:00 UTC = 1735689600
                assert_eq!(ts, UNIX_EPOCH + Duration::from_secs(1_735_689_600));
            }
            _ => panic!("expected Within"),
        }

        assert!(TimeFilter::parse_within("bad", now).is_err());
    }

    #[test]
    fn pattern_matcher_smart_case_glob_and_fixed() {
        // Smart-case: lowercase pattern is case-insensitive.
        let m1 = PatternMatcher::new("readme", false, false, false, false).unwrap();
        assert!(m1.matches("README.md"));
        assert!(m1.matches("readme.txt"));

        // Smart-case: uppercase in pattern makes it case-sensitive.
        let m2 = PatternMatcher::new("ReadMe", false, false, false, false).unwrap();
        assert!(m2.matches("ReadMe.md"));
        assert!(!m2.matches("readme.md"));

        // Explicit --ignore-case overrides uppercase.
        let m3 = PatternMatcher::new("ReadMe", false, false, false, true).unwrap();
        assert!(m3.matches("readme.md"));

        // Explicit --case-sensitive overrides lowercase.
        let m4 = PatternMatcher::new("readme", false, false, true, false).unwrap();
        assert!(!m4.matches("README.md"));

        // Glob mode.
        let g = PatternMatcher::new("*.rs", true, false, false, false).unwrap();
        assert!(g.matches("main.RS"));
        assert!(!g.matches("main.py"));

        // Fixed-strings mode does not treat `.` or `*` as regex metacharacters.
        let f = PatternMatcher::new("a.b*", false, true, false, false).unwrap();
        assert!(f.matches("foo_a.b*_bar"));
        assert!(!f.matches("foo_axb_bar"));
    }

    #[test]
    fn entry_filter_matches_types_extensions_and_sizes() {
        let dir = tempdir().unwrap();
        let empty_file = dir.path().join("empty.rs");
        File::create(&empty_file).unwrap();

        let non_empty = dir.path().join("data.txt");
        let mut f = File::create(&non_empty).unwrap();
        f.write_all(b"hello world").unwrap();

        let sub_dir = dir.path().join("sub");
        fs::create_dir(&sub_dir).unwrap();

        let filter_empty_rs = EntryFilter {
            file_types: vec![FileTypeFilter::Empty],
            extensions: vec!["rs".to_string()],
            ..Default::default()
        };
        assert!(filter_empty_rs.matches(&empty_file, None));
        assert!(!filter_empty_rs.matches(&non_empty, None));
        assert!(!filter_empty_rs.matches(&sub_dir, None));

        let filter_dir = EntryFilter {
            file_types: vec![FileTypeFilter::Directory],
            ..Default::default()
        };
        assert!(filter_dir.matches(&sub_dir, None));
        assert!(!filter_dir.matches(&non_empty, None));

        let filter_size = EntryFilter {
            size_filters: vec![SizeFilter::parse("+5B").unwrap()],
            ..Default::default()
        };
        assert!(filter_size.matches(&non_empty, None));
        assert!(!filter_size.matches(&empty_file, None));
    }
}
