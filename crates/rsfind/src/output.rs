use std::fs::Metadata;
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{SystemTime, UNIX_EPOCH};

use rs_common::fmt::format_bytes;
use rs_common::sanitize_control_chars;

use crate::cli::{Args, ColorWhen};
use crate::walk::FoundEntry;

/// File kind used for ANSI color selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Dir,
    Symlink,
    Executable,
    Socket,
    Pipe,
    Regular,
}

impl FileKind {
    /// Return the ANSI SGR prefix for this file kind, or `None` for regular files.
    pub fn ansi_prefix(self) -> Option<&'static str> {
        match self {
            Self::Dir => Some("\x1b[1;34m"),
            Self::Symlink => Some("\x1b[36m"),
            Self::Executable => Some("\x1b[32m"),
            Self::Socket => Some("\x1b[35m"),
            Self::Pipe => Some("\x1b[33m"),
            Self::Regular => None,
        }
    }
}

/// Options controlling CLI stdout rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputOpts {
    pub color: bool,
    pub print0: bool,
    pub list_details: bool,
    pub absolute_path: bool,
}

impl OutputOpts {
    /// Construct `OutputOpts` from parsed CLI `Args`.
    pub fn from_args(args: &Args) -> Self {
        Self {
            color: should_use_color(args.effective_color()),
            print0: args.print0,
            list_details: args.list_details,
            absolute_path: args.absolute_path,
        }
    }
}

/// Determine whether ANSI colors should be emitted for `when`, checking `stdout` TTY
/// status and the `NO_COLOR` environment variable.
pub fn should_use_color(when: ColorWhen) -> bool {
    let is_tty = io::stdout().is_terminal();
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    should_use_color_with(when, is_tty, no_color)
}

/// Pure decision helper for color enablement (testable without mutating env/tty).
pub fn should_use_color_with(when: ColorWhen, is_tty: bool, no_color: bool) -> bool {
    match when {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        ColorWhen::Auto => is_tty && !no_color,
    }
}

/// Format a single `FoundEntry` according to `opts` (without trailing newline or NUL).
pub fn format_entry(entry: &FoundEntry, opts: OutputOpts) -> String {
    let path_str = render_path_str(entry, opts.absolute_path);
    let sanitized = sanitize_control_chars(&path_str);

    let meta = if opts.list_details {
        std::fs::symlink_metadata(&entry.path).ok()
    } else {
        None
    };

    let kind = detect_file_kind(entry, meta.as_ref());
    let colored_path = if opts.color {
        if let Some(prefix) = kind.ansi_prefix() {
            format!("{prefix}{sanitized}\x1b[0m")
        } else {
            sanitized
        }
    } else {
        sanitized
    };

    if opts.list_details {
        match meta {
            Some(ref m) => {
                let mode = format_mode(m);
                let size = format_bytes(m.len());
                let mtime = m
                    .modified()
                    .ok()
                    .map(format_mtime)
                    .unwrap_or_else(|| "---- -- -- --:--".to_string());
                format!("{mode} {size:>6} {mtime} {colored_path}")
            }
            None => format!("??????????      ? ---- -- -- --:-- {colored_path}"),
        }
    } else {
        colored_path
    }
}

/// Write a single `FoundEntry` to `writer` with `\n` or `\0` terminator.
pub fn write_entry<W: Write>(
    writer: &mut W,
    entry: &FoundEntry,
    opts: OutputOpts,
) -> io::Result<()> {
    let line = format_entry(entry, opts);
    writer.write_all(line.as_bytes())?;
    if opts.print0 {
        writer.write_all(b"\0")
    } else {
        writer.write_all(b"\n")
    }
}

/// Write a slice of `FoundEntry` items to `writer`.
pub fn write_entries<W: Write>(
    writer: &mut W,
    entries: &[FoundEntry],
    opts: OutputOpts,
) -> io::Result<()> {
    for entry in entries {
        write_entry(writer, entry, opts)?;
    }
    Ok(())
}

/// Stream entries from `rx` to buffered stdout, ignoring `BrokenPipe`.
pub fn stream_to_stdout(rx: Receiver<FoundEntry>, opts: OutputOpts) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    for entry in rx {
        if let Err(err) = write_entry(&mut writer, &entry, opts) {
            if err.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(err);
        }
    }

    match writer.flush() {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err),
    }
}

fn render_path_str(entry: &FoundEntry, absolute_path: bool) -> String {
    if absolute_path {
        let abs: PathBuf = std::path::absolute(&entry.path).unwrap_or_else(|_| entry.path.clone());
        abs.to_string_lossy().into_owned()
    } else {
        entry.display.clone()
    }
}

fn detect_file_kind(entry: &FoundEntry, meta: Option<&Metadata>) -> FileKind {
    if entry.is_symlink {
        return FileKind::Symlink;
    }
    if entry.is_dir {
        return FileKind::Dir;
    }
    if let Some(m) = meta {
        let ft = m.file_type();
        if is_socket(&ft) {
            return FileKind::Socket;
        }
        if is_pipe(&ft) {
            return FileKind::Pipe;
        }
        if is_executable(m) {
            return FileKind::Executable;
        }
    }
    if entry.is_exec {
        return FileKind::Executable;
    }
    FileKind::Regular
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

#[cfg(unix)]
fn is_executable(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && (meta.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_executable(meta: &Metadata) -> bool {
    meta.is_file()
}

#[cfg(unix)]
fn format_mode(meta: &Metadata) -> String {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let ft = meta.file_type();
    let kind_ch = if ft.is_dir() {
        'd'
    } else if ft.is_symlink() {
        'l'
    } else if ft.is_socket() {
        's'
    } else if ft.is_fifo() {
        'p'
    } else if ft.is_char_device() {
        'c'
    } else if ft.is_block_device() {
        'b'
    } else {
        '-'
    };

    let mode = meta.permissions().mode();
    let mut s = String::with_capacity(10);
    s.push(kind_ch);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if (bits & 0o4) != 0 { 'r' } else { '-' });
        s.push(if (bits & 0o2) != 0 { 'w' } else { '-' });
        s.push(if (bits & 0o1) != 0 { 'x' } else { '-' });
    }
    s
}

#[cfg(not(unix))]
fn format_mode(meta: &Metadata) -> String {
    let kind_ch = if meta.is_dir() { 'd' } else { '-' };
    let ro = if meta.permissions().readonly() {
        "r--r--r--"
    } else {
        "rw-rw-rw-"
    };
    format!("{kind_ch}{ro}")
}

fn format_mtime(ts: SystemTime) -> String {
    let secs = match ts.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}")
}

/// Howard Hinnant's civil-from-days algorithm (days since 1970-01-01 -> YYYY-MM-DD).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn make_entry(path: &Path, is_dir: bool, is_symlink: bool, is_exec: bool) -> FoundEntry {
        FoundEntry {
            path: path.to_path_buf(),
            display: path.to_string_lossy().into_owned(),
            is_dir,
            is_symlink,
            is_exec,
        }
    }

    #[test]
    fn color_decision_respects_mode_tty_and_no_color() {
        assert!(!should_use_color_with(ColorWhen::Never, true, false));
        assert!(should_use_color_with(ColorWhen::Always, false, true));
        assert!(should_use_color_with(ColorWhen::Auto, true, false));
        assert!(!should_use_color_with(ColorWhen::Auto, false, false));
        assert!(!should_use_color_with(ColorWhen::Auto, true, true));
    }

    #[test]
    fn format_entry_colors_and_strips_control_chars() {
        let dir_entry = make_entry(Path::new("src\x1b[31m/dir"), true, false, false);
        let colored = format_entry(
            &dir_entry,
            OutputOpts {
                color: true,
                print0: false,
                list_details: false,
                absolute_path: false,
            },
        );
        assert!(colored.starts_with("\x1b[1;34m"));
        assert!(colored.ends_with("\x1b[0m"));
        assert!(!colored.contains("\x1b[31m"));

        let plain = format_entry(
            &dir_entry,
            OutputOpts {
                color: false,
                print0: false,
                list_details: false,
                absolute_path: false,
            },
        );
        assert!(!plain.contains('\x1b'));
        assert_eq!(plain, "src [31m/dir");
    }

    #[test]
    fn write_entries_supports_print0_and_newlines() {
        let entries = vec![
            make_entry(Path::new("a.txt"), false, false, false),
            make_entry(Path::new("b.txt"), false, false, false),
        ];

        let mut buf0 = Vec::new();
        write_entries(
            &mut buf0,
            &entries,
            OutputOpts {
                color: false,
                print0: true,
                list_details: false,
                absolute_path: false,
            },
        )
        .unwrap();
        assert_eq!(buf0, b"a.txt\0b.txt\0");

        let mut buf_nl = Vec::new();
        write_entries(
            &mut buf_nl,
            &entries,
            OutputOpts {
                color: false,
                print0: false,
                list_details: false,
                absolute_path: false,
            },
        )
        .unwrap();
        assert_eq!(buf_nl, b"a.txt\nb.txt\n");
    }

    #[test]
    fn list_details_and_absolute_path_format() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "hello world").unwrap();

        let entry = make_entry(&file, false, false, false);
        let formatted = format_entry(
            &entry,
            OutputOpts {
                color: false,
                print0: false,
                list_details: true,
                absolute_path: true,
            },
        );
        assert!(formatted.contains("11B"));
        assert!(formatted.ends_with(&file.to_string_lossy().into_owned()));
        assert!(formatted.starts_with('-'));
    }
}
