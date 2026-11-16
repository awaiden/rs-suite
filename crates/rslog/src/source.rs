use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use crate::cli::Args;
use crate::model::{LogEntry, parse_journal_json_line, parse_log_line};

/// Errors produced when reading or streaming log sources.
#[derive(Debug)]
pub enum SourceError {
    /// File I/O failure for a specific path.
    Io(PathBuf, io::Error),
    /// `journalctl` binary was not found in `PATH`.
    JournalctlNotFound(String),
    /// `journalctl` exited with an error status or failed to start.
    JournalctlFailed(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, err) => write!(f, "{}: {err}", path.display()),
            Self::JournalctlNotFound(bin) => write!(
                f,
                "'{bin}' was not found in PATH; systemd journald is unavailable (pass log file paths instead)"
            ),
            Self::JournalctlFailed(msg) => write!(f, "journalctl failed: {msg}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// Reads and incrementally tails a log file by byte offset.
#[derive(Debug, Clone)]
pub struct FileTailer {
    path: PathBuf,
    offset: u64,
    line_number: usize,
}

impl FileTailer {
    /// Creates a new [`FileTailer`] for `path` starting at byte offset 0.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            offset: 0,
            line_number: 0,
        }
    }

    /// Returns the tailed file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the current byte offset.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Reads the initial contents of the file, optionally returning only the last `tail_lines`.
    /// Advances `offset` to the end of the file so subsequent [`Self::poll`] calls only read new data.
    pub fn read_initial(
        &mut self,
        tail_lines: Option<usize>,
    ) -> Result<Vec<LogEntry>, SourceError> {
        let mut file = File::open(&self.path).map_err(|e| SourceError::Io(self.path.clone(), e))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| SourceError::Io(self.path.clone(), e))?;

        self.offset = buf.len() as u64;
        let text = String::from_utf8_lossy(&buf);
        let source = self.path.display().to_string();

        let mut entries = Vec::new();
        for line in text.lines() {
            self.line_number += 1;
            entries.push(parse_log_line(line, &source, Some(self.line_number)));
        }

        if let Some(n) = tail_lines
            && entries.len() > n
        {
            entries.drain(..entries.len() - n);
        }

        Ok(entries)
    }

    /// Polls for newly appended complete lines since the last read.
    /// Automatically resets to offset 0 if the file was truncated or rotated.
    pub fn poll(&mut self) -> Result<Vec<LogEntry>, SourceError> {
        let mut file = File::open(&self.path).map_err(|e| SourceError::Io(self.path.clone(), e))?;
        let len = file
            .metadata()
            .map_err(|e| SourceError::Io(self.path.clone(), e))?
            .len();

        if len < self.offset {
            self.offset = 0;
            self.line_number = 0;
        }
        if len == self.offset {
            return Ok(Vec::new());
        }

        file.seek(SeekFrom::Start(self.offset))
            .map_err(|e| SourceError::Io(self.path.clone(), e))?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| SourceError::Io(self.path.clone(), e))?;

        // Only consume up to the last newline so partial writes at EOF are not split.
        let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else {
            return Ok(Vec::new());
        };

        let consumed = &buf[..=last_nl];
        self.offset += consumed.len() as u64;
        let text = String::from_utf8_lossy(consumed);
        let source = self.path.display().to_string();

        let mut entries = Vec::new();
        for line in text.lines() {
            self.line_number += 1;
            entries.push(parse_log_line(line, &source, Some(self.line_number)));
        }
        Ok(entries)
    }
}

/// Configuration for invoking `journalctl -o json`.
#[derive(Debug, Clone)]
pub struct JournalOpts {
    pub program: String,
    pub units: Vec<String>,
    pub lines: usize,
    pub since: Option<String>,
    pub until: Option<String>,
    pub follow: bool,
}

impl JournalOpts {
    /// Builds [`JournalOpts`] from parsed CLI [`Args`].
    pub fn from_args(args: &Args) -> Self {
        Self {
            program: "journalctl".to_owned(),
            units: args.units.clone(),
            lines: args.lines.unwrap_or(200),
            since: args.since.clone(),
            until: args.until.clone(),
            follow: args.follow,
        }
    }
}

/// Builds the CLI argument list for `journalctl`.
pub fn build_journalctl_args(opts: &JournalOpts) -> Vec<String> {
    let mut cmd_args = vec![
        "-o".to_owned(),
        "json".to_owned(),
        "--no-pager".to_owned(),
        "-n".to_owned(),
        opts.lines.to_string(),
    ];
    for unit in &opts.units {
        cmd_args.push("-u".to_owned());
        cmd_args.push(unit.clone());
    }
    if let Some(since) = &opts.since {
        cmd_args.push("--since".to_owned());
        cmd_args.push(since.clone());
    }
    if let Some(until) = &opts.until {
        cmd_args.push("--until".to_owned());
        cmd_args.push(until.clone());
    }
    if opts.follow {
        cmd_args.push("-f".to_owned());
    }
    cmd_args
}

/// Reads a snapshot of journald entries via `journalctl -o json`.
pub fn read_journal(opts: &JournalOpts) -> Result<Vec<LogEntry>, SourceError> {
    let mut snapshot_opts = opts.clone();
    snapshot_opts.follow = false;
    let cmd_args = build_journalctl_args(&snapshot_opts);

    let output = Command::new(&opts.program)
        .args(&cmd_args)
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                SourceError::JournalctlNotFound(opts.program.clone())
            } else {
                SourceError::JournalctlFailed(e.to_string())
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let msg = if stderr.is_empty() {
            format!("exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(SourceError::JournalctlFailed(msg));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if let Ok(entry) = parse_journal_json_line(line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Events emitted by a background [`JournalStream`].
#[derive(Debug)]
pub enum JournalEvent {
    Entry(LogEntry),
    Error(String),
    Eof,
}

/// Streaming `journalctl -o json -f` reader running on a background thread.
pub struct JournalStream {
    child: Child,
    rx: Receiver<JournalEvent>,
    handle: Option<JoinHandle<()>>,
}

impl JournalStream {
    /// Spawns `journalctl -o json -f` and starts streaming parsed [`LogEntry`] items.
    pub fn spawn(opts: &JournalOpts) -> Result<Self, SourceError> {
        let mut follow_opts = opts.clone();
        follow_opts.follow = true;
        let cmd_args = build_journalctl_args(&follow_opts);

        let mut child = Command::new(&opts.program)
            .args(&cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    SourceError::JournalctlNotFound(opts.program.clone())
                } else {
                    SourceError::JournalctlFailed(e.to_string())
                }
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SourceError::JournalctlFailed("failed to capture stdout".to_owned()))?;
        let stderr = child.stderr.take();

        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut saw_any = false;
            for line_res in reader.lines() {
                match line_res {
                    Ok(line) => {
                        if let Ok(entry) = parse_journal_json_line(&line) {
                            saw_any = true;
                            if tx.send(JournalEvent::Entry(entry)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(JournalEvent::Error(err.to_string()));
                        break;
                    }
                }
            }
            if !saw_any && let Some(mut err_pipe) = stderr {
                let mut err_buf = String::new();
                if err_pipe.read_to_string(&mut err_buf).is_ok() {
                    let trimmed = err_buf.trim();
                    if !trimmed.is_empty() {
                        let _ = tx.send(JournalEvent::Error(trimmed.to_owned()));
                    }
                }
            }
            let _ = tx.send(JournalEvent::Eof);
        });

        Ok(Self {
            child,
            rx,
            handle: Some(handle),
        })
    }

    /// Returns the receiver for incoming [`JournalEvent`] items.
    pub fn receiver(&self) -> &Receiver<JournalEvent> {
        &self.rx
    }
}

impl Drop for JournalStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::model::LogLevel;

    #[test]
    fn file_tailer_initial_tail_and_incremental_poll() {
        let mut tmp = NamedTempFile::new().expect("create tempfile");
        writeln!(tmp, "2026-11-17T00:00:01Z INFO first").unwrap();
        writeln!(tmp, "2026-11-17T00:00:02Z WARN second").unwrap();
        writeln!(tmp, "2026-11-17T00:00:03Z ERROR third").unwrap();
        tmp.flush().unwrap();

        let mut tailer = FileTailer::new(tmp.path());
        let initial = tailer.read_initial(Some(2)).expect("initial read");
        assert_eq!(initial.len(), 2);
        assert_eq!(initial[0].message, "second");
        assert_eq!(initial[1].message, "third");
        assert_eq!(initial[1].level, Some(LogLevel::Error));

        // Polling with no changes yields empty.
        assert!(tailer.poll().expect("empty poll").is_empty());

        // Partial line without newline should not be emitted yet.
        write!(tmp, "2026-11-17T00:00:04Z DEBUG partial").unwrap();
        tmp.flush().unwrap();
        assert!(tailer.poll().expect("partial poll").is_empty());

        // Completing the line emits it.
        writeln!(tmp, " complete").unwrap();
        tmp.flush().unwrap();
        let polled = tailer.poll().expect("complete poll");
        assert_eq!(polled.len(), 1);
        assert_eq!(polled[0].level, Some(LogLevel::Debug));
        assert_eq!(polled[0].message, "partial complete");
        assert_eq!(polled[0].line_number, Some(4));
    }

    #[test]
    fn file_tailer_handles_truncation() {
        let mut tmp = NamedTempFile::new().expect("create tempfile");
        writeln!(tmp, "2026-11-17T00:00:01Z INFO line one is long enough").unwrap();
        writeln!(tmp, "2026-11-17T00:00:02Z INFO line two is also long").unwrap();
        tmp.flush().unwrap();

        let mut tailer = FileTailer::new(tmp.path());
        let initial = tailer.read_initial(None).expect("initial read");
        assert_eq!(initial.len(), 2);
        assert!(tailer.offset() > 20);

        // Truncate and write shorter content.
        tmp.as_file().set_len(0).unwrap();
        tmp.as_file().seek(SeekFrom::Start(0)).unwrap();
        writeln!(tmp, "2026-11-17T00:00:03Z WARN rotated").unwrap();
        tmp.flush().unwrap();

        let after_truncate = tailer.poll().expect("poll after truncate");
        assert_eq!(after_truncate.len(), 1);
        assert_eq!(after_truncate[0].level, Some(LogLevel::Warn));
        assert_eq!(after_truncate[0].message, "rotated");
        assert_eq!(after_truncate[0].line_number, Some(1));
    }

    #[test]
    fn journalctl_absent_returns_graceful_error() {
        let opts = JournalOpts {
            program: "rslog-nonexistent-journalctl-binary".to_owned(),
            units: vec!["nginx".to_owned()],
            lines: 50,
            since: Some("1 hour ago".to_owned()),
            until: None,
            follow: false,
        };
        let err = read_journal(&opts).expect_err("missing binary should error");
        assert!(matches!(err, SourceError::JournalctlNotFound(_)));
        assert!(err.to_string().contains("not found"));

        let stream_err = JournalStream::spawn(&opts)
            .err()
            .expect("missing binary stream should error");
        assert!(matches!(stream_err, SourceError::JournalctlNotFound(_)));
    }

    #[test]
    fn build_journalctl_args_includes_filters() {
        let opts = JournalOpts {
            program: "journalctl".to_owned(),
            units: vec!["sshd".to_owned(), "nginx".to_owned()],
            lines: 100,
            since: Some("today".to_owned()),
            until: Some("now".to_owned()),
            follow: true,
        };
        let args = build_journalctl_args(&opts);
        assert_eq!(
            args,
            vec![
                "-o",
                "json",
                "--no-pager",
                "-n",
                "100",
                "-u",
                "sshd",
                "-u",
                "nginx",
                "--since",
                "today",
                "--until",
                "now",
                "-f",
            ]
        );
    }
}
