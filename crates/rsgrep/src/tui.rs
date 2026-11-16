use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use rs_common::sanitize_control_chars;
use rs_common::tui::Theme;

use crate::cli::Args;
use crate::search::{FileMatches, SearchOpts, search};

/// Result of running the interactive TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiOutcome {
    /// User pressed Enter on a match.
    Selected(FlatMatch),
    /// User cancelled with Esc or Ctrl-C.
    Cancelled,
}

/// Flattened match entry for list display and navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatMatch {
    pub path: PathBuf,
    pub line_number: usize,
    pub text: String,
    pub spans: Vec<(usize, usize)>,
}

/// Interactive TUI application state.
pub struct AppState {
    pub query: String,
    pub base_opts: SearchOpts,
    pub matches: Vec<FlatMatch>,
    pub file_count: usize,
    pub selected: usize,
    pub error: Option<String>,
    pub theme: Theme,
    preview_path: Option<PathBuf>,
    preview_lines: Vec<String>,
}

impl AppState {
    /// Creates a new `AppState` and runs an initial search if `query` is non-empty.
    pub fn new(args: &Args) -> Self {
        let query = args.pattern.clone().unwrap_or_default();
        let mut base_opts = SearchOpts::from_args(args, query.clone());
        // Context lines are shown in the preview pane rather than cluttering the match list.
        base_opts.before_context = 0;
        base_opts.after_context = 0;

        let mut state = Self {
            query,
            base_opts,
            matches: Vec::new(),
            file_count: 0,
            selected: 0,
            error: None,
            theme: Theme::new(args.no_color),
            preview_path: None,
            preview_lines: Vec::new(),
        };
        state.refresh_search();
        state
    }

    /// Re-runs the parallel search for the current `query`.
    pub fn refresh_search(&mut self) {
        if self.query.is_empty() {
            self.matches.clear();
            self.file_count = 0;
            self.selected = 0;
            self.error = None;
            self.preview_path = None;
            self.preview_lines.clear();
            return;
        }

        self.base_opts.pattern = self.query.clone();
        match search(&self.base_opts) {
            Ok(files) => {
                self.file_count = files.len();
                self.matches = flatten_matches(&files);
                self.error = None;
                if self.selected >= self.matches.len() {
                    self.selected = self.matches.len().saturating_sub(1);
                }
                self.sync_preview();
            }
            Err(err) => {
                self.error = Some(sanitize_control_chars(&err));
            }
        }
    }

    /// Returns the currently selected match, if any.
    pub fn current_match(&self) -> Option<&FlatMatch> {
        self.matches.get(self.selected)
    }

    /// Moves selection up by `delta`.
    pub fn move_up(&mut self, delta: usize) {
        self.selected = self.selected.saturating_sub(delta);
        self.sync_preview();
    }

    /// Moves selection down by `delta`.
    pub fn move_down(&mut self, delta: usize) {
        if !self.matches.is_empty() {
            self.selected = (self.selected + delta).min(self.matches.len() - 1);
            self.sync_preview();
        }
    }

    fn sync_preview(&mut self) {
        let Some(m) = self.matches.get(self.selected) else {
            self.preview_path = None;
            self.preview_lines.clear();
            return;
        };
        if self.preview_path.as_ref() == Some(&m.path) {
            return;
        }
        self.preview_path = Some(m.path.clone());
        self.preview_lines = load_sanitized_lines(&m.path);
    }
}

fn flatten_matches(files: &[FileMatches]) -> Vec<FlatMatch> {
    let mut out = Vec::new();
    for file in files {
        for hunk in &file.hunks {
            for line in hunk {
                if line.is_match {
                    out.push(FlatMatch {
                        path: file.path.clone(),
                        line_number: line.line_number,
                        text: line.text.clone(),
                        spans: line.spans.clone(),
                    });
                }
            }
        }
    }
    out
}

fn load_sanitized_lines(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    if bytes.contains(&0) {
        return vec!["[binary file]".to_string()];
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .map(sanitize_control_chars)
        .collect()
}

/// Computes the 0-based starting line index to center `target_line` (1-based) in `viewport_height`.
pub fn preview_scroll(target_line: usize, total_lines: usize, viewport_height: usize) -> usize {
    if viewport_height == 0 || total_lines <= viewport_height {
        return 0;
    }
    let target_idx = target_line.saturating_sub(1);
    let half = viewport_height / 2;
    let max_start = total_lines.saturating_sub(viewport_height);
    target_idx.saturating_sub(half).min(max_start)
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Runs the interactive split-pane TUI until the user selects a match or cancels.
pub fn run_tui(args: &Args) -> io::Result<TuiOutcome> {
    let mut guard = TerminalGuard::enter()?;
    let mut app = AppState::new(args);

    loop {
        guard.terminal.draw(|f| render(&app, f))?;

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            match handle_key(&mut app, key) {
                KeyAction::Continue => {}
                KeyAction::Select => {
                    return Ok(app
                        .current_match()
                        .cloned()
                        .map_or(TuiOutcome::Cancelled, TuiOutcome::Selected));
                }
                KeyAction::Cancel => return Ok(TuiOutcome::Cancelled),
            }
        }
    }
}

enum KeyAction {
    Continue,
    Select,
    Cancel,
}

fn handle_key(app: &mut AppState, key: KeyEvent) -> KeyAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => return KeyAction::Cancel,
            KeyCode::Char('u') => {
                app.query.clear();
                app.refresh_search();
                return KeyAction::Continue;
            }
            KeyCode::Char('p') => {
                app.move_up(1);
                return KeyAction::Continue;
            }
            KeyCode::Char('n') => {
                app.move_down(1);
                return KeyAction::Continue;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => KeyAction::Cancel,
        KeyCode::Enter => KeyAction::Select,
        KeyCode::Up => {
            app.move_up(1);
            KeyAction::Continue
        }
        KeyCode::Down => {
            app.move_down(1);
            KeyAction::Continue
        }
        KeyCode::PageUp => {
            app.move_up(10);
            KeyAction::Continue
        }
        KeyCode::PageDown => {
            app.move_down(10);
            KeyAction::Continue
        }
        KeyCode::Home => {
            app.selected = 0;
            app.sync_preview();
            KeyAction::Continue
        }
        KeyCode::End => {
            if !app.matches.is_empty() {
                app.selected = app.matches.len() - 1;
                app.sync_preview();
            }
            KeyAction::Continue
        }
        KeyCode::Backspace => {
            app.query.pop();
            app.refresh_search();
            KeyAction::Continue
        }
        KeyCode::Char(c) => {
            app.query.push(c);
            app.refresh_search();
            KeyAction::Continue
        }
        _ => KeyAction::Continue,
    }
}

fn render(app: &AppState, frame: &mut ratatui::Frame<'_>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_query_bar(app, frame, chunks[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    render_match_list(app, frame, panes[0]);
    render_preview(app, frame, panes[1]);
    render_status_bar(app, frame, chunks[2]);
}

fn render_query_bar(app: &AppState, frame: &mut ratatui::Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let safe_query = sanitize_control_chars(&app.query);
    let title = if let Some(err) = &app.error {
        format!(" rsgrep — {err} ")
    } else {
        format!(
            " rsgrep — {} matches in {} files ",
            app.matches.len(),
            app.file_count
        )
    };
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", theme.accent),
        Span::styled(safe_query, theme.text),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(title, theme.title)),
    );
    frame.render_widget(input, area);
}

fn render_match_list(app: &AppState, frame: &mut ratatui::Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let items: Vec<ListItem<'_>> = app
        .matches
        .iter()
        .map(|m| {
            let safe_path = sanitize_control_chars(&m.path.display().to_string());
            let safe_text = sanitize_control_chars(&m.text);
            let line = Line::from(vec![
                Span::styled(safe_path, theme.accent),
                Span::styled(format!(":{}: ", m.line_number), theme.muted),
                Span::styled(safe_text, theme.text),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title(Span::styled(" Matches ", theme.title)),
        )
        .highlight_style(theme.selected_row);

    let mut state = ListState::default();
    if !app.matches.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_preview(app: &AppState, frame: &mut ratatui::Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let inner_height = area.height.saturating_sub(2) as usize;

    let (title, lines): (String, Vec<Line<'_>>) = if let Some(m) = app.current_match() {
        let safe_path = sanitize_control_chars(&m.path.display().to_string());
        let title = format!(" Preview: {safe_path}:{} ", m.line_number);
        let start = preview_scroll(m.line_number, app.preview_lines.len(), inner_height);
        let end = (start + inner_height).min(app.preview_lines.len());
        let width = app.preview_lines.len().to_string().len().max(3);

        let rendered = app.preview_lines[start..end]
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                let line_num = start + offset + 1;
                let is_target = line_num == m.line_number;
                let num_style = if is_target {
                    theme.accent.add_modifier(Modifier::BOLD)
                } else {
                    theme.muted
                };
                let text_style = if is_target {
                    theme.selected_row.add_modifier(Modifier::BOLD)
                } else {
                    theme.text
                };
                Line::from(vec![
                    Span::styled(format!("{line_num:>width$} │ "), num_style),
                    Span::styled(text.clone(), text_style),
                ])
            })
            .collect();
        (title, rendered)
    } else {
        (
            " Preview ".to_string(),
            vec![Line::from(Span::styled("No match selected", theme.muted))],
        )
    };

    let preview = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(title, theme.title)),
    );
    frame.render_widget(preview, area);
}

fn render_status_bar(app: &AppState, frame: &mut ratatui::Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let bar = Paragraph::new(Line::from(vec![Span::styled(
        " Up/Down: navigate | PgUp/PgDn: page | Ctrl-U: clear | Enter: select | Esc: quit",
        theme.bar_style(),
    )]))
    .style(theme.bar_style());
    frame.render_widget(bar, area);
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn preview_scroll_centers_target_line_and_clamps_bounds() {
        assert_eq!(preview_scroll(1, 100, 20), 0);
        // 1-based line 50 is 0-based index 49; 49 - 20/2 = 39 (shows 1-based lines 40..=59).
        assert_eq!(preview_scroll(50, 100, 20), 39);
        assert_eq!(preview_scroll(98, 100, 20), 80);
        assert_eq!(preview_scroll(5, 10, 20), 0);
    }

    #[test]
    fn app_state_searches_navigates_and_sanitizes_control_chars() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("dirty.txt");
        fs::write(&file_path, "line one\nmatch\x1b[31m_here\nline three\n").unwrap();

        let args = Args::try_parse_from([
            "rsgrep",
            "--tui",
            "--no-color",
            "match",
            dir.path().to_str().unwrap(),
        ])
        .unwrap();

        let mut app = AppState::new(&args);
        assert!(app.theme.no_color);
        assert_eq!(app.matches.len(), 1);
        assert_eq!(app.matches[0].line_number, 2);
        // Preview lines must have control characters sanitized.
        assert!(!app.preview_lines[1].contains('\x1b'));

        app.query = "line".to_string();
        app.refresh_search();
        assert_eq!(app.matches.len(), 2);
        app.move_down(1);
        assert_eq!(app.selected, 1);
        app.move_up(1);
        assert_eq!(app.selected, 0);
    }
}
