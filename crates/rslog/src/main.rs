use std::io::{self, BufWriter, ErrorKind, IsTerminal, Write};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use clap::Parser;

use rslog::cli::Args;
use rslog::model::{LogEntry, LogFilter};
use rslog::output::{OutputOpts, write_entries, write_entry};
use rslog::source::{FileTailer, JournalEvent, JournalOpts, JournalStream, read_journal};
use rslog::tui::{AppState, run_tui};

fn main() -> ExitCode {
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> ExitCode {
    if let Err(err) = args.validate() {
        eprintln!("rslog: {err}");
        return ExitCode::from(2);
    }

    let filter = match LogFilter::new(args.level, args.grep.as_deref()) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("rslog: invalid regex: {err}");
            return ExitCode::from(2);
        }
    };

    let cli_mode = args.cli || args.json || !io::stdout().is_terminal();

    if args.should_read_journal() {
        run_journal_mode(&args, filter, cli_mode)
    } else {
        run_file_mode(&args, filter, cli_mode)
    }
}

fn run_journal_mode(args: &Args, filter: LogFilter, cli_mode: bool) -> ExitCode {
    let opts = JournalOpts::from_args(args);

    if cli_mode {
        let out_opts = OutputOpts::from_args(args);
        if args.follow {
            let stream = match JournalStream::spawn(&opts) {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("rslog: {err}");
                    return ExitCode::from(2);
                }
            };
            return stream_journal_cli(&stream, &filter, &out_opts);
        }

        let entries = match read_journal(&opts) {
            Ok(e) => e,
            Err(err) => {
                eprintln!("rslog: {err}");
                return ExitCode::from(2);
            }
        };
        let filtered: Vec<LogEntry> = entries.into_iter().filter(|e| filter.matches(e)).collect();
        return write_cli_batch(&filtered, &out_opts);
    }

    let initial = match read_journal(&opts) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("rslog: {err}");
            return ExitCode::from(2);
        }
    };

    let stream = if args.follow {
        let mut follow_opts = opts.clone();
        follow_opts.lines = 0;
        match JournalStream::spawn(&follow_opts) {
            Ok(s) => Some(s),
            Err(err) => {
                eprintln!("rslog: {err}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };

    let source_label = if args.units.is_empty() {
        "journald".to_owned()
    } else {
        format!("journald ({})", args.units.join(", "))
    };
    let initial_regex = args.grep.clone().unwrap_or_default();
    let mut state = AppState::new(
        source_label,
        initial,
        filter,
        initial_regex,
        args.follow,
        args.no_color,
    );

    if let Err(err) = run_tui(&mut state, Vec::new(), stream) {
        eprintln!("rslog: {err}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn run_file_mode(args: &Args, filter: LogFilter, cli_mode: bool) -> ExitCode {
    let mut tailers = Vec::with_capacity(args.files.len());
    let mut initial = Vec::new();

    for path in &args.files {
        let mut tailer = FileTailer::new(path);
        match tailer.read_initial(args.lines) {
            Ok(entries) => initial.extend(entries),
            Err(err) => {
                eprintln!("rslog: {err}");
                return ExitCode::from(2);
            }
        }
        tailers.push(tailer);
    }

    if cli_mode {
        let out_opts = OutputOpts::from_args(args);
        let filtered: Vec<LogEntry> = initial.into_iter().filter(|e| filter.matches(e)).collect();
        let code = write_cli_batch(&filtered, &out_opts);
        if code != ExitCode::SUCCESS || !args.follow {
            return code;
        }
        return follow_files_cli(&mut tailers, &filter, &out_opts);
    }

    let source_label = args
        .files
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let initial_regex = args.grep.clone().unwrap_or_default();
    let mut state = AppState::new(
        source_label,
        initial,
        filter,
        initial_regex,
        args.follow,
        args.no_color,
    );

    if let Err(err) = run_tui(&mut state, tailers, None) {
        eprintln!("rslog: {err}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn write_cli_batch(entries: &[LogEntry], out_opts: &OutputOpts) -> ExitCode {
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    if let Err(err) = write_entries(&mut writer, entries, out_opts)
        && err.kind() != ErrorKind::BrokenPipe
    {
        eprintln!("rslog: {err}");
        return ExitCode::from(2);
    }
    if let Err(err) = writer.flush()
        && err.kind() != ErrorKind::BrokenPipe
    {
        eprintln!("rslog: {err}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn stream_journal_cli(
    stream: &JournalStream,
    filter: &LogFilter,
    out_opts: &OutputOpts,
) -> ExitCode {
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    for ev in stream.receiver() {
        match ev {
            JournalEvent::Entry(entry) => {
                if filter.matches(&entry) {
                    if let Err(err) = write_entry(&mut writer, &entry, out_opts) {
                        if err.kind() == ErrorKind::BrokenPipe {
                            return ExitCode::SUCCESS;
                        }
                        eprintln!("rslog: {err}");
                        return ExitCode::from(2);
                    }
                    if let Err(err) = writer.flush() {
                        if err.kind() == ErrorKind::BrokenPipe {
                            return ExitCode::SUCCESS;
                        }
                        eprintln!("rslog: {err}");
                        return ExitCode::from(2);
                    }
                }
            }
            JournalEvent::Error(msg) => {
                eprintln!("rslog: {msg}");
                return ExitCode::from(2);
            }
            JournalEvent::Eof => break,
        }
    }
    ExitCode::SUCCESS
}

fn follow_files_cli(
    tailers: &mut [FileTailer],
    filter: &LogFilter,
    out_opts: &OutputOpts,
) -> ExitCode {
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    loop {
        for tailer in tailers.iter_mut() {
            match tailer.poll() {
                Ok(entries) => {
                    for entry in entries {
                        if filter.matches(&entry) {
                            if let Err(err) = write_entry(&mut writer, &entry, out_opts) {
                                if err.kind() == ErrorKind::BrokenPipe {
                                    return ExitCode::SUCCESS;
                                }
                                eprintln!("rslog: {err}");
                                return ExitCode::from(2);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("rslog: {err}");
                    return ExitCode::from(2);
                }
            }
        }
        if let Err(err) = writer.flush() {
            if err.kind() == ErrorKind::BrokenPipe {
                return ExitCode::SUCCESS;
            }
            eprintln!("rslog: {err}");
            return ExitCode::from(2);
        }
        thread::sleep(Duration::from_millis(150));
    }
}
