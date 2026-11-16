use std::io::{self, BufWriter, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use clap::Parser;

use rsfind::cli::Args;
use rsfind::exec::{exec_batch, exec_parallel};
use rsfind::output::{OutputOpts, stream_to_stdout, write_entries};
use rsfind::tui::{TuiConfig, TuiOutcome, run_tui, spawn_stdin_reader};
use rsfind::walk::{WalkOpts, walk_collect, walk_stream};

fn main() -> ExitCode {
    let args = Args::parse();
    run(args)
}

fn validate_args(args: &Args) -> Result<(), String> {
    if !args.exec.is_empty() && !args.exec_batch.is_empty() {
        return Err("cannot use both -x/--exec and -X/--exec-batch".to_string());
    }
    if args.glob && args.fixed_strings {
        return Err("cannot use both -g/--glob and -F/--fixed-strings".to_string());
    }
    if args.case_sensitive && args.ignore_case {
        return Err("cannot use both -s/--case-sensitive and --ignore-case".to_string());
    }
    if args.interactive && (!args.exec.is_empty() || !args.exec_batch.is_empty()) {
        return Err(
            "cannot combine -i/--interactive with -x/--exec or -X/--exec-batch".to_string(),
        );
    }
    Ok(())
}

fn should_run_tui(args: &Args, stdin_is_tty: bool) -> bool {
    args.interactive
        || (!stdin_is_tty
            && !args.no_interactive
            && args.exec.is_empty()
            && args.exec_batch.is_empty())
}

fn run(args: Args) -> ExitCode {
    if let Err(err) = validate_args(&args) {
        eprintln!("rsfind: {err}");
        return ExitCode::from(1);
    }

    let stdin_is_tty = io::stdin().is_terminal();
    if should_run_tui(&args, stdin_is_tty) {
        return run_interactive(&args, stdin_is_tty);
    }

    run_cli(&args)
}

fn run_interactive(args: &Args, stdin_is_tty: bool) -> ExitCode {
    let config = TuiConfig::from_args(args);

    let (rx, walker_handle) = if !stdin_is_tty {
        (spawn_stdin_reader(), None)
    } else {
        let walk_opts = match WalkOpts::from_args(args) {
            Ok(opts) => opts,
            Err(err) => {
                eprintln!("rsfind: {err}");
                return ExitCode::from(1);
            }
        };
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || walk_stream(&walk_opts, tx));
        (rx, Some(handle))
    };

    let outcome = match run_tui(rx, config) {
        Ok(outcome) => outcome,
        Err(err) => {
            eprintln!("rsfind: {err}");
            return ExitCode::from(1);
        }
    };

    if let Some(handle) = walker_handle
        && let Ok(Err(err)) = handle.join()
    {
        eprintln!("rsfind: {err}");
    }

    match outcome {
        TuiOutcome::Selected(entries) => {
            let output_opts = OutputOpts::from_args(args);
            let stdout = io::stdout();
            let mut writer = BufWriter::new(stdout.lock());
            if let Err(err) = write_entries(&mut writer, &entries, output_opts) {
                if err.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("rsfind: {err}");
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }
            if let Err(err) = writer.flush()
                && err.kind() != io::ErrorKind::BrokenPipe
            {
                eprintln!("rsfind: {err}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        TuiOutcome::Aborted => ExitCode::from(130),
    }
}

fn run_cli(args: &Args) -> ExitCode {
    let walk_opts = match WalkOpts::from_args(args) {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("rsfind: {err}");
            return ExitCode::from(1);
        }
    };

    if !args.exec.is_empty() || !args.exec_batch.is_empty() {
        let entries = match walk_collect(&walk_opts) {
            Ok(entries) => entries,
            Err(err) => {
                eprintln!("rsfind: {err}");
                return ExitCode::from(1);
            }
        };

        let result = if !args.exec.is_empty() {
            exec_parallel(&args.exec, &entries)
        } else {
            exec_batch(&args.exec_batch, &entries)
        };

        return match result {
            Ok(0) => ExitCode::SUCCESS,
            Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(err) => {
                eprintln!("rsfind: {err}");
                ExitCode::from(1)
            }
        };
    }

    let output_opts = OutputOpts::from_args(args);
    let (tx, rx) = mpsc::channel();

    let result = thread::scope(|s| {
        let walker = s.spawn(|| walk_stream(&walk_opts, tx));
        let out_res = stream_to_stdout(rx, output_opts);
        let walk_res = walker.join();
        (out_res, walk_res)
    });

    match result {
        (Err(err), _) if err.kind() != io::ErrorKind::BrokenPipe => {
            eprintln!("rsfind: {err}");
            ExitCode::from(1)
        }
        (_, Ok(Err(err))) => {
            eprintln!("rsfind: {err}");
            ExitCode::from(1)
        }
        (_, Err(_)) => {
            eprintln!("rsfind: walker thread panicked");
            ExitCode::from(1)
        }
        _ => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_conflicting_flags() {
        let both_exec = Args::try_parse_from(["rsfind", "-x", "echo", ";", "-X", "echo"]).unwrap();
        assert!(validate_args(&both_exec).is_err());

        let glob_fixed = Args::try_parse_from(["rsfind", "-g", "-F", "foo"]).unwrap();
        assert!(validate_args(&glob_fixed).is_err());

        let both_case = Args::try_parse_from(["rsfind", "-s", "--ignore-case", "foo"]).unwrap();
        assert!(validate_args(&both_case).is_err());

        let interactive_exec = Args::try_parse_from(["rsfind", "-i", "-x", "echo"]).unwrap();
        assert!(validate_args(&interactive_exec).is_err());

        let valid = Args::try_parse_from(["rsfind", "-t", "f", "-e", "rs"]).unwrap();
        assert!(validate_args(&valid).is_ok());
    }

    #[test]
    fn determines_tui_vs_cli_mode() {
        let plain = Args::try_parse_from(["rsfind"]).unwrap();
        assert!(!should_run_tui(&plain, true));
        assert!(should_run_tui(&plain, false));

        let interactive = Args::try_parse_from(["rsfind", "-i"]).unwrap();
        assert!(should_run_tui(&interactive, true));
        assert!(should_run_tui(&interactive, false));

        let no_interactive = Args::try_parse_from(["rsfind", "--no-interactive"]).unwrap();
        assert!(!should_run_tui(&no_interactive, true));
        assert!(!should_run_tui(&no_interactive, false));

        let with_exec = Args::try_parse_from(["rsfind", "-x", "echo"]).unwrap();
        assert!(!should_run_tui(&with_exec, false));

        let with_exec_batch = Args::try_parse_from(["rsfind", "-X", "echo"]).unwrap();
        assert!(!should_run_tui(&with_exec_batch, false));
    }
}
