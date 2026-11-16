use std::io::{self, BufWriter, ErrorKind, Write};
use std::process::ExitCode;

use clap::Parser;

use rsgrep::cli::Args;
use rsgrep::output::{OutputOpts, write_results};
use rsgrep::search::{SearchOpts, search};
use rsgrep::tui::{TuiOutcome, run_tui};

fn main() -> ExitCode {
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> ExitCode {
    if let Err(err) = args.validate() {
        eprintln!("rsgrep: {err}");
        return ExitCode::from(2);
    }

    if args.interactive {
        return match run_tui(&args) {
            Ok(TuiOutcome::Selected(m)) => {
                println!("{}:{}", m.path.display(), m.line_number);
                ExitCode::SUCCESS
            }
            Ok(TuiOutcome::Cancelled) => ExitCode::from(130),
            Err(err) => {
                eprintln!("rsgrep: {err}");
                ExitCode::from(2)
            }
        };
    }

    let pattern = args.pattern.clone().unwrap_or_default();
    let search_opts = SearchOpts::from_args(&args, pattern);
    let results = match search(&search_opts) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("rsgrep: {err}");
            return ExitCode::from(2);
        }
    };

    let out_opts = OutputOpts::from_args(&args);
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    if let Err(err) = write_results(&mut writer, &results, &out_opts)
        && err.kind() != ErrorKind::BrokenPipe
    {
        eprintln!("rsgrep: {err}");
        return ExitCode::from(2);
    }
    if let Err(err) = writer.flush()
        && err.kind() != ErrorKind::BrokenPipe
    {
        eprintln!("rsgrep: {err}");
        return ExitCode::from(2);
    }

    if results.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
