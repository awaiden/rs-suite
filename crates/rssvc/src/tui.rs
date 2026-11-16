use std::io::{self, Stdout};
use std::panic;
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use rs_common::sanitize_control_chars;
use rs_common::tui::Theme;

use crate::model::{ServiceAction, UnitInfo, UnitStateCategory};
use crate::systemd::SystemdClient;

const JOURNAL_LINES: usize = 100;

/// Active input mode in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
}

/// Which pane currently receives scroll keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Units,
    Status,
    Logs,
}

impl FocusPane {
    fn next(self) -> Self {
        match self {
            Self::Units => Self::Status,
            Self::Status => Self::Logs,
            Self::Logs => Self::Units,
        }
    }
}

/// Pending mutating action awaiting user confirmation in the modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAction {
    pub action: ServiceAction,
    pub unit: String,
    pub command: String,
}

/// Banner message displayed above the footer after dry-run or real service actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    pub is_error: bool,
}

/// Interactive systemd service manager TUI state.
pub struct AppState {
    pub units: Vec<UnitInfo>,
    pub filtered_indices: Vec<usize>,
    pub selected: usize,
    pub all_units: bool,
    pub unit_type: Option<String>,
    pub state_filter: Option<String>,
    pub filter_query: String,
    pub input_mode: InputMode,
    pub focus: FocusPane,
    pub status_lines: Vec<String>,
    pub log_lines: Vec<String>,
    pub status_scroll: u16,
    pub log_scroll: u16,
    pub loaded_unit: Option<String>,
    pub pending_action: Option<PendingAction>,
    pub banner: Option<Banner>,
    pub show_help: bool,
    pub dry_run: bool,
    pub user_mode: bool,
    pub theme: Theme,
    pub should_quit: bool,
}

/// Configuration for initializing the [`AppState`] and TUI loop.
#[derive(Debug, Clone, Default)]
pub struct TuiConfig {
    pub all_units: bool,
    pub unit_type: Option<String>,
    pub state_filter: Option<String>,
    pub filter_query: String,
    pub user_mode: bool,
    pub dry_run: bool,
    pub no_color: bool,
}

impl AppState {
    /// Creates a new [`AppState`] with initial units and configuration.
    pub fn new(units: Vec<UnitInfo>, cfg: TuiConfig) -> Self {
        let mut state = Self {
            units,
            filtered_indices: Vec::new(),
            selected: 0,
            all_units: cfg.all_units,
            unit_type: cfg.unit_type,
            state_filter: cfg.state_filter,
            filter_query: cfg.filter_query,
            input_mode: InputMode::Normal,
            focus: FocusPane::Units,
            status_lines: Vec::new(),
            log_lines: Vec::new(),
            status_scroll: 0,
            log_scroll: 0,
            loaded_unit: None,
            pending_action: None,
            banner: cfg.dry_run.then(|| Banner {
                text: "DRY-RUN mode active: service actions will be previewed without executing"
                    .to_string(),
                is_error: false,
            }),
            show_help: false,
            dry_run: cfg.dry_run,
            user_mode: cfg.user_mode,
            theme: Theme::new(cfg.no_color),
            should_quit: false,
        };
        state.recompute_filter();
        state
    }

    /// Recomputes `filtered_indices` from `units` and active filters.
    pub fn recompute_filter(&mut self) {
        let query = (!self.filter_query.trim().is_empty()).then_some(self.filter_query.as_str());
        self.filtered_indices = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.matches_filter(
                    self.unit_type.as_deref(),
                    self.state_filter.as_deref(),
                    query,
                )
            })
            .map(|(i, _)| i)
            .collect();

        if self.filtered_indices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len() - 1;
        }
    }

    /// Returns the currently selected [`UnitInfo`], if any.
    pub fn selected_unit(&self) -> Option<&UnitInfo> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.units.get(idx))
    }

    /// Opens the confirmation modal for `action` on the currently selected unit.
    pub fn request_action(&mut self, action: ServiceAction) {
        if let Some(unit) = self.selected_unit() {
            let unit_name = unit.unit.clone();
            let command = crate::systemd::format_action_command(self.user_mode, action, &unit_name);
            self.pending_action = Some(PendingAction {
                action,
                unit: unit_name,
                command,
            });
        }
    }

    /// Cancels any pending confirmation modal.
    pub fn cancel_action(&mut self) {
        self.pending_action = None;
    }

    /// Executes (or dry-run previews) the pending action using `client`.
    pub fn confirm_action(&mut self, client: &SystemdClient) {
        let Some(pending) = self.pending_action.take() else {
            return;
        };

        match client.perform_action(pending.action, &pending.unit) {
            Ok(outcome) => {
                self.banner = Some(Banner {
                    text: sanitize_control_chars(&outcome.output),
                    is_error: false,
                });
                if outcome.executed {
                    self.refresh_all(client);
                }
            }
            Err(err) => {
                self.banner = Some(Banner {
                    text: sanitize_control_chars(&err),
                    is_error: true,
                });
            }
        }
    }

    /// Refreshes the unit list and the selected unit's status & journal logs.
    pub fn refresh_all(&mut self, client: &SystemdClient) {
        match client.list_units(
            self.all_units,
            self.unit_type.as_deref(),
            self.state_filter.as_deref(),
        ) {
            Ok(units) => {
                let prev_name = self.selected_unit().map(|u| u.unit.clone());
                self.units = units;
                self.recompute_filter();
                if let Some(name) = prev_name
                    && let Some(pos) = self
                        .filtered_indices
                        .iter()
                        .position(|&idx| self.units[idx].unit == name)
                {
                    self.selected = pos;
                }
                self.loaded_unit = None;
                self.sync_selected_details(client);
            }
            Err(err) => {
                self.banner = Some(Banner {
                    text: sanitize_control_chars(&err),
                    is_error: true,
                });
            }
        }
    }

    /// Loads status and journal logs if the selected unit changed.
    pub fn sync_selected_details(&mut self, client: &SystemdClient) {
        let Some(unit_name) = self.selected_unit().map(|u| u.unit.clone()) else {
            self.loaded_unit = None;
            self.status_lines.clear();
            self.log_lines.clear();
            return;
        };

        if self.loaded_unit.as_deref() == Some(unit_name.as_str()) {
            return;
        }

        self.loaded_unit = Some(unit_name.clone());
        self.status_scroll = 0;
        self.log_scroll = 0;

        self.status_lines = match client.unit_status(&unit_name) {
            Ok(text) => text.lines().map(sanitize_control_chars).collect(),
            Err(err) => vec![format!(
                "Error loading status: {}",
                sanitize_control_chars(&err)
            )],
        };

        self.log_lines = match client.unit_logs(&unit_name, JOURNAL_LINES) {
            Ok(lines) if lines.is_empty() => vec!["(no journal entries)".to_string()],
            Ok(lines) => lines
                .into_iter()
                .map(|l| sanitize_control_chars(&l))
                .collect(),
            Err(err) => vec![format!(
                "Error loading journal logs: {}",
                sanitize_control_chars(&err)
            )],
        };
    }

    /// Handles a single keyboard event.
    pub fn handle_key(&mut self, key: KeyEvent, client: &SystemdClient) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        if self.pending_action.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.confirm_action(client);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.cancel_action();
                }
                _ => {}
            }
            return;
        }

        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1) | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return;
        }

        if self.input_mode == InputMode::Filter {
            match key.code {
                KeyCode::Enter => {
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Esc => {
                    self.filter_query.clear();
                    self.recompute_filter();
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Backspace => {
                    self.filter_query.pop();
                    self.recompute_filter();
                }
                KeyCode::Char(c) => {
                    self.filter_query.push(c);
                    self.recompute_filter();
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => self.should_quit = true,
            KeyCode::Char('?') | KeyCode::F(1) => self.show_help = true,
            KeyCode::Char('/') => self.input_mode = InputMode::Filter,
            KeyCode::Esc => {
                if !self.filter_query.is_empty() {
                    self.filter_query.clear();
                    self.recompute_filter();
                } else {
                    self.banner = None;
                }
            }
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::Char('a') => {
                self.all_units = !self.all_units;
                self.refresh_all(client);
            }
            KeyCode::Char('R') | KeyCode::F(5) => self.refresh_all(client),
            KeyCode::Char('s') => self.request_action(ServiceAction::Start),
            KeyCode::Char('x') | KeyCode::Char('S') => self.request_action(ServiceAction::Stop),
            KeyCode::Char('r') => self.request_action(ServiceAction::Restart),
            KeyCode::Char('l') => self.request_action(ServiceAction::Reload),
            KeyCode::Char('e') => self.request_action(ServiceAction::Enable),
            KeyCode::Char('d') => self.request_action(ServiceAction::Disable),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_down(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_up(1),
            KeyCode::PageDown => self.scroll_down(15),
            KeyCode::PageUp => self.scroll_up(15),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_top(),
            KeyCode::Char('G') | KeyCode::End => self.scroll_bottom(),
            _ => {}
        }
    }

    fn scroll_down(&mut self, amount: usize) {
        match self.focus {
            FocusPane::Units => {
                if !self.filtered_indices.is_empty() {
                    self.selected = (self.selected + amount).min(self.filtered_indices.len() - 1);
                }
            }
            FocusPane::Status => {
                let max = self.status_lines.len().saturating_sub(1) as u16;
                self.status_scroll = self.status_scroll.saturating_add(amount as u16).min(max);
            }
            FocusPane::Logs => {
                let max = self.log_lines.len().saturating_sub(1) as u16;
                self.log_scroll = self.log_scroll.saturating_add(amount as u16).min(max);
            }
        }
    }

    fn scroll_up(&mut self, amount: usize) {
        match self.focus {
            FocusPane::Units => {
                self.selected = self.selected.saturating_sub(amount);
            }
            FocusPane::Status => {
                self.status_scroll = self.status_scroll.saturating_sub(amount as u16);
            }
            FocusPane::Logs => {
                self.log_scroll = self.log_scroll.saturating_sub(amount as u16);
            }
        }
    }

    fn scroll_top(&mut self) {
        match self.focus {
            FocusPane::Units => self.selected = 0,
            FocusPane::Status => self.status_scroll = 0,
            FocusPane::Logs => self.log_scroll = 0,
        }
    }

    fn scroll_bottom(&mut self) {
        match self.focus {
            FocusPane::Units => {
                if !self.filtered_indices.is_empty() {
                    self.selected = self.filtered_indices.len() - 1;
                }
            }
            FocusPane::Status => {
                self.status_scroll = self.status_lines.len().saturating_sub(1) as u16;
            }
            FocusPane::Logs => {
                self.log_scroll = self.log_lines.len().saturating_sub(1) as u16;
            }
        }
    }
}

/// Runs the interactive `rssvc` TUI until the user exits.
pub fn run_tui(client: &SystemdClient, cfg: TuiConfig) -> Result<(), String> {
    let units = client.list_units(
        cfg.all_units,
        cfg.unit_type.as_deref(),
        cfg.state_filter.as_deref(),
    )?;
    let mut app = AppState::new(units, cfg);
    app.sync_selected_details(client);

    let mut terminal =
        setup_terminal().map_err(|e| format!("failed to initialize terminal: {e}"))?;
    let result = event_loop(&mut terminal, &mut app, client);
    let restore_res = restore_terminal(&mut terminal);

    result.map_err(|e| format!("TUI error: {e}"))?;
    restore_res.map_err(|e| format!("failed to restore terminal: {e}"))?;
    Ok(())
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut AppState,
    client: &SystemdClient,
) -> io::Result<()> {
    while !app.should_quit {
        app.sync_selected_details(client);
        terminal.draw(|frame| draw_ui(frame, app))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key, client);
        }
    }
    Ok(())
}

fn draw_ui(frame: &mut ratatui::Frame, app: &AppState) {
    let show_filter = app.input_mode == InputMode::Filter || !app.filter_query.is_empty();
    let show_banner = app.banner.is_some();

    let mut constraints = vec![Constraint::Length(1)];
    if show_filter {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(8));
    if show_banner {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.area());

    let mut idx = 0;
    draw_header(frame, chunks[idx], app);
    idx += 1;

    if show_filter {
        draw_filter_bar(frame, chunks[idx], app);
        idx += 1;
    }

    draw_main_panes(frame, chunks[idx], app);
    idx += 1;

    if let Some(banner) = &app.banner {
        draw_banner(frame, chunks[idx], banner, &app.theme);
        idx += 1;
    }

    draw_footer(frame, chunks[idx], app);

    if let Some(pending) = &app.pending_action {
        draw_confirm_modal(frame, pending, app.dry_run, &app.theme);
    } else if app.show_help {
        draw_help_modal(frame, &app.theme);
    }
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let scope = if app.user_mode { "user" } else { "system" };
    let all_tag = if app.all_units { " [all]" } else { "" };
    let dry_tag = if app.dry_run { " [DRY-RUN]" } else { "" };
    let text = format!(
        " rssvc ({scope}){all_tag}{dry_tag}  |  {}/{} units",
        app.filtered_indices.len(),
        app.units.len()
    );
    frame.render_widget(Paragraph::new(text).style(app.theme.bar_style()), area);
}

fn draw_filter_bar(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let prefix = if app.input_mode == InputMode::Filter {
        "Filter (Enter confirm, Esc clear): /"
    } else {
        "Filter (Esc clear): /"
    };
    let clean = sanitize_control_chars(&app.filter_query);
    let line = Line::from(vec![
        Span::styled(prefix, app.theme.accent),
        Span::styled(clean, app.theme.text),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_main_panes(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);

    draw_unit_list(frame, cols[0], app);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(cols[1]);

    let unit_label = app
        .selected_unit()
        .map(|u| sanitize_control_chars(&u.unit))
        .unwrap_or_else(|| "none".to_string());

    draw_text_pane(
        frame,
        right_rows[0],
        &format!(" Status ({unit_label}) "),
        &app.status_lines,
        app.status_scroll,
        app.focus == FocusPane::Status,
        &app.theme,
    );

    draw_text_pane(
        frame,
        right_rows[1],
        &format!(" Journal Logs ({unit_label}) "),
        &app.log_lines,
        app.log_scroll,
        app.focus == FocusPane::Logs,
        &app.theme,
    );
}

fn draw_unit_list(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let border_style = if app.focus == FocusPane::Units {
        app.theme.accent
    } else {
        app.theme.border
    };

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .map(|&idx| {
            let u = &app.units[idx];
            let state_style = match u.state_category() {
                UnitStateCategory::Active => app.theme.proc_running,
                UnitStateCategory::Failed => app.theme.proc_zombie,
                UnitStateCategory::Transitioning => app.theme.usage_mid,
                UnitStateCategory::Inactive => app.theme.muted,
            };

            let unit_name = sanitize_control_chars(&u.unit);
            let active = sanitize_control_chars(&u.active);
            let sub = sanitize_control_chars(&u.sub);
            let desc = sanitize_control_chars(&u.description);

            let line = Line::from(vec![
                Span::styled(format!("{:<28} ", unit_name), app.theme.text),
                Span::styled(format!("{:<9} {:<9} ", active, sub), state_style),
                Span::styled(desc, app.theme.muted),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    if !app.filtered_indices.is_empty() {
        state.select(Some(app.selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(" Units ", app.theme.title)),
        )
        .highlight_style(app.theme.selected_row);

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_text_pane(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    lines: &[String],
    scroll: u16,
    focused: bool,
    theme: &Theme,
) {
    let border_style = if focused { theme.accent } else { theme.border };
    let text: Vec<Line> = lines
        .iter()
        .map(|l| Line::from(Span::styled(l.as_str(), theme.text)))
        .collect();

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(title, theme.title)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(para, area);
}

fn draw_banner(frame: &mut ratatui::Frame, area: Rect, banner: &Banner, theme: &Theme) {
    let style = if banner.is_error
        || banner.text.starts_with("[dry-run]")
        || banner.text.starts_with("DRY-RUN")
    {
        theme.status_alert
    } else {
        theme.bar_style()
    };
    let text = format!(" {}", sanitize_control_chars(&banner.text));
    frame.render_widget(Paragraph::new(text).style(style), area);
}

fn draw_footer(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let hints = " q:Quit  /:Filter  a:All  s:Start  x:Stop  r:Restart  l:Reload  e:Enable  d:Disable  Tab:Pane  R:Refresh  ?:Help";
    frame.render_widget(Paragraph::new(hints).style(app.theme.status_bar), area);
}

fn draw_confirm_modal(
    frame: &mut ratatui::Frame,
    pending: &PendingAction,
    dry_run: bool,
    theme: &Theme,
) {
    let area = centered_rect(64, 9, frame.area());
    frame.render_widget(Clear, area);

    let title = if dry_run {
        " Confirm Dry-Run Action "
    } else {
        " Confirm Service Action "
    };

    let clean_unit = sanitize_control_chars(&pending.unit);
    let clean_cmd = sanitize_control_chars(&pending.command);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Action: ", theme.muted),
            Span::styled(
                format!("{} {}", pending.action.label(), clean_unit),
                theme.accent.add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Command: ", theme.muted),
            Span::styled(clean_cmd, theme.text),
        ]),
    ];

    if dry_run {
        lines.push(Line::from(Span::styled(
            "Dry-run is enabled: command will NOT be executed.",
            theme.usage_mid,
        )));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[y / Enter] Confirm    [n / Esc] Cancel",
        theme.title,
    )));

    let modal = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.accent)
                .title(Span::styled(title, theme.title)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(modal, area);
}

fn draw_help_modal(frame: &mut ratatui::Frame, theme: &Theme) {
    let area = centered_rect(66, 17, frame.area());
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::from(Span::styled("Navigation", theme.title)),
        Line::from("  j / k, Up / Down   Move selection or scroll focused pane"),
        Line::from("  PgUp / PgDn, g / G Page up/down, jump to top/bottom"),
        Line::from("  Tab                Cycle focus: Units -> Status -> Logs"),
        Line::from("  /                  Interactive filter (Enter confirm, Esc clear)"),
        Line::from("  a                  Toggle --all (show inactive units)"),
        Line::from("  R / F5             Refresh units, status, and logs"),
        Line::from(""),
        Line::from(Span::styled(
            "Service Actions (with confirmation)",
            theme.title,
        )),
        Line::from("  s                  Start unit"),
        Line::from("  x / S              Stop unit"),
        Line::from("  r                  Restart unit"),
        Line::from("  l                  Reload unit"),
        Line::from("  e / d              Enable / Disable unit"),
        Line::from("  q / Esc / ?        Close help or quit"),
    ];

    let modal = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.accent)
            .title(Span::styled(" rssvc Help ", theme.title)),
    );
    frame.render_widget(modal, area);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height.min(area.height)),
            Constraint::Min(0),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let mut out = io::stdout();
        let _ = execute!(out, LeaveAlternateScreen, Show);
        prev_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Maps a [`UnitStateCategory`] to its [`Theme`] style (exposed for unit tests).
pub fn state_style(theme: &Theme, cat: UnitStateCategory) -> Style {
    match cat {
        UnitStateCategory::Active => theme.proc_running,
        UnitStateCategory::Failed => theme.proc_zombie,
        UnitStateCategory::Transitioning => theme.usage_mid,
        UnitStateCategory::Inactive => theme.muted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_units() -> Vec<UnitInfo> {
        vec![
            UnitInfo {
                unit: "sshd.service".into(),
                load: "loaded".into(),
                active: "active".into(),
                sub: "running".into(),
                description: "OpenSSH Daemon".into(),
            },
            UnitInfo {
                unit: "nginx.service".into(),
                load: "loaded".into(),
                active: "failed".into(),
                sub: "failed".into(),
                description: "Web server".into(),
            },
            UnitInfo {
                unit: "fstrim.timer".into(),
                load: "loaded".into(),
                active: "inactive".into(),
                sub: "dead".into(),
                description: "Weekly trim".into(),
            },
        ]
    }

    #[test]
    fn interactive_filter_updates_selection() {
        let mut app = AppState::new(
            sample_units(),
            TuiConfig {
                dry_run: true,
                ..Default::default()
            },
        );
        let client = SystemdClient::new(false, true);

        assert_eq!(app.filtered_indices.len(), 3);
        app.handle_key(KeyEvent::from(KeyCode::Char('/')), &client);
        assert_eq!(app.input_mode, InputMode::Filter);

        for ch in ['n', 'g', 'i', 'n', 'x'] {
            app.handle_key(KeyEvent::from(KeyCode::Char(ch)), &client);
        }
        assert_eq!(app.filtered_indices.len(), 1);
        assert_eq!(app.selected_unit().unwrap().unit, "nginx.service");

        app.handle_key(KeyEvent::from(KeyCode::Esc), &client);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.filtered_indices.len(), 3);
    }

    #[test]
    fn confirmation_modal_and_dry_run_banner() {
        let mut app = AppState::new(
            sample_units(),
            TuiConfig {
                user_mode: true,
                dry_run: true,
                ..Default::default()
            },
        );
        let client = SystemdClient::new(true, true);

        app.handle_key(KeyEvent::from(KeyCode::Char('r')), &client);
        let pending = app.pending_action.as_ref().expect("modal should open");
        assert_eq!(pending.action, ServiceAction::Restart);
        assert_eq!(pending.unit, "sshd.service");
        assert_eq!(pending.command, "systemctl --user restart sshd.service");

        app.handle_key(KeyEvent::from(KeyCode::Char('y')), &client);
        assert!(app.pending_action.is_none());
        let banner = app.banner.as_ref().expect("dry-run banner should appear");
        assert_eq!(
            banner.text,
            "[dry-run] systemctl --user restart sshd.service"
        );
        assert!(!banner.is_error);
    }
}
