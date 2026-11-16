use std::fmt;

use clap::ValueEnum;
use regex::Regex;
use serde_json::{Map, Value};

/// Severity level for a log entry, ordered from lowest (`Trace`) to highest (`Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ValueEnum)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Returns the full uppercase level name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    /// Returns a fixed-width 3-character badge label.
    pub fn badge(self) -> &'static str {
        match self {
            Self::Trace => "TRC",
            Self::Debug => "DBG",
            Self::Info => "INF",
            Self::Warn => "WRN",
            Self::Error => "ERR",
        }
    }

    /// Maps a syslog/journald numeric priority (`0..=7`) to a [`LogLevel`].
    pub fn from_syslog_priority(priority: u8) -> Self {
        match priority {
            0..=3 => Self::Error,
            4 => Self::Warn,
            5..=6 => Self::Info,
            _ => Self::Debug,
        }
    }

    /// Parses a level token case-insensitively.
    pub fn parse_token(token: &str) -> Option<Self> {
        let trimmed = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        match trimmed.to_ascii_lowercase().as_str() {
            "err" | "error" | "crit" | "critical" | "fatal" | "emerg" | "alert" | "panic" => {
                Some(Self::Error)
            }
            "warn" | "warning" | "wrn" => Some(Self::Warn),
            "info" | "notice" | "inf" => Some(Self::Info),
            "debug" | "dbg" => Some(Self::Debug),
            "trace" | "trc" => Some(Self::Trace),
            _ => None,
        }
    }

    /// Cycles to the next level filter (`None -> Trace -> Debug -> Info -> Warn -> Error -> None`).
    pub fn cycle_filter(current: Option<Self>) -> Option<Self> {
        match current {
            None => Some(Self::Trace),
            Some(Self::Trace) => Some(Self::Debug),
            Some(Self::Debug) => Some(Self::Info),
            Some(Self::Info) => Some(Self::Warn),
            Some(Self::Warn) => Some(Self::Error),
            Some(Self::Error) => None,
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed log record from a file or systemd journald.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: Option<String>,
    pub level: Option<LogLevel>,
    pub unit: Option<String>,
    pub pid: Option<u32>,
    pub message: String,
    pub raw: String,
    pub source: String,
    pub line_number: Option<usize>,
    pub fields: Vec<(String, String)>,
}

impl LogEntry {
    /// Returns the parsed level, defaulting to [`LogLevel::Info`] for untagged lines.
    pub fn effective_level(&self) -> LogLevel {
        self.level.unwrap_or(LogLevel::Info)
    }

    /// Serializes this entry into a [`serde_json::Value`] object.
    pub fn to_json_value(&self) -> Value {
        let mut obj = Map::new();
        if let Some(ts) = &self.timestamp {
            obj.insert("timestamp".to_owned(), Value::String(ts.clone()));
        } else {
            obj.insert("timestamp".to_owned(), Value::Null);
        }
        obj.insert(
            "level".to_owned(),
            Value::String(self.effective_level().as_str().to_lowercase()),
        );
        match &self.unit {
            Some(u) => obj.insert("unit".to_owned(), Value::String(u.clone())),
            None => obj.insert("unit".to_owned(), Value::Null),
        };
        match self.pid {
            Some(p) => obj.insert("pid".to_owned(), Value::Number(p.into())),
            None => obj.insert("pid".to_owned(), Value::Null),
        };
        obj.insert("message".to_owned(), Value::String(self.message.clone()));
        obj.insert("source".to_owned(), Value::String(self.source.clone()));
        if let Some(ln) = self.line_number {
            obj.insert("line_number".to_owned(), Value::Number(ln.into()));
        }
        if !self.fields.is_empty() {
            let mut extra = Map::new();
            for (k, v) in &self.fields {
                extra.insert(k.clone(), Value::String(v.clone()));
            }
            obj.insert("fields".to_owned(), Value::Object(extra));
        }
        Value::Object(obj)
    }
}

/// Filter criteria for log entries (minimum severity + optional regex).
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub min_level: Option<LogLevel>,
    pub regex: Option<Regex>,
}

impl LogFilter {
    /// Builds a [`LogFilter`] from an optional minimum level and optional regex pattern.
    pub fn new(min_level: Option<LogLevel>, pattern: Option<&str>) -> Result<Self, regex::Error> {
        let regex = match pattern {
            Some(p) if !p.is_empty() => Some(Regex::new(p)?),
            _ => None,
        };
        Ok(Self { min_level, regex })
    }

    /// Returns `true` if `entry` satisfies both the level threshold and regex filter.
    pub fn matches(&self, entry: &LogEntry) -> bool {
        if let Some(min) = self.min_level
            && entry.effective_level() < min
        {
            return false;
        }
        if let Some(re) = &self.regex {
            let matches_msg = re.is_match(&entry.message);
            let matches_unit = entry.unit.as_deref().is_some_and(|u| re.is_match(u));
            let matches_raw = re.is_match(&entry.raw);
            if !matches_msg && !matches_unit && !matches_raw {
                return false;
            }
        }
        true
    }
}

/// Parses a plain text or syslog-formatted log line into a [`LogEntry`].
pub fn parse_log_line(line: &str, source: &str, line_number: Option<usize>) -> LogEntry {
    let raw = line.trim_end_matches(['\r', '\n']).to_owned();
    let trimmed = raw.trim_start();

    let (timestamp, rest) = extract_timestamp(trimmed);
    let (unit, pid, rest) = extract_syslog_header(rest);
    let (level, message) = extract_level_and_message(rest);

    let final_message = if message.is_empty() && unit.is_none() && timestamp.is_none() {
        raw.clone()
    } else {
        message.to_owned()
    };

    LogEntry {
        timestamp,
        level,
        unit,
        pid,
        message: final_message,
        raw,
        source: source.to_owned(),
        line_number,
        fields: Vec::new(),
    }
}

/// Parses a single `journalctl -o json` line into a [`LogEntry`].
pub fn parse_journal_json_line(line: &str) -> Result<LogEntry, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("empty journal JSON line".to_owned());
    }
    let val: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid journal JSON: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "journal JSON entry is not an object".to_owned())?;

    let message = match obj.get("MESSAGE") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => {
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        }
        Some(other) => other.to_string(),
        None => String::new(),
    };

    let priority = obj.get("PRIORITY").and_then(|v| match v {
        Value::String(s) => s.parse::<u8>().ok(),
        Value::Number(n) => n.as_u64().and_then(|u| u8::try_from(u).ok()),
        _ => None,
    });
    let level = priority.map(LogLevel::from_syslog_priority);

    let timestamp = obj
        .get("__REALTIME_TIMESTAMP")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .map(format_unix_micros);

    let unit = obj
        .get("_SYSTEMD_UNIT")
        .or_else(|| obj.get("SYSLOG_IDENTIFIER"))
        .or_else(|| obj.get("_COMM"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    let pid = obj.get("_PID").and_then(|v| match v {
        Value::String(s) => s.parse::<u32>().ok(),
        Value::Number(n) => n.as_u64().and_then(|u| u32::try_from(u).ok()),
        _ => None,
    });

    let mut fields = Vec::new();
    for key in [
        "_HOSTNAME",
        "_COMM",
        "_EXE",
        "_UID",
        "_GID",
        "SYSLOG_IDENTIFIER",
        "SYSLOG_FACILITY",
        "_SYSTEMD_UNIT",
        "__CURSOR",
    ] {
        if let Some(v) = obj.get(key) {
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            fields.push((key.to_owned(), s));
        }
    }

    Ok(LogEntry {
        timestamp,
        level,
        unit,
        pid,
        message,
        raw: trimmed.to_owned(),
        source: "journald".to_owned(),
        line_number: None,
        fields,
    })
}

/// Formats Unix epoch microseconds as UTC `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn format_unix_micros(micros: i64) -> String {
    let secs = micros.div_euclid(1_000_000);
    let millis = micros.rem_euclid(1_000_000) / 1_000;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

fn extract_timestamp(s: &str) -> (Option<String>, &str) {
    // ISO-8601 / RFC-3339: YYYY-MM-DD[T ]HH:MM:SS...
    if s.len() >= 19 {
        let bytes = s.as_bytes();
        let is_iso = bytes[0..4].iter().all(u8::is_ascii_digit)
            && bytes[4] == b'-'
            && bytes[5..7].iter().all(u8::is_ascii_digit)
            && bytes[7] == b'-'
            && bytes[8..10].iter().all(u8::is_ascii_digit)
            && (bytes[10] == b'T' || bytes[10] == b' ')
            && bytes[11..13].iter().all(u8::is_ascii_digit)
            && bytes[13] == b':'
            && bytes[14..16].iter().all(u8::is_ascii_digit)
            && bytes[16] == b':'
            && bytes[17..19].iter().all(u8::is_ascii_digit);
        if is_iso {
            let end = s.find(' ').unwrap_or(s.len());
            // Handle "YYYY-MM-DD HH:MM:SS[,mmm]" where separator at index 10 is a space.
            let ts_end = if bytes[10] == b' ' {
                s[11..]
                    .find(|c: char| c.is_whitespace())
                    .map_or(s.len(), |i| 11 + i)
            } else {
                end
            };
            let ts = s[..ts_end].to_owned();
            let rest = s[ts_end..].trim_start();
            return (Some(ts), rest);
        }
    }

    // Traditional syslog timestamp: "Mmm dd HH:MM:SS" (15 chars, day may be space-padded)
    if s.len() >= 15 && s.get(0..3).is_some_and(is_syslog_month) && s.as_bytes()[3] == b' ' {
        let day_ok = (s.as_bytes()[4] == b' ' || s.as_bytes()[4].is_ascii_digit())
            && s.as_bytes()[5].is_ascii_digit();
        let time_ok = s.as_bytes()[6] == b' '
            && s.as_bytes()[7..9].iter().all(u8::is_ascii_digit)
            && s.as_bytes()[9] == b':'
            && s.as_bytes()[10..12].iter().all(u8::is_ascii_digit)
            && s.as_bytes()[12] == b':'
            && s.as_bytes()[13..15].iter().all(u8::is_ascii_digit);
        if day_ok && time_ok {
            let ts = s[..15].to_owned();
            let rest = s[15..].trim_start();
            return (Some(ts), rest);
        }
    }

    (None, s)
}

fn is_syslog_month(m: &str) -> bool {
    matches!(
        m,
        "Jan"
            | "Feb"
            | "Mar"
            | "Apr"
            | "May"
            | "Jun"
            | "Jul"
            | "Aug"
            | "Sep"
            | "Oct"
            | "Nov"
            | "Dec"
    )
}

fn extract_syslog_header(s: &str) -> (Option<String>, Option<u32>, &str) {
    // Pattern: "<host> <unit>[<pid>]: <msg>" or "<unit>[<pid>]: <msg>"
    if let Some(colon_idx) = s.find(": ") {
        let prefix = &s[..colon_idx];
        // Avoid treating URLs or timestamps as syslog headers.
        if !prefix.contains("://") && prefix.len() <= 96 {
            let parts: Vec<&str> = prefix.split_whitespace().collect();
            if let Some(last) = parts.last()
                && parts.len() <= 2
            {
                let (unit, pid) = parse_unit_pid(last);
                if !unit.is_empty()
                    && LogLevel::parse_token(&unit).is_none()
                    && unit
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
                {
                    let rest = s[colon_idx + 2..].trim_start();
                    return (Some(unit), pid, rest);
                }
            }
        }
    }
    (None, None, s)
}

fn parse_unit_pid(s: &str) -> (String, Option<u32>) {
    if let Some(open) = s.find('[')
        && let Some(close) = s[open..].find(']')
    {
        let unit = s[..open].to_owned();
        let pid_str = &s[open + 1..open + close];
        let pid = pid_str.parse::<u32>().ok();
        return (unit, pid);
    }
    (s.to_owned(), None)
}

fn extract_level_and_message(s: &str) -> (Option<LogLevel>, &str) {
    let trimmed = s.trim_start();
    // Check key=value prefix (e.g. "level=error msg=...")
    for token in trimmed.split_whitespace().take(4) {
        if let Some((k, v)) = token.split_once('=')
            && matches!(
                k.to_ascii_lowercase().as_str(),
                "level" | "lvl" | "severity"
            )
            && let Some(lvl) = LogLevel::parse_token(v)
        {
            return (Some(lvl), trimmed);
        }
    }

    // Check first token for explicit level marker like "[ERROR]", "WARN:", "INFO"
    if let Some((first, rest)) = split_first_token(trimmed)
        && let Some(lvl) = LogLevel::parse_token(first)
    {
        let stripped = rest.trim_start_matches([':', '-', ' ']);
        return (
            Some(lvl),
            if stripped.is_empty() {
                trimmed
            } else {
                stripped
            },
        );
    }

    // Fallback: scan up to first 6 tokens for bracketed/delimited level markers
    for token in trimmed.split_whitespace().take(6) {
        if (token.starts_with('[') && token.ends_with(']')) || token.ends_with(':') {
            if let Some(lvl) = LogLevel::parse_token(token) {
                return (Some(lvl), trimmed);
            }
        }
    }

    (None, trimmed)
}

fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    match s.find(char::is_whitespace) {
        Some(idx) => Some((&s[..idx], s[idx..].trim_start())),
        None => Some((s, "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_log_line_with_level() {
        let line = "2026-11-17T02:13:52.123Z [ERROR] database connection refused";
        let entry = parse_log_line(line, "app.log", Some(42));
        assert_eq!(entry.timestamp.as_deref(), Some("2026-11-17T02:13:52.123Z"));
        assert_eq!(entry.level, Some(LogLevel::Error));
        assert_eq!(entry.message, "database connection refused");
        assert_eq!(entry.source, "app.log");
        assert_eq!(entry.line_number, Some(42));
    }

    #[test]
    fn parses_syslog_line_with_unit_and_pid() {
        let line = "Nov 17 02:13:52 myhost sshd[1234]: WARN Failed password for root";
        let entry = parse_log_line(line, "/var/log/auth.log", Some(10));
        assert_eq!(entry.timestamp.as_deref(), Some("Nov 17 02:13:52"));
        assert_eq!(entry.unit.as_deref(), Some("sshd"));
        assert_eq!(entry.pid, Some(1234));
        assert_eq!(entry.level, Some(LogLevel::Warn));
        assert_eq!(entry.message, "Failed password for root");
    }

    #[test]
    fn parses_key_value_level() {
        let line = "2026-11-17 02:13:52 level=debug msg=\"cache miss\"";
        let entry = parse_log_line(line, "svc.log", Some(1));
        assert_eq!(entry.timestamp.as_deref(), Some("2026-11-17 02:13:52"));
        assert_eq!(entry.level, Some(LogLevel::Debug));
    }

    #[test]
    fn parses_journal_json_line_standard() {
        let json = r#"{"__REALTIME_TIMESTAMP":"1700000000123456","PRIORITY":"3","_SYSTEMD_UNIT":"nginx.service","_PID":"456","MESSAGE":"upstream timed out","_HOSTNAME":"web01"}"#;
        let entry = parse_journal_json_line(json).expect("valid journal json");
        assert_eq!(entry.level, Some(LogLevel::Error));
        assert_eq!(entry.unit.as_deref(), Some("nginx.service"));
        assert_eq!(entry.pid, Some(456));
        assert_eq!(entry.message, "upstream timed out");
        assert_eq!(entry.source, "journald");
        assert!(entry.timestamp.as_deref().is_some_and(|t| t.ends_with('Z')));
        assert!(
            entry
                .fields
                .iter()
                .any(|(k, v)| k == "_HOSTNAME" && v == "web01")
        );
    }

    #[test]
    fn parses_journal_json_line_byte_array_message() {
        let json = r#"{"PRIORITY":"6","SYSLOG_IDENTIFIER":"kernel","MESSAGE":[79,75]}"#;
        let entry = parse_journal_json_line(json).expect("byte array message");
        assert_eq!(entry.level, Some(LogLevel::Info));
        assert_eq!(entry.unit.as_deref(), Some("kernel"));
        assert_eq!(entry.message, "OK");
    }

    #[test]
    fn rejects_invalid_journal_json() {
        assert!(parse_journal_json_line("not json").is_err());
        assert!(parse_journal_json_line("").is_err());
    }

    #[test]
    fn filter_checks_min_level_and_regex() {
        let filter = LogFilter::new(Some(LogLevel::Warn), Some(r"timeout|refused")).unwrap();
        let err = parse_log_line("2026-11-17T00:00:00Z ERROR connection refused", "a", None);
        let dbg = parse_log_line("2026-11-17T00:00:00Z DEBUG connection refused", "a", None);
        let warn_other = parse_log_line("2026-11-17T00:00:00Z WARN disk 80% full", "a", None);

        assert!(filter.matches(&err));
        assert!(!filter.matches(&dbg));
        assert!(!filter.matches(&warn_other));
    }

    #[test]
    fn unix_micros_formatting_epoch_and_known_date() {
        assert_eq!(format_unix_micros(0), "1970-01-01T00:00:00.000Z");
        // 2023-11-14T22:13:20.123Z = 1700000000.123456s
        assert_eq!(
            format_unix_micros(1_700_000_000_123_456),
            "2023-11-14T22:13:20.123Z"
        );
    }

    #[test]
    fn parses_multibyte_utf8_prefix_without_panicking() {
        let emoji_line = "🔥 something went wrong in worker 42";
        let entry = parse_log_line(emoji_line, "app.log", Some(1));
        assert_eq!(entry.timestamp, None);
        assert_eq!(entry.message, emoji_line);

        let accented_line = "échec de connexion au serveur distant";
        let entry2 = parse_log_line(accented_line, "app.log", Some(2));
        assert_eq!(entry2.timestamp, None);
        assert_eq!(entry2.message, accented_line);
    }
}
