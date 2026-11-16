use std::process::ExitCode;

use clap::Parser;
use rssvc::cli::Args;
use rssvc::model::filter_units;
use rssvc::output::{render_json, render_table};
use rssvc::systemd::SystemdClient;
use rssvc::tui::{TuiConfig, run_tui};

fn main() -> ExitCode {
    let args = Args::parse();
    let client = SystemdClient::new(args.user, args.dry_run);

    if args.is_cli_mode() {
        let units =
            match client.list_units(args.all, args.unit_type.as_deref(), args.state.as_deref()) {
                Ok(units) => units,
                Err(err) => {
                    eprintln!("rssvc: {err}");
                    return ExitCode::FAILURE;
                }
            };

        let filtered = filter_units(
            &units,
            args.unit_type.as_deref(),
            args.state.as_deref(),
            args.filter.as_deref(),
        );

        if args.json {
            match render_json(&filtered) {
                Ok(json) => println!("{json}"),
                Err(err) => {
                    eprintln!("rssvc: {err}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            print!("{}", render_table(&filtered, args.no_color));
        }

        return ExitCode::SUCCESS;
    }

    if let Err(err) = run_tui(
        &client,
        TuiConfig {
            all_units: args.all,
            unit_type: args.unit_type,
            state_filter: args.state,
            filter_query: args.filter.unwrap_or_default(),
            user_mode: args.user,
            dry_run: args.dry_run,
            no_color: args.no_color,
        },
    ) {
        eprintln!("rssvc: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
