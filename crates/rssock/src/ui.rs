use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use rs_common::sanitize_control_chars;
use rs_common::tui::Theme;

use crate::app::{AppState, InputMode};
use crate::model::{
    ListenMode, SocketKind, SocketRecord, SocketState, SortColumn, SummaryStats, format_endpoint,
};

/// Renders the complete `rssock` TUI frame: header, socket table, footer, and active overlays.
pub fn render(frame: &mut Frame, app: &AppState) {
    let theme = Theme::new(app.no_color);
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let header_height = if area.height < 6 { 1 } else { 2 };
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
    render_table(frame, chunks[1], app, &theme);
    if footer_height > 0 {
        render_footer(frame, chunks[2], app, &theme);
    }

    match app.input_mode {
        InputMode::Help => render_help_overlay(frame, area, &theme),
        InputMode::Detail => {
            if let Some(rec) = app.selected_record() {
                render_detail_popup(frame, area, rec, app.numeric, &theme);
            }
        }
        InputMode::Normal | InputMode::Filter => {}
    }
}

fn proto_badge(label: &str, active: bool, theme: &Theme) -> Span<'static> {
    if active {
        Span::styled(format!("[{label}]"), theme.proc_running)
    } else {
        Span::styled(format!("[{label}]"), theme.muted)
    }
}

fn listen_mode_label(mode: ListenMode) -> &'static str {
    match mode {
        ListenMode::All => "ALL",
        ListenMode::Listening => "LISTEN",
        ListenMode::NonListening => "NON-LISTEN",
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut top_spans = vec![
        Span::styled(" rssock ", theme.title),
        Span::raw(" "),
        proto_badge("TCP", app.show_tcp, theme),
        Span::raw(" "),
        proto_badge("UDP", app.show_udp, theme),
        Span::raw(" "),
        proto_badge("UNIX", app.show_unix, theme),
        Span::raw(" "),
        Span::styled(
            format!("[{}]", listen_mode_label(app.listen_mode)),
            theme.accent,
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "{}/{} sockets",
                app.filtered_records.len(),
                app.all_records.len()
            ),
            theme.text,
        ),
    ];

    if app.numeric {
        top_spans.push(Span::raw(" "));
        top_spans.push(Span::styled("[NUMERIC]", theme.accent));
    }
    if app.paused {
        top_spans.push(Span::raw(" "));
        top_spans.push(Span::styled("[PAUSED]", theme.status_alert));
    }

    let mut lines = vec![Line::from(top_spans)];

    if area.height >= 2 {
        let second_line = if app.input_mode == InputMode::Filter {
            Line::from(vec![
                Span::styled(" Filter: ", theme.status_alert),
                Span::raw(" "),
                Span::styled(format!("{}_", app.filter_query), theme.text),
            ])
        } else if !app.filter_query.is_empty() {
            Line::from(vec![
                Span::styled(" Filter: ", theme.accent),
                Span::styled(app.filter_query.clone(), theme.text),
                Span::styled("  (Esc to clear)", theme.muted),
            ])
        } else {
            let stats = SummaryStats::from_records(&app.all_records);
            Line::from(vec![Span::styled(
                format!(
                    " TCP: {} (estab {}, listen {})  UDP: {}  UNIX: {}",
                    stats.tcp + stats.tcp6,
                    stats.tcp_established,
                    stats.tcp_listen,
                    stats.udp + stats.udp6,
                    stats.unix
                ),
                theme.muted,
            )])
        };
        lines.push(second_line);
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn state_style(state: SocketState, theme: &Theme) -> Style {
    match state {
        SocketState::Established | SocketState::Connected => theme.proc_running,
        SocketState::Listen => theme.accent,
        SocketState::TimeWait
        | SocketState::SynSent
        | SocketState::SynRecv
        | SocketState::NewSynRecv
        | SocketState::FinWait1
        | SocketState::FinWait2
        | SocketState::CloseWait
        | SocketState::LastAck
        | SocketState::Closing
        | SocketState::Connecting
        | SocketState::Disconnecting => theme.usage_mid,
        SocketState::Unconn | SocketState::Close | SocketState::Unknown(_) => theme.muted,
    }
}

fn local_display(rec: &SocketRecord, numeric: bool) -> String {
    match &rec.kind {
        SocketKind::Inet { local, .. } => format_endpoint(local, rec.protocol, numeric),
        SocketKind::Unix { path, .. } => path
            .as_deref()
            .map(sanitize_control_chars)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "*".to_string()),
    }
}

fn peer_display(rec: &SocketRecord, numeric: bool) -> String {
    match &rec.kind {
        SocketKind::Inet { remote, .. } => format_endpoint(remote, rec.protocol, numeric),
        SocketKind::Unix { .. } => "*".to_string(),
    }
}

fn header_cell(
    label: &str,
    col: SortColumn,
    active: SortColumn,
    reverse: bool,
    theme: &Theme,
) -> Cell<'static> {
    if col == active {
        let arrow = if reverse { "▼" } else { "▲" };
        Cell::from(format!("{label} {arrow}")).style(theme.table_header_active)
    } else {
        Cell::from(label.to_string()).style(theme.table_header)
    }
}

fn render_table(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    let header = Row::new([
        header_cell(
            "Netid",
            SortColumn::Proto,
            app.sort_column,
            app.reverse,
            theme,
        ),
        header_cell(
            "State",
            SortColumn::State,
            app.sort_column,
            app.reverse,
            theme,
        ),
        header_cell(
            "Recv-Q",
            SortColumn::RecvQ,
            app.sort_column,
            app.reverse,
            theme,
        ),
        header_cell(
            "Send-Q",
            SortColumn::SendQ,
            app.sort_column,
            app.reverse,
            theme,
        ),
        header_cell(
            "Local",
            SortColumn::Local,
            app.sort_column,
            app.reverse,
            theme,
        ),
        header_cell(
            "Peer",
            SortColumn::Remote,
            app.sort_column,
            app.reverse,
            theme,
        ),
        header_cell("PID", SortColumn::Pid, app.sort_column, app.reverse, theme),
        header_cell(
            "Process",
            SortColumn::Process,
            app.sort_column,
            app.reverse,
            theme,
        ),
    ])
    .height(1);

    let rows: Vec<Row<'_>> = app
        .filtered_records
        .iter()
        .map(|rec| {
            let pid = rec
                .process
                .as_ref()
                .map_or_else(|| "-".to_string(), |p| p.pid.to_string());
            let proc_name = rec
                .process
                .as_ref()
                .map_or_else(|| "-".to_string(), |p| sanitize_control_chars(&p.name));

            Row::new([
                Cell::from(rec.protocol.as_str()).style(theme.accent),
                Cell::from(rec.state.to_string()).style(state_style(rec.state, theme)),
                Cell::from(rec.recv_q.to_string()).style(theme.text),
                Cell::from(rec.send_q.to_string()).style(theme.text),
                Cell::from(local_display(rec, app.numeric)).style(theme.text),
                Cell::from(peer_display(rec, app.numeric)).style(theme.text),
                Cell::from(pid).style(theme.muted),
                Cell::from(proc_name).style(theme.text),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Percentage(26),
        Constraint::Percentage(24),
        Constraint::Length(8),
        Constraint::Min(12),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title(" Sockets "),
        )
        .row_highlight_style(theme.selected_row);

    let mut state = TableState::default();
    if !app.filtered_records.is_empty() {
        state.select(Some(app.selected));
    }

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    let text = match app.input_mode {
        InputMode::Filter => " Filter mode: type query | Enter/Esc: done | Backspace: delete",
        InputMode::Help => " Help: Esc/?/Enter: close | q: quit",
        InputMode::Detail => " Detail: Esc/Enter: close | ?: help | q: quit",
        InputMode::Normal => {
            " q:quit  j/k:nav  t/u/x:proto  l/a:listen  s:sort  r:rev  n:num  /:filter  Enter:detail  Space:pause  ?:help"
        }
    };

    let bar =
        Paragraph::new(Line::from(Span::styled(text, theme.status_bar))).style(theme.status_bar);
    frame.render_widget(bar, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn render_help_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
    let popup_area = centered_rect(70, 75, area);
    frame.render_widget(Clear, popup_area);

    let lines = vec![
        Line::from(Span::styled("Navigation", theme.title)),
        Line::from("  j / k, Up / Down   Move selection"),
        Line::from("  PageUp / PageDown  Scroll by page"),
        Line::from("  g / G              Jump to top / bottom"),
        Line::from(""),
        Line::from(Span::styled("Filtering & Display", theme.title)),
        Line::from("  t / u / x          Toggle TCP / UDP / UNIX"),
        Line::from("  l                  Toggle Listening / All"),
        Line::from("  a                  Toggle All / Non-Listening"),
        Line::from("  s / r              Cycle sort column / Reverse sort"),
        Line::from("  n                  Toggle numeric ports"),
        Line::from("  /                  Substring filter (Esc clears in Normal)"),
        Line::from(""),
        Line::from(Span::styled("Actions", theme.title)),
        Line::from("  Enter              Toggle socket detail popup"),
        Line::from("  Space / p          Pause / resume refresh"),
        Line::from("  ? / F1             Toggle this help"),
        Line::from("  q / F10 / Ctrl+C   Quit"),
    ];

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.accent)
                .title(Span::styled(" rssock Help ", theme.title)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(popup, popup_area);
}

fn render_detail_popup(
    frame: &mut Frame,
    area: Rect,
    rec: &SocketRecord,
    numeric: bool,
    theme: &Theme,
) {
    let popup_area = centered_rect(74, 65, area);
    frame.render_widget(Clear, popup_area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Protocol: ", theme.table_header),
            Span::styled(rec.protocol.as_str(), theme.accent),
            Span::raw("    "),
            Span::styled("State: ", theme.table_header),
            Span::styled(rec.state.to_string(), state_style(rec.state, theme)),
        ]),
        Line::from(vec![
            Span::styled("Queues:   ", theme.table_header),
            Span::raw(format!("Recv-Q={}  Send-Q={}", rec.recv_q, rec.send_q)),
        ]),
        Line::from(vec![
            Span::styled("UID:      ", theme.table_header),
            Span::raw(rec.uid.to_string()),
            Span::raw("    "),
            Span::styled("Inode: ", theme.table_header),
            Span::raw(rec.inode.to_string()),
        ]),
    ];

    match &rec.kind {
        SocketKind::Inet { local, remote } => {
            lines.push(Line::from(vec![
                Span::styled("Local:    ", theme.table_header),
                Span::raw(format_endpoint(local, rec.protocol, numeric)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Peer:     ", theme.table_header),
                Span::raw(format_endpoint(remote, rec.protocol, numeric)),
            ]));
        }
        SocketKind::Unix {
            unix_type,
            flags,
            ref_count,
            path,
        } => {
            lines.push(Line::from(vec![
                Span::styled("Type:     ", theme.table_header),
                Span::raw(unix_type.to_string()),
                Span::raw("    "),
                Span::styled("Flags: ", theme.table_header),
                Span::raw(format!("0x{flags:08x}")),
                Span::raw("    "),
                Span::styled("RefCount: ", theme.table_header),
                Span::raw(ref_count.to_string()),
            ]));
            let safe_path = path
                .as_deref()
                .map(sanitize_control_chars)
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "-".to_string());
            lines.push(Line::from(vec![
                Span::styled("Path:     ", theme.table_header),
                Span::raw(safe_path),
            ]));
        }
    }

    lines.push(Line::from(""));
    if let Some(proc_info) = &rec.process {
        lines.push(Line::from(vec![
            Span::styled("Process:  ", theme.table_header),
            Span::raw(format!(
                "{} (pid={}, uid={})",
                sanitize_control_chars(&proc_info.name),
                proc_info.pid,
                proc_info.uid
            )),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Cmdline:  ", theme.table_header),
            Span::raw(sanitize_control_chars(&proc_info.cmdline)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Process:  ", theme.table_header),
            Span::styled(
                "(unavailable - insufficient permissions or kernel socket)",
                theme.muted,
            ),
        ]));
    }

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.accent)
                .title(Span::styled(" Socket Details ", theme.title)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(popup, popup_area);
}
