use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use rs_common::sanitize_control_chars;
use rs_common::tui::Theme;

use crate::app::{AppState, InputMode, unit_family_label, view_mode_label};
use crate::cli::{SortColumn, UnitFamily, ViewMode};
use crate::model::{CaptureStatus, format_bytes_with_unit, format_rate_with_unit};

/// Renders the complete `rsnet` TUI frame: header, traffic panes, footer, and active overlays.
pub fn render(frame: &mut Frame, app: &AppState) {
    let theme = Theme::new(app.no_color);
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let is_fallback = matches!(app.snapshot.status, CaptureStatus::ProcfsFallback(_));
    let header_height = if area.height < 6 {
        1
    } else if is_fallback {
        3
    } else {
        2
    };

    let footer_height = if area.height < 4 { 0 } else { 1 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(footer_height),
        ])
        .split(area);

    render_header(frame, chunks[0], app, &theme);
    render_body(frame, chunks[1], app, &theme);
    if footer_height > 0 {
        render_footer(frame, chunks[2], app, &theme);
    }

    if app.input_mode == InputMode::Help {
        render_help_overlay(frame, area, &theme);
    }
}

fn format_counter(bytes: u64, unit: UnitFamily, cumulative: bool) -> String {
    if cumulative {
        format_bytes_with_unit(bytes, unit)
    } else {
        format_rate_with_unit(bytes as f64, unit)
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut top_spans = vec![Span::styled(" rsnet ", theme.title), Span::raw(" ")];

    match &app.snapshot.status {
        CaptureStatus::Live => {
            top_spans.push(Span::styled("[LIVE]", theme.proc_running));
        }
        CaptureStatus::ProcfsFallback(_) => {
            top_spans.push(Span::styled("[PROCFS-ONLY]", theme.status_alert));
        }
    }

    top_spans.push(Span::raw(" "));
    top_spans.push(Span::styled(
        format!("[View:{}]", view_mode_label(app.view_mode)),
        theme.accent,
    ));
    top_spans.push(Span::raw(" "));
    top_spans.push(Span::styled(
        format!("[{}]", unit_family_label(app.unit_family)),
        theme.muted,
    ));

    if app.cumulative {
        top_spans.push(Span::raw(" "));
        top_spans.push(Span::styled("[Total]", theme.accent));
    }
    if app.paused {
        top_spans.push(Span::raw(" "));
        top_spans.push(Span::styled("[Paused]", theme.status_alert));
    }
    if app.no_resolve {
        top_spans.push(Span::raw(" "));
        top_spans.push(Span::styled("[NoDNS]", theme.muted));
    }
    if !app.filter_text.is_empty() {
        top_spans.push(Span::raw(" "));
        top_spans.push(Span::styled(
            format!("[Filter:{}]", sanitize_control_chars(&app.filter_text)),
            theme.accent,
        ));
    }

    let mut lines = vec![Line::from(top_spans)];

    if area.height >= 2 {
        let total_rx = format_counter(app.snapshot.total.rx_bytes, app.unit_family, app.cumulative);
        let total_tx = format_counter(app.snapshot.total.tx_bytes, app.unit_family, app.cumulative);
        let mut summary = format!(" Total: Rx {total_rx} | Tx {total_tx}");

        let active_ifaces: Vec<String> = app
            .snapshot
            .interfaces
            .iter()
            .filter(|i| i.is_up)
            .take(3)
            .map(|i| {
                let name = sanitize_control_chars(&i.name);
                let rx = format_counter(i.counters.rx_bytes, app.unit_family, app.cumulative);
                let tx = format_counter(i.counters.tx_bytes, app.unit_family, app.cumulative);
                format!("{name}: {rx}/{tx}")
            })
            .collect();

        if !active_ifaces.is_empty() {
            summary.push_str("  |  Ifaces: ");
            summary.push_str(&active_ifaces.join(", "));
        }
        lines.push(Line::from(Span::styled(summary, theme.text)));
    }

    if area.height >= 3
        && let CaptureStatus::ProcfsFallback(reason) = &app.snapshot.status
    {
        let banner = format!(
            " Fallback mode (procfs-only, no per-packet attribution): {}",
            sanitize_control_chars(reason)
        );
        lines.push(Line::from(Span::styled(banner, theme.status_alert)));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_body(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    match app.view_mode {
        ViewMode::All => {
            let panes = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .split(area);
            render_processes_pane(frame, panes[0], app, theme, true);
            render_connections_pane(frame, panes[1], app, theme, false);
            render_remotes_pane(frame, panes[2], app, theme, false);
        }
        ViewMode::Processes => render_processes_pane(frame, area, app, theme, true),
        ViewMode::Connections => render_connections_pane(frame, area, app, theme, true),
        ViewMode::Addresses => render_remotes_pane(frame, area, app, theme, true),
    }
}

fn sort_header_cell(
    label: &str,
    col: SortColumn,
    active_col: SortColumn,
    sort_desc: bool,
    theme: &Theme,
) -> Cell<'static> {
    if col == active_col {
        let arrow = if sort_desc { "▼" } else { "▲" };
        Cell::from(format!("{label} {arrow}")).style(theme.table_header_active)
    } else {
        Cell::from(label.to_string()).style(theme.table_header)
    }
}

fn plain_header_cell(label: &str, theme: &Theme) -> Cell<'static> {
    Cell::from(label.to_string()).style(theme.table_header)
}

fn render_processes_pane(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
    is_focused: bool,
) {
    let rows_data = app.visible_processes();
    let title = format!(" Processes ({}) ", rows_data.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_focused {
            theme.accent
        } else {
            theme.border
        })
        .title(Span::styled(title, theme.title));

    let header = Row::new([
        sort_header_cell(
            "PID",
            SortColumn::Pid,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
        sort_header_cell(
            "Process",
            SortColumn::Name,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
        plain_header_cell("Conns", theme),
        sort_header_cell("Rx", SortColumn::Rx, app.sort_column, app.sort_desc, theme),
        sort_header_cell("Tx", SortColumn::Tx, app.sort_column, app.sort_desc, theme),
        sort_header_cell(
            "Total",
            SortColumn::Total,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
    ]);

    let rows: Vec<Row> = rows_data
        .iter()
        .map(|item| {
            let rx = format_counter(item.counters.rx_bytes, app.unit_family, app.cumulative);
            let tx = format_counter(item.counters.tx_bytes, app.unit_family, app.cumulative);
            let total =
                format_counter(item.counters.total_bytes(), app.unit_family, app.cumulative);
            Row::new([
                Cell::from(item.process.pid.to_string()),
                Cell::from(sanitize_control_chars(&item.process.name)),
                Cell::from(item.connection_count.to_string()),
                Cell::from(rx),
                Cell::from(tx),
                Cell::from(total),
            ])
            .style(theme.text)
        })
        .collect();

    let widths = [
        Constraint::Length(7),
        Constraint::Min(12),
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
    ];

    render_table(
        frame, area, block, header, rows, widths, app, theme, is_focused,
    );
}

fn render_connections_pane(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
    is_focused: bool,
) {
    let rows_data = app.visible_connections();
    let title = format!(" Connections ({}) ", rows_data.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_focused {
            theme.accent
        } else {
            theme.border
        })
        .title(Span::styled(title, theme.title));

    let header = Row::new([
        plain_header_cell("Proto", theme),
        plain_header_cell("Local", theme),
        plain_header_cell("Remote", theme),
        sort_header_cell(
            "Process",
            SortColumn::Name,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
        sort_header_cell("Rx", SortColumn::Rx, app.sort_column, app.sort_desc, theme),
        sort_header_cell("Tx", SortColumn::Tx, app.sort_column, app.sort_desc, theme),
        sort_header_cell(
            "Total",
            SortColumn::Total,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
    ]);

    let rows: Vec<Row> = rows_data
        .iter()
        .map(|item| {
            let local = format!("{}:{}", item.key.local.ip, item.key.local.port);
            let remote_host = if app.no_resolve {
                item.key.remote.ip.to_string()
            } else {
                item.hostname
                    .as_deref()
                    .map(sanitize_control_chars)
                    .unwrap_or_else(|| item.key.remote.ip.to_string())
            };
            let remote = format!("{remote_host}:{}", item.key.remote.port);
            let proc_label = item
                .process
                .as_ref()
                .map(|p| format!("{}({})", sanitize_control_chars(&p.name), p.pid))
                .unwrap_or_else(|| "-".to_string());
            let rx = format_counter(item.counters.rx_bytes, app.unit_family, app.cumulative);
            let tx = format_counter(item.counters.tx_bytes, app.unit_family, app.cumulative);
            let total =
                format_counter(item.counters.total_bytes(), app.unit_family, app.cumulative);
            Row::new([
                Cell::from(item.key.protocol.to_string()),
                Cell::from(local),
                Cell::from(remote),
                Cell::from(proc_label),
                Cell::from(rx),
                Cell::from(tx),
                Cell::from(total),
            ])
            .style(theme.text)
        })
        .collect();

    let widths = [
        Constraint::Length(5),
        Constraint::Min(15),
        Constraint::Min(18),
        Constraint::Length(14),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(11),
    ];

    render_table(
        frame, area, block, header, rows, widths, app, theme, is_focused,
    );
}

fn render_remotes_pane(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
    is_focused: bool,
) {
    let rows_data = app.visible_remotes();
    let title = format!(" Remote Addresses ({}) ", rows_data.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_focused {
            theme.accent
        } else {
            theme.border
        })
        .title(Span::styled(title, theme.title));

    let header = Row::new([
        sort_header_cell(
            "Address / Host",
            SortColumn::Name,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
        plain_header_cell("Conns", theme),
        sort_header_cell("Rx", SortColumn::Rx, app.sort_column, app.sort_desc, theme),
        sort_header_cell("Tx", SortColumn::Tx, app.sort_column, app.sort_desc, theme),
        sort_header_cell(
            "Total",
            SortColumn::Total,
            app.sort_column,
            app.sort_desc,
            theme,
        ),
    ]);

    let rows: Vec<Row> = rows_data
        .iter()
        .map(|item| {
            let label = if app.no_resolve {
                item.ip.to_string()
            } else {
                match item.hostname.as_deref() {
                    Some(host) => format!("{} ({})", sanitize_control_chars(host), item.ip),
                    None => item.ip.to_string(),
                }
            };
            let rx = format_counter(item.counters.rx_bytes, app.unit_family, app.cumulative);
            let tx = format_counter(item.counters.tx_bytes, app.unit_family, app.cumulative);
            let total =
                format_counter(item.counters.total_bytes(), app.unit_family, app.cumulative);
            Row::new([
                Cell::from(label),
                Cell::from(item.connection_count.to_string()),
                Cell::from(rx),
                Cell::from(tx),
                Cell::from(total),
            ])
            .style(theme.text)
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
    ];

    render_table(
        frame, area, block, header, rows, widths, app, theme, is_focused,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_table<'a>(
    frame: &mut Frame,
    area: Rect,
    block: Block<'a>,
    header: Row<'a>,
    rows: Vec<Row<'a>>,
    widths: impl IntoIterator<Item = Constraint>,
    app: &AppState,
    theme: &Theme,
    is_focused: bool,
) {
    let row_count = rows.len();
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(if is_focused {
            theme.selected_row
        } else {
            Style::default()
        });

    if is_focused && row_count > 0 {
        let mut state = TableState::default()
            .with_selected(Some(app.selected_row.min(row_count - 1)))
            .with_offset(app.scroll_offset.min(row_count - 1));
        frame.render_stateful_widget(table, area, &mut state);
    } else {
        frame.render_widget(table, area);
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.input_mode == InputMode::Filter {
        let text = format!(" Filter: {}_", sanitize_control_chars(&app.filter_text));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, theme.status_alert))),
            area,
        );
        return;
    }

    if let Some((msg, _)) = &app.status_message {
        let text = format!(" {}", sanitize_control_chars(msg));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, theme.status_alert)))
                .style(theme.status_alert),
            area,
        );
        return;
    }

    let hints = " q:Quit | Tab/0-3:View | j/k:Nav | s:Sort | r:Rev | /:Filter | Space:Pause | t:Total | u:Unit | n:DNS | ?:Help";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hints, theme.status_bar))).style(theme.status_bar),
        area,
    );
}

fn render_help_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
    let width = area.width.min(64).max(20.min(area.width));
    let height = area.height.min(18).max(8.min(area.height));
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let help_lines = vec![
        Line::from(Span::styled("rsnet Keybindings", theme.title)),
        Line::from(""),
        Line::from("  q / F10 / Ctrl+C   Quit"),
        Line::from("  Tab / 0..3         Switch view (All/Proc/Conn/Addr)"),
        Line::from("  j / k / Up / Down  Move selection"),
        Line::from("  s                  Cycle sort column (Total/Rx/Tx/PID/Name)"),
        Line::from("  r                  Reverse sort order"),
        Line::from("  /                  Filter rows (Enter/Esc to finish)"),
        Line::from("  Space / p          Pause or resume display updates"),
        Line::from("  t                  Toggle rates vs cumulative totals"),
        Line::from("  u                  Cycle units (BinBytes/BinBits/SiBytes/SiBits)"),
        Line::from("  n                  Toggle reverse DNS display"),
        Line::from("  ? / F1 / Esc       Toggle or close this help overlay"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.accent)
        .title(Span::styled(" Help ", theme.title));

    let paragraph = Paragraph::new(help_lines)
        .block(block)
        .style(theme.text)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::model::{
        ConnectionKey, ConnectionTraffic, InterfaceTraffic, LocalSocket, NetSnapshot, ProcessInfo,
        ProcessTraffic, Protocol, RemoteSocket, RemoteTraffic, TrafficCounters,
    };

    fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn sample_app() -> AppState {
        let local_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let remote_ip = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        let mut app = AppState::default();
        app.update_snapshot(NetSnapshot {
            interval_secs: 1.0,
            status: CaptureStatus::Live,
            total: TrafficCounters {
                rx_bytes: 4096,
                tx_bytes: 1024,
                rx_packets: 20,
                tx_packets: 10,
            },
            interfaces: vec![InterfaceTraffic {
                name: "eth0".to_string(),
                counters: TrafficCounters {
                    rx_bytes: 4096,
                    tx_bytes: 1024,
                    rx_packets: 20,
                    tx_packets: 10,
                },
                is_up: true,
            }],
            processes: vec![ProcessTraffic {
                process: ProcessInfo {
                    pid: 1234,
                    name: "curl".to_string(),
                    cmdline: "curl https://example.com".to_string(),
                    uid: 1000,
                },
                counters: TrafficCounters {
                    rx_bytes: 4096,
                    tx_bytes: 1024,
                    rx_packets: 20,
                    tx_packets: 10,
                },
                connection_count: 1,
            }],
            connections: vec![ConnectionTraffic {
                key: ConnectionKey {
                    protocol: Protocol::Tcp,
                    local: LocalSocket {
                        ip: local_ip,
                        port: 50000,
                    },
                    remote: RemoteSocket {
                        ip: remote_ip,
                        port: 443,
                    },
                },
                process: Some(ProcessInfo {
                    pid: 1234,
                    name: "curl".to_string(),
                    cmdline: "curl https://example.com".to_string(),
                    uid: 1000,
                }),
                hostname: Some("example.com".to_string()),
                counters: TrafficCounters {
                    rx_bytes: 4096,
                    tx_bytes: 1024,
                    rx_packets: 20,
                    tx_packets: 10,
                },
            }],
            remotes: vec![RemoteTraffic {
                ip: remote_ip,
                hostname: Some("example.com".to_string()),
                counters: TrafficCounters {
                    rx_bytes: 4096,
                    tx_bytes: 1024,
                    rx_packets: 20,
                    tx_packets: 10,
                },
                connection_count: 1,
            }],
        });
        app
    }

    #[test]
    fn renders_all_view_modes_with_test_backend() {
        let mut app = sample_app();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("terminal should init");

        terminal
            .draw(|f| render(f, &app))
            .expect("All view should render");
        let text = buffer_to_string(&terminal);
        assert!(text.contains("rsnet"));
        assert!(text.contains("[LIVE]"));
        assert!(text.contains("Processes (1)"));
        assert!(text.contains("Connections (1)"));
        assert!(text.contains("Remote Addresses (1)"));

        app.view_mode = ViewMode::Processes;
        terminal
            .draw(|f| render(f, &app))
            .expect("Processes view should render");
        let text = buffer_to_string(&terminal);
        assert!(text.contains("Processes (1)"));
        assert!(!text.contains("Remote Addresses (1)"));

        app.view_mode = ViewMode::Connections;
        terminal
            .draw(|f| render(f, &app))
            .expect("Connections view should render");
        let text = buffer_to_string(&terminal);
        assert!(text.contains("Connections (1)"));
        assert!(text.contains("example.com:443"));

        app.view_mode = ViewMode::Addresses;
        terminal
            .draw(|f| render(f, &app))
            .expect("Addresses view should render");
        let text = buffer_to_string(&terminal);
        assert!(text.contains("Remote Addresses (1)"));
        assert!(text.contains("93.184.216.34"));
    }

    #[test]
    fn renders_no_color_help_overlay_and_fallback_banner() {
        let mut app = sample_app();
        app.no_color = true;
        app.paused = true;
        app.cumulative = true;
        app.snapshot.status = CaptureStatus::ProcfsFallback("CAP_NET_RAW required".to_string());
        app.input_mode = InputMode::Help;

        let backend = TestBackend::new(90, 26);
        let mut terminal = Terminal::new(backend).expect("terminal should init");
        terminal
            .draw(|f| render(f, &app))
            .expect("help + fallback should render");

        let text = buffer_to_string(&terminal);
        assert!(text.contains("[PROCFS-ONLY]"));
        assert!(text.contains("[Paused]"));
        assert!(text.contains("[Total]"));
        assert!(text.contains("CAP_NET_RAW required"));
        assert!(text.contains("rsnet Keybindings"));

        // Ensure no RGB/indexed color was set in no_color mode
        let buf = terminal.backend().buffer();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                assert_eq!(buf[(x, y)].fg, ratatui::style::Color::Reset);
                assert_eq!(buf[(x, y)].bg, ratatui::style::Color::Reset);
            }
        }
    }

    #[test]
    fn renders_safely_on_tiny_terminal() {
        let app = sample_app();
        let backend = TestBackend::new(15, 5);
        let mut terminal = Terminal::new(backend).expect("terminal should init");
        terminal
            .draw(|f| render(f, &app))
            .expect("tiny terminal should not panic");
    }
}
