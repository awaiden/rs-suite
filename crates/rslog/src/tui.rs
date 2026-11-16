use std::io::{self, Stdout};
use std::panic;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

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
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use rs_common::sanitize_control_chars;
use rs_common::tui::Theme;

use crate::model::{LogEntry, LogFilter, LogLevel};
use crate::source::{FileTailer, JournalEvent, JournalStream};

/// Input mode for the interactive TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    RegexFilter,
}

/// Interactive `lnav`-style log viewer state.
pub struct AppState {
    pub source_label: String,
    pub entries: Vec<LogEntry>,
    pub filtered_indices: Vec<usize>,
    pub selected: usize,
    pub filter: LogFilter,
    pub regex_input: String,
    pub regex_error: Option<String>,
    pub input_mode: InputMode,
    pub follow: bool,
    pub show_detail: bool,
    pub status_message: Option<String>,
    pub theme: Theme,
    pub should_quit: bool,
}

impl AppState {
    /// Creates a new [`AppState`] with initial entries, filter, and color theme.
    pub fn new(
        source_label: String,
        entries: Vec<LogEntry>,
        initial_filter: LogFilter,
        initial_regex: String,
        follow: bool,
        no_color: bool,
    ) -> Self {
        let mut state = Self {
            source_label,
            entries,
            filtered_indices: Vec::new(),
            selected: 0,
            filter: initial_filter,
            regex_input: initial_regex,
            regex_error: None,
            input_mode: InputMode::Normal,
            follow,
            show_detail: false,
            status_message: None,
            theme: Theme::new(no_color),
            should_quit: false,
        };
        state.recompute_filter(follow);
        state
    }

    /// Appends newly arrived log entries and updates the filtered view.
    pub fn push_entries(&mut self, new_entries: Vec<LogEntry>) {
        if new_entries.is_empty() {
            return;
        }
        let start_idx = self.entries.len();
        self.entries.extend(new_entries);
        for (offset, entry) in self.entries[start_idx..].iter().enumerate() {
            if self.filter.matches(entry) {
                self.filtered_indices.push(start_idx + offset);
            }
        }
        if self.follow && !self.filtered_indices.is_empty() {
            self.selected = self.filtered_indices.len() - 1;
        }
    }

    /// Recomputes `filtered_indices` from all `entries`.
    pub fn recompute_filter(&mut self, jump_to_end: bool) {
        self.filtered_indices = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| self.filter.matches(e).then_some(i))
            .collect();

        if self.filtered_indices.is_empty() {
            self.selected = 0;
        } else if jump_to_end || self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len() - 1;
        }
    }

    /// Returns the currently selected [`LogEntry`], if any.
    pub fn selected_entry(&self) -> Option<&LogEntry> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.entries.get(idx))
    }

    /// Applies `self.regex_input` to `self.filter`, recording any regex compilation error.
    pub fn apply_regex_input(&mut self) {
        match LogFilter::new(self.filter.min_level, Some(&self.regex_input)) {
            Ok(new_filter) => {
                self.filter = new_filter;
                self.regex_error = None;
                self.recompute_filter(self.follow);
            }
            Err(err) => {
                self.regex_error = Some(err.to_string());
            }
        }
    }

    /// Handles a single keyboard event and updates state accordingly.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.input_mode {
            InputMode::RegexFilter => self.handle_regex_key(key),
            InputMode::Normal => self.handle_normal_key(key),
        }
    }

    fn handle_regex_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.regex_input.pop();
                self.apply_regex_input();
            }
            KeyCode::Char(c) => {
                self.regex_input.push(c);
                self.apply_regex_input();
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.follow = false;
                self.move_selection(-1);
            }
            KeyCode::PageDown => {
                self.move_selection(20);
            }
            KeyCode::PageUp => {
                self.follow = false;
                self.move_selection(-20);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.follow = false;
                self.selected = 0;
            }
            KeyCode::Char('G') | KeyCode::End => {
                if !self.filtered_indices.is_empty() {
                    self.selected = self.filtered_indices.len() - 1;
                }
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::RegexFilter;
            }
            KeyCode::Char('1') => self.set_min_level(Some(LogLevel::Trace)),
            KeyCode::Char('2') => self.set_min_level(Some(LogLevel::Debug)),
            KeyCode::Char('3') => self.set_min_level(Some(LogLevel::Info)),
            KeyCode::Char('4') => self.set_min_level(Some(LogLevel::Warn)),
            KeyCode::Char('5') => self.set_min_level(Some(LogLevel::Error)),
            KeyCode::Char('L') => {
                let next = LogLevel::cycle_filter(self.filter.min_level);
                self.set_min_level(next);
            }
            KeyCode::Char('f') => {
                self.follow = !self.follow;
                if self.follow && !self.filtered_indices.is_empty() {
                    self.selected = self.filtered_indices.len() - 1;
                }
            }
            KeyCode::Enter => {
                self.show_detail = !self.show_detail;
            }
            KeyCode::Char('c') => {
                self.regex_input.clear();
                self.regex_error = None;
                self.filter = LogFilter::default();
                self.recompute_filter(self.follow);
            }
            _ => {}
        }
    }

    fn set_min_level(&mut self, level: Option<LogLevel>) {
        self.filter.min_level = level;
        self.recompute_filter(self.follow);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered_indices.is_empty() {
            self.selected = 0;
            return;
        }
        let max = self.filtered_indices.len() - 1;
        let next = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected.saturating_add(delta as usize).min(max)
        };
        self.selected = next;
    }

    /// Returns the `Theme`-aware style for a log level badge.
    pub fn level_badge_style(&self, level: LogLevel) -> Style {
        match level {
            LogLevel::Error => self.theme.usage_high.add_modifier(Modifier::BOLD),
            LogLevel::Warn => self.theme.usage_mid,
            LogLevel::Info => self.theme.proc_running,
            LogLevel::Debug => self.theme.accent,
            LogLevel::Trace => self.theme.muted,
        }
    }
}

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
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));
}

/// Runs the interactive TUI event loop until the user quits.
pub fn run_tui(
    state: &mut AppState,
    mut tailers: Vec<FileTailer>,
    journal_stream: Option<JournalStream>,
) -> io::Result<()> {
    install_panic_hook();
    let mut guard = TerminalGuard::new()?;
    let mut list_state = ListState::default();
    let mut last_poll = Instant::now();

    loop {
        if !state.filtered_indices.is_empty() {
            list_state.select(Some(state.selected));
        } else {
            list_state.select(None);
        }

        guard
            .terminal
            .draw(|frame| draw_ui(frame, state, &mut list_state))?;

        if state.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            state.handle_key(key);
        }

        if last_poll.elapsed() >= Duration::from_millis(150) {
            last_poll = Instant::now();
            poll_sources(state, &mut tailers, journal_stream.as_ref());
        }
    }

    Ok(())
}

fn poll_sources(
    state: &mut AppState,
    tailers: &mut [FileTailer],
    journal_stream: Option<&JournalStream>,
) {
    for tailer in tailers {
        match tailer.poll() {
            Ok(entries) => state.push_entries(entries),
            Err(err) => state.status_message = Some(err.to_string()),
        }
    }

    if let Some(stream) = journal_stream {
        let mut batch = Vec::new();
        loop {
            match stream.receiver().try_recv() {
                Ok(JournalEvent::Entry(entry)) => batch.push(entry),
                Ok(JournalEvent::Error(msg)) => {
                    state.status_message = Some(msg);
                    break;
                }
                Ok(JournalEvent::Eof)
                | Err(TryRecvError::Empty)
                | Err(TryRecvError::Disconnected) => {
                    break;
                }
            }
        }
        state.push_entries(batch);
    }
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, state: &AppState, list_state: &mut ListState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], state);

    if state.show_detail {
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(chunks[1]);
        draw_log_list(frame, body[0], state, list_state);
        draw_detail_pane(frame, body[1], state);
    } else {
        draw_log_list(frame, chunks[1], state, list_state);
    }

    draw_footer(frame, chunks[2], state);
}

fn draw_header(frame: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let level_label = state
        .filter
        .min_level
        .map_or("ALL".to_owned(), |l| format!("{l}+"));
    let follow_label = if state.follow { " [FOLLOW]" } else { "" };
    let clean_source = sanitize_control_chars(&state.source_label);
    let clean_regex = sanitize_control_chars(&state.regex_input);
    let regex_part = if clean_regex.is_empty() {
        String::new()
    } else {
        format!(" | /{clean_regex}/")
    };

    let text = format!(
        " rslog | {clean_source} | {}/{} lines | level:{level_label}{regex_part}{follow_label}",
        state.filtered_indices.len(),
        state.entries.len(),
    );

    let bar = Paragraph::new(Line::from(Span::styled(text, state.theme.bar_style())))
        .style(state.theme.bar_style());
    frame.render_widget(bar, area);
}

fn draw_log_list(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &AppState,
    list_state: &mut ListState,
) {
    let items: Vec<ListItem<'_>> = state
        .filtered_indices
        .iter()
        .filter_map(|&idx| state.entries.get(idx))
        .map(|entry| {
            let level = entry.effective_level();
            let mut spans = Vec::new();
            if let Some(ts) = &entry.timestamp {
                spans.push(Span::styled(
                    format!("{} ", sanitize_control_chars(ts)),
                    state.theme.muted,
                ));
            }
            spans.push(Span::styled(
                format!("[{}] ", level.badge()),
                state.level_badge_style(level),
            ));
            if let Some(unit) = &entry.unit {
                let clean_unit = sanitize_control_chars(unit);
                let unit_text = match entry.pid {
                    Some(pid) => format!("{clean_unit}[{pid}]: "),
                    None => format!("{clean_unit}: "),
                };
                spans.push(Span::styled(unit_text, state.theme.accent));
            }
            spans.push(Span::styled(
                sanitize_control_chars(&entry.message),
                state.theme.text,
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(state.theme.border)
                .title(Span::styled(" Logs ", state.theme.title)),
        )
        .highlight_style(state.theme.selected_row);

    frame.render_stateful_widget(list, area, list_state);
}

fn draw_detail_pane(frame: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let mut lines = Vec::new();
    if let Some(entry) = state.selected_entry() {
        lines.push(Line::from(vec![
            Span::styled("Source: ", state.theme.accent),
            Span::styled(sanitize_control_chars(&entry.source), state.theme.text),
            Span::styled(
                entry
                    .line_number
                    .map_or(String::new(), |n| format!("  (line {n})")),
                state.theme.muted,
            ),
        ]));
        if let Some(ts) = &entry.timestamp {
            lines.push(Line::from(vec![
                Span::styled("Time:   ", state.theme.accent),
                Span::styled(sanitize_control_chars(ts), state.theme.text),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("Level:  ", state.theme.accent),
            Span::styled(
                entry.effective_level().as_str(),
                state.level_badge_style(entry.effective_level()),
            ),
        ]));
        if let Some(unit) = &entry.unit {
            let pid_part = entry.pid.map_or(String::new(), |p| format!(" (pid {p})"));
            lines.push(Line::from(vec![
                Span::styled("Unit:   ", state.theme.accent),
                Span::styled(
                    format!("{}{}", sanitize_control_chars(unit), pid_part),
                    state.theme.text,
                ),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("Msg:    ", state.theme.accent),
            Span::styled(sanitize_control_chars(&entry.message), state.theme.text),
        ]));
        for (k, v) in &entry.fields {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", sanitize_control_chars(k)),
                    state.theme.muted,
                ),
                Span::styled(sanitize_control_chars(v), state.theme.text),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No log entry selected.",
            state.theme.muted,
        )));
    }

    let detail = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(state.theme.border)
                .title(Span::styled(" Entry Detail ", state.theme.title)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(detail, area);
}

fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let widget = match state.input_mode {
        InputMode::RegexFilter => {
            let clean_input = sanitize_control_chars(&state.regex_input);
            let err_suffix = state
                .regex_error
                .as_deref()
                .map(|e| format!("  [invalid regex: {}]", sanitize_control_chars(e)))
                .unwrap_or_default();
            let style = if state.regex_error.is_some() {
                state.theme.status_alert
            } else {
                state.theme.status_bar
            };
            Paragraph::new(Line::from(Span::styled(
                format!(" /{clean_input}_{err_suffix}"),
                style,
            )))
            .style(style)
        }
        InputMode::Normal => {
            let text = if let Some(msg) = &state.status_message {
                format!(" {}", sanitize_control_chars(msg))
            } else {
                " /:regex  1-5/L:level  f:follow  Enter:detail  j/k/g/G:nav  c:clear  q:quit"
                    .to_owned()
            };
            let style = if state.status_message.is_some() {
                state.theme.status_alert
            } else {
                state.theme.status_bar
            };
            Paragraph::new(Line::from(Span::styled(text, style))).style(style)
        }
    };
    frame.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_log_line;

    fn sample_entries() -> Vec<LogEntry> {
        vec![
            parse_log_line("2026-11-17T00:00:01Z TRACE trace msg", "app.log", Some(1)),
            parse_log_line("2026-11-17T00:00:02Z DEBUG debug msg", "app.log", Some(2)),
            parse_log_line("2026-11-17T00:00:03Z INFO info msg", "app.log", Some(3)),
            parse_log_line("2026-11-17T00:00:04Z WARN warn timeout", "app.log", Some(4)),
            parse_log_line("2026-11-17T00:00:05Z ERROR fatal error", "app.log", Some(5)),
        ]
    }

    #[test]
    fn tui_state_level_and_regex_filtering() {
        let mut state = AppState::new(
            "app.log".to_owned(),
            sample_entries(),
            LogFilter::default(),
            String::new(),
            false,
            false,
        );
        assert_eq!(state.filtered_indices.len(), 5);

        // Press '4' -> Warn+
        state.handle_key(KeyEvent::from(KeyCode::Char('4')));
        assert_eq!(state.filter.min_level, Some(LogLevel::Warn));
        assert_eq!(state.filtered_indices.len(), 2);

        // Press '/' and type "fatal"
        state.handle_key(KeyEvent::from(KeyCode::Char('/')));
        assert_eq!(state.input_mode, InputMode::RegexFilter);
        for ch in "fatal".chars() {
            state.handle_key(KeyEvent::from(KeyCode::Char(ch)));
        }
        assert_eq!(state.filtered_indices.len(), 1);
        assert_eq!(state.selected_entry().unwrap().level, Some(LogLevel::Error));

        // Invalid regex records error without panicking
        state.handle_key(KeyEvent::from(KeyCode::Char('[')));
        assert!(state.regex_error.is_some());

        // Exit regex mode and clear filters
        state.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.input_mode, InputMode::Normal);
        state.handle_key(KeyEvent::from(KeyCode::Char('c')));
        assert_eq!(state.filtered_indices.len(), 5);
        assert!(state.regex_error.is_none());
    }

    #[test]
    fn tui_state_follow_and_detail_toggle() {
        let mut state = AppState::new(
            "app.log".to_owned(),
            sample_entries(),
            LogFilter::default(),
            String::new(),
            false,
            true,
        );
        assert!(!state.follow);
        assert!(!state.show_detail);

        state.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(state.show_detail);

        state.handle_key(KeyEvent::from(KeyCode::Char('f')));
        assert!(state.follow);
        assert_eq!(state.selected, 4);

        state.push_entries(vec![parse_log_line(
            "2026-11-17T00:00:06Z INFO new line",
            "app.log",
            Some(6),
        )]);
        assert_eq!(state.selected, 5);
    }
}
