use std::io::{self, Write};

use rs_common::sanitize_control_chars;

use crate::cli::Args;
use crate::model::{LogEntry, LogLevel};

/// Options controlling non-interactive CLI output formatting.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputOpts {
    pub json: bool,
    pub no_color: bool,
}

impl OutputOpts {
    /// Builds [`OutputOpts`] from parsed CLI [`Args`].
    pub fn from_args(args: &Args) -> Self {
        Self {
            json: args.json,
            no_color: args.no_color,
        }
    }
}

/// Writes a slice of [`LogEntry`] items using the configured [`OutputOpts`].
pub fn write_entries<W: Write>(
    writer: &mut W,
    entries: &[LogEntry],
    opts: &OutputOpts,
) -> io::Result<()> {
    for entry in entries {
        write_entry(writer, entry, opts)?;
    }
    Ok(())
}

/// Writes a single [`LogEntry`] as JSON-lines or plain/ANSI-colored text.
pub fn write_entry<W: Write>(
    writer: &mut W,
    entry: &LogEntry,
    opts: &OutputOpts,
) -> io::Result<()> {
    if opts.json {
        serde_json::to_writer(&mut *writer, &entry.to_json_value()).map_err(io::Error::other)?;
        return writeln!(writer);
    }

    let level = entry.effective_level();
    let badge = level.badge();

    if let Some(ts) = &entry.timestamp {
        let clean_ts = sanitize_control_chars(ts);
        if opts.no_color {
            write!(writer, "{clean_ts} ")?;
        } else {
            write!(writer, "\x1b[90m{clean_ts}\x1b[0m ")?;
        }
    }

    if opts.no_color {
        write!(writer, "[{badge}] ")?;
    } else {
        let color = level_ansi_color(level);
        write!(writer, "{color}[{badge}]\x1b[0m ")?;
    }

    if let Some(unit) = &entry.unit {
        let clean_unit = sanitize_control_chars(unit);
        if opts.no_color {
            match entry.pid {
                Some(pid) => write!(writer, "{clean_unit}[{pid}]: ")?,
                None => write!(writer, "{clean_unit}: ")?,
            }
        } else {
            match entry.pid {
                Some(pid) => write!(writer, "\x1b[36m{clean_unit}[{pid}]\x1b[0m: ")?,
                None => write!(writer, "\x1b[36m{clean_unit}\x1b[0m: ")?,
            }
        }
    }

    let clean_msg = sanitize_control_chars(&entry.message);
    writeln!(writer, "{clean_msg}")
}

fn level_ansi_color(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "\x1b[1;31m",
        LogLevel::Warn => "\x1b[33m",
        LogLevel::Info => "\x1b[32m",
        LogLevel::Debug => "\x1b[36m",
        LogLevel::Trace => "\x1b[90m",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_log_line;

    #[test]
    fn writes_plain_and_sanitizes_control_chars() {
        let entry = parse_log_line(
            "2026-11-17T00:00:00Z ERROR \x1b[31minjected\x1b[0m message",
            "app.log",
            Some(1),
        );
        let mut buf = Vec::new();
        write_entry(
            &mut buf,
            &entry,
            &OutputOpts {
                json: false,
                no_color: true,
            },
        )
        .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(!out.contains('\x1b'));
        assert!(out.contains("2026-11-17T00:00:00Z [ERR]"));
        assert!(out.contains("injected"));
    }

    #[test]
    fn writes_colored_when_enabled() {
        let entry = parse_log_line("2026-11-17T00:00:00Z WARN disk full", "app.log", Some(1));
        let mut buf = Vec::new();
        write_entry(
            &mut buf,
            &entry,
            &OutputOpts {
                json: false,
                no_color: false,
            },
        )
        .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("\x1b[33m[WRN]\x1b[0m"));
        assert!(out.contains("disk full"));
    }

    #[test]
    fn writes_json_lines() {
        let entries = vec![
            parse_log_line("2026-11-17T00:00:00Z INFO started", "app.log", Some(1)),
            parse_log_line("2026-11-17T00:00:01Z ERROR failed", "app.log", Some(2)),
        ];
        let mut buf = Vec::new();
        write_entries(
            &mut buf,
            &entries,
            &OutputOpts {
                json: true,
                no_color: true,
            },
        )
        .unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        let v0: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v0["level"], "info");
        assert_eq!(v0["message"], "started");
        assert_eq!(v0["line_number"], 1);
    }
}
