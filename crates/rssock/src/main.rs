use std::io::{self, Stdout, Write};
use std::panic;
use std::time::{Duration, Instant};

use clap::Parser;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use rssock::app::AppState;
use rssock::cli::CliArgs;
use rssock::model::{SummaryStats, filter_and_sort};
use rssock::output::{format_json_sockets, format_json_summary, format_summary, format_table};
use rssock::procfs::{CollectOptions, collect_system_sockets};
use rssock::ui;

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err);
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(term) => term,
            Err(err) => {
                let _ = restore_terminal();
                return Err(err);
            }
        };
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore_terminal();
        let _ = self.terminal.show_cursor();
    }
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, Show)?;
    Ok(())
}

fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        default_hook(panic_info);
    }));
}

fn run_tui_mode(args: &CliArgs) -> io::Result<()> {
    install_panic_hook();
    let mut guard = TerminalGuard::new()?;
    let mut app = AppState::from_cli(args);

    let collect_opts = CollectOptions {
        tcp: true,
        udp: true,
        unix: true,
        with_processes: true,
    };

    let tick_rate = Duration::from_millis(args.delay);
    let mut next_tick = Instant::now() + tick_rate;

    app.update_records(collect_system_sockets(collect_opts));

    while !app.should_quit {
        guard.terminal.draw(|frame| {
            app.page_size = frame.area().height.saturating_sub(6).max(1) as usize;
            ui::render(frame, &app);
        })?;

        let timeout = next_tick.saturating_duration_since(Instant::now());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && app.handle_key(key)
        {
            break;
        }

        let now = Instant::now();
        if now >= next_tick {
            if !app.paused {
                app.update_records(collect_system_sockets(collect_opts));
            }
            next_tick = now + tick_rate;
        }
    }

    Ok(())
}

fn run_oneshot_mode(args: &CliArgs) -> io::Result<()> {
    let records = collect_system_sockets(args.collect_options());
    let filtered = filter_and_sort(
        &records,
        &args.socket_filter(),
        args.sort.into(),
        args.reverse,
    );

    let output = if args.summary {
        let stats = SummaryStats::from_records(&filtered);
        if args.json {
            format_json_summary(&stats)
        } else {
            format_summary(&stats, args.no_color)
        }
    } else if args.json {
        format_json_sockets(&filtered, args.numeric)
    } else {
        format_table(&filtered, args.processes, args.numeric, args.no_color)
    };

    let mut stdout = io::stdout().lock();
    if let Err(err) = writeln!(stdout, "{output}").and_then(|()| stdout.flush()) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(err);
    }

    Ok(())
}

fn main() {
    let args = CliArgs::parse();

    let result = if args.tui {
        run_tui_mode(&args)
    } else {
        run_oneshot_mode(&args)
    };

    if let Err(err) = result {
        eprintln!("rssock: {err}");
        std::process::exit(1);
    }
}
