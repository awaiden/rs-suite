mod render;

use clap::Parser;
use rsfetch::cli::CliArgs;
use rsfetch::logo::{select_logo, supported_logo_names};
use rsfetch::system::{ModuleId, collect_system_report};

fn main() {
    let mut args = CliArgs::parse();
    let env_no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    args.no_color = args.no_color || env_no_color;

    if args.list_logos {
        for name in supported_logo_names() {
            println!("{name}");
        }
        return;
    }

    if args.list_modules {
        for id in ModuleId::ALL {
            println!("{}", id.as_str());
        }
        return;
    }

    let report = collect_system_report(&args);
    let id_like = if report.distro_id_like.is_empty() {
        None
    } else {
        Some(report.distro_id_like.join(" "))
    };
    let logo = select_logo(
        &report.distro_id,
        id_like.as_deref(),
        args.logo.as_deref(),
        args.logo_type,
        args.no_logo,
    );

    if args.json {
        println!("{}", render::render_json(&report, logo));
    } else {
        println!(
            "{}",
            render::render_text(&report, logo, args.logo_type, args.no_color)
        );
    }
}
