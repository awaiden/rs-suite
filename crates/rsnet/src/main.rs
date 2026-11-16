use std::io::{self, BufWriter, Stdout, Write};
use std::panic;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
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

use rsnet::app::AppState;
use rsnet::cli::CliArgs;
use rsnet::engine::BandwidthEngine;
use rsnet::model::CaptureStatus;
use rsnet::output::{current_unix_timestamp_secs, format_json_snapshot, format_raw_snapshot};
use rsnet::packet::{CaptureError, PacketCapture, ParsedPacket};
use rsnet::ui;

const CAPTURE_BATCH_SIZE: usize = 256;
const CAPTURE_CHANNEL_CAPACITY: usize = 64;
const STATUS_TTL: Duration = Duration::from_secs(5);

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

struct CaptureHandle {
    rx: Receiver<Vec<ParsedPacket>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    fn stop_and_join(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn start_capture(
    interface: Option<&str>,
) -> Result<(CaptureStatus, Option<CaptureHandle>), String> {
    match PacketCapture::open(interface) {
        Ok(mut capture) => {
            let (tx, rx) = mpsc::sync_channel::<Vec<ParsedPacket>>(CAPTURE_CHANNEL_CAPACITY);
            let stop = Arc::new(AtomicBool::new(false));
            let stop_worker = Arc::clone(&stop);

            let thread = thread::Builder::new()
                .name("rsnet-capture".to_string())
                .spawn(move || {
                    while !stop_worker.load(Ordering::Relaxed) {
                        match capture.recv_batch(CAPTURE_BATCH_SIZE) {
                            Ok(batch) => {
                                if !batch.is_empty() {
                                    let _ = tx.try_send(batch);
                                }
                            }
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    io::ErrorKind::WouldBlock
                                        | io::ErrorKind::TimedOut
                                        | io::ErrorKind::Interrupted
                                ) => {}
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|err| format!("failed to spawn capture thread: {err}"))?;

            Ok((
                CaptureStatus::Live,
                Some(CaptureHandle {
                    rx,
                    stop,
                    thread: Some(thread),
                }),
            ))
        }
        Err(CaptureError::PermissionDenied(reason)) => {
            Ok((CaptureStatus::ProcfsFallback(reason), None))
        }
        Err(CaptureError::Io(err))
            if matches!(err.raw_os_error(), Some(libc::EPERM | libc::EACCES)) =>
        {
            Ok((CaptureStatus::ProcfsFallback(err.to_string()), None))
        }
        Err(err) => Err(err.to_string()),
    }
}

fn drain_and_refresh(engine: &mut BandwidthEngine, capture: Option<&CaptureHandle>) {
    if let Some(handle) = capture {
        while let Ok(batch) = handle.rx.try_recv() {
            engine.ingest_packets(&batch);
        }
    }
    let proc_root = Path::new("/proc");
    engine.refresh_sockets(proc_root);
    engine.refresh_interfaces_from_proc(proc_root, Instant::now());
}

fn run_stream_mode(
    args: &CliArgs,
    engine: &mut BandwidthEngine,
    capture: Option<&CaptureHandle>,
) -> io::Result<()> {
    let delay = Duration::from_millis(args.delay);
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let mut emitted = 0u64;

    loop {
        if let Some(limit) = args.count
            && emitted >= limit
        {
            break;
        }

        thread::sleep(delay);
        drain_and_refresh(engine, capture);

        let snap = engine.snapshot(args.sort, args.filter.as_deref(), args.total);
        let ts = current_unix_timestamp_secs();

        let line = if args.json {
            format_json_snapshot(&snap, ts, args.total, args.no_resolve)
                .map_err(io::Error::other)?
        } else {
            format_raw_snapshot(&snap, ts, args.unit_family, args.total, args.no_resolve)
        };

        if let Err(err) = writeln!(writer, "{line}").and_then(|()| writer.flush()) {
            if err.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(err);
        }

        emitted += 1;
    }

    Ok(())
}

fn run_tui_mode(
    args: &CliArgs,
    engine: &mut BandwidthEngine,
    capture: Option<&CaptureHandle>,
) -> io::Result<()> {
    install_panic_hook();
    let mut guard = TerminalGuard::new()?;
    let mut app = AppState::from_cli(args);

    let tick_rate = Duration::from_millis(args.delay);
    let mut next_tick = Instant::now() + tick_rate;

    drain_and_refresh(engine, capture);
    let initial_filter = (!app.filter_text.is_empty()).then_some(app.filter_text.clone());
    app.update_snapshot(engine.snapshot(
        app.sort_column,
        initial_filter.as_deref(),
        app.cumulative,
    ));

    while !app.should_quit {
        app.clear_expired_status(STATUS_TTL);

        guard.terminal.draw(|frame| {
            let area = frame.area();
            let viewport_rows = area.height.saturating_sub(6) as usize;
            app.adjust_scroll(viewport_rows.max(1));
            ui::render(frame, &app);
        })?;

        let mut timeout = next_tick.saturating_duration_since(Instant::now());
        if let Some((_, created_at)) = &app.status_message {
            let remaining = STATUS_TTL.saturating_sub(created_at.elapsed());
            timeout = timeout.min(remaining);
        }

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && app.handle_key_event(key)
        {
            break;
        }

        let now = Instant::now();
        if now >= next_tick {
            drain_and_refresh(engine, capture);
            let filter = (!app.filter_text.is_empty()).then_some(app.filter_text.clone());
            let snap = engine.snapshot(app.sort_column, filter.as_deref(), app.cumulative);
            app.update_snapshot(snap);
            next_tick = now + tick_rate;
        }
    }

    Ok(())
}

fn main() {
    let args = CliArgs::parse();

    let (status, capture) = match start_capture(args.interface.as_deref()) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("rsnet: {err}");
            std::process::exit(1);
        }
    };

    let mut engine = BandwidthEngine::new(args.interface.clone(), args.no_resolve, status);
    let proc_root = Path::new("/proc");
    engine.refresh_sockets(proc_root);
    engine.refresh_interfaces_from_proc(proc_root, Instant::now());

    let result = if args.raw || args.json {
        run_stream_mode(&args, &mut engine, capture.as_ref())
    } else {
        run_tui_mode(&args, &mut engine, capture.as_ref())
    };

    if let Some(handle) = capture {
        handle.stop_and_join();
    }

    if let Err(err) = result {
        eprintln!("rsnet: {err}");
        std::process::exit(1);
    }
}
