use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use rs_common::fmt::format_bytes;
use rs_common::sanitize_control_chars;
use rs_common::tui::{Theme, spinner_char};

use crate::cli::{Args, ColorWhen};
use crate::fuzzy::{CaseMode, FuzzyEngine, MatchResult};
use crate::output::should_use_color;
use crate::walk::FoundEntry;

const MAX_PREVIEW_BYTES: usize = 64 * 1024;
const MAX_PREVIEW_LINES: usize = 200;
const MAX_DIR_ENTRIES: usize = 200;
const POLL_INTERVAL: Duration = Duration::from_millis(16);
const BATCH_RECV_LIMIT: usize = 4096;

/// Direction of the optional preview pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewDirection {
    #[default]
    Right,
    Down,
    Left,
    Up,
}

/// Parsed `--preview-window` configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewWindow {
    pub direction: PreviewDirection,
    pub percent: u16,
    pub visible: bool,
}

impl Default for PreviewWindow {
    fn default() -> Self {
        Self {
            direction: PreviewDirection::Right,
            percent: 50,
            visible: true,
        }
    }
}

impl PreviewWindow {
    /// Parse a `--preview-window` specification such as `"right:50%"` or `"down:40%:hidden"`.
    pub fn parse(spec: Option<&str>) -> Self {
        let mut cfg = Self::default();
        let Some(raw) = spec else {
            return cfg;
        };

        for part in raw.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "right" => cfg.direction = PreviewDirection::Right,
                "down" | "bottom" => cfg.direction = PreviewDirection::Down,
                "left" => cfg.direction = PreviewDirection::Left,
                "up" | "top" => cfg.direction = PreviewDirection::Up,
                "hidden" => cfg.visible = false,
                "nohidden" => cfg.visible = true,
                other => {
                    if let Some(num_str) = other.strip_suffix('%')
                        && let Ok(pct) = num_str.parse::<u16>()
                    {
                        cfg.percent = pct.clamp(10, 90);
                    }
                }
            }
        }
        cfg
    }
}

/// Configuration for the interactive TUI.
#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub query: String,
    pub multi: bool,
    pub case_mode: CaseMode,
    pub no_color: bool,
    pub preview_cmd: Option<String>,
    pub preview_window: PreviewWindow,
}

impl TuiConfig {
    /// Build `TuiConfig` from parsed CLI `Args`.
    pub fn from_args(args: &Args) -> Self {
        let case_mode = if args.case_sensitive {
            CaseMode::Sensitive
        } else if args.ignore_case {
            CaseMode::Ignore
        } else {
            CaseMode::Smart
        };

        let no_color = match args.effective_color() {
            ColorWhen::Never => true,
            ColorWhen::Always => false,
            ColorWhen::Auto => !should_use_color(ColorWhen::Auto),
        };

        Self {
            query: args.query.clone().unwrap_or_default(),
            multi: args.multi,
            case_mode,
            no_color,
            preview_cmd: args.preview.clone(),
            preview_window: PreviewWindow::parse(args.preview_window.as_deref()),
        }
    }
}

/// Outcome returned when the TUI exits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiOutcome {
    /// User confirmed one or more entries with `Enter`.
    Selected(Vec<FoundEntry>),
    /// User aborted with `Esc` or `Ctrl-C` (exit code 130).
    Aborted,
}

/// Pure application state for the interactive fuzzy-finder TUI.
pub struct TuiState {
    pub candidates: Vec<FoundEntry>,
    candidate_displays: Vec<String>,
    pub matches: Vec<MatchResult>,
    pub query: String,
    /// Cursor position in `query` measured in Unicode scalar values (chars).
    pub cursor_pos: usize,
    pub case_mode: CaseMode,
    pub multi: bool,
    /// Selected candidate indices (into `candidates`), preserved across query edits.
    pub selected: HashSet<usize>,
    /// Highlighted row index into `matches`.
    pub list_cursor: usize,
    /// First visible row index in `matches`.
    pub scroll_offset: usize,
    /// Height of the candidate list viewport in rows.
    pub viewport_height: usize,
    /// Whether the candidate source is still streaming entries.
    pub loading: bool,
    engine: FuzzyEngine,
}

impl TuiState {
    /// Create a new `TuiState` with an optional initial query.
    pub fn new(query: String, case_mode: CaseMode, multi: bool) -> Self {
        let cursor_pos = query.chars().count();
        Self {
            candidates: Vec::new(),
            candidate_displays: Vec::new(),
            matches: Vec::new(),
            query,
            cursor_pos,
            case_mode,
            multi,
            selected: HashSet::new(),
            list_cursor: 0,
            scroll_offset: 0,
            viewport_height: 20,
            loading: true,
            engine: FuzzyEngine::new(),
        }
    }

    /// Total number of received candidates.
    pub fn total_count(&self) -> usize {
        self.candidates.len()
    }

    /// Number of candidates matching the current query.
    pub fn matched_count(&self) -> usize {
        self.matches.len()
    }

    /// Returns `true` if `candidate_idx` is currently multi-selected.
    pub fn is_selected(&self, candidate_idx: usize) -> bool {
        self.selected.contains(&candidate_idx)
    }

    /// Append a batch of streamed entries and re-rank matches.
    pub fn push_entries(&mut self, entries: impl IntoIterator<Item = FoundEntry>) {
        let mut added = false;
        for entry in entries {
            self.candidate_displays.push(entry.display.clone());
            self.candidates.push(entry);
            added = true;
        }
        if added {
            self.refilter();
        }
    }

    /// Re-run fuzzy matching and ranking over all candidates, clamping cursor and scroll.
    pub fn refilter(&mut self) {
        self.matches =
            self.engine
                .rank_items(&self.query, &self.candidate_displays, self.case_mode);
        self.clamp_cursor_and_scroll();
    }

    /// Update the viewport height (from layout) and clamp scroll offset.
    pub fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height.max(1);
        self.clamp_scroll();
    }

    /// Clamp `list_cursor` to `matches` bounds and adjust `scroll_offset` so the
    /// cursor remains visible inside `viewport_height`.
    pub fn clamp_scroll(&mut self) {
        if self.matches.is_empty() {
            self.list_cursor = 0;
            self.scroll_offset = 0;
            return;
        }

        let max_idx = self.matches.len() - 1;
        if self.list_cursor > max_idx {
            self.list_cursor = max_idx;
        }

        let vh = self.viewport_height.max(1);
        if self.list_cursor < self.scroll_offset {
            self.scroll_offset = self.list_cursor;
        } else if self.list_cursor >= self.scroll_offset + vh {
            self.scroll_offset = self.list_cursor + 1 - vh;
        }

        let max_scroll = self.matches.len().saturating_sub(vh);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    fn clamp_cursor_and_scroll(&mut self) {
        if self.matches.is_empty() {
            self.list_cursor = 0;
            self.scroll_offset = 0;
        } else {
            if self.list_cursor >= self.matches.len() {
                self.list_cursor = self.matches.len() - 1;
            }
            self.clamp_scroll();
        }
    }

    // --- Query editing ---

    fn char_to_byte_idx(&self, char_idx: usize) -> usize {
        self.query
            .char_indices()
            .nth(char_idx)
            .map_or(self.query.len(), |(b, _)| b)
    }

    /// Insert a character at `cursor_pos`, advance cursor, and refilter.
    pub fn insert_char(&mut self, ch: char) {
        let byte_idx = self.char_to_byte_idx(self.cursor_pos);
        self.query.insert(byte_idx, ch);
        self.cursor_pos += 1;
        self.list_cursor = 0;
        self.scroll_offset = 0;
        self.refilter();
    }

    /// Delete the character before `cursor_pos`.
    pub fn backspace(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let end_byte = self.char_to_byte_idx(self.cursor_pos);
        let start_byte = self.char_to_byte_idx(self.cursor_pos - 1);
        self.query.replace_range(start_byte..end_byte, "");
        self.cursor_pos -= 1;
        self.list_cursor = 0;
        self.scroll_offset = 0;
        self.refilter();
    }

    /// Delete the character at `cursor_pos`.
    pub fn delete_char(&mut self) {
        let char_len = self.query.chars().count();
        if self.cursor_pos >= char_len {
            return;
        }
        let start_byte = self.char_to_byte_idx(self.cursor_pos);
        let end_byte = self.char_to_byte_idx(self.cursor_pos + 1);
        self.query.replace_range(start_byte..end_byte, "");
        self.list_cursor = 0;
        self.scroll_offset = 0;
        self.refilter();
    }

    /// Clear the entire query (`Ctrl-U`).
    pub fn clear_query(&mut self) {
        if self.query.is_empty() {
            return;
        }
        self.query.clear();
        self.cursor_pos = 0;
        self.list_cursor = 0;
        self.scroll_offset = 0;
        self.refilter();
    }

    /// Delete the word immediately before `cursor_pos` (`Ctrl-W`).
    pub fn delete_word(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let chars: Vec<char> = self.query.chars().collect();
        let mut new_pos = self.cursor_pos;
        while new_pos > 0 && chars[new_pos - 1].is_whitespace() {
            new_pos -= 1;
        }
        while new_pos > 0 && !chars[new_pos - 1].is_whitespace() {
            new_pos -= 1;
        }
        let start_byte = self.char_to_byte_idx(new_pos);
        let end_byte = self.char_to_byte_idx(self.cursor_pos);
        self.query.replace_range(start_byte..end_byte, "");
        self.cursor_pos = new_pos;
        self.list_cursor = 0;
        self.scroll_offset = 0;
        self.refilter();
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        let len = self.query.chars().count();
        if self.cursor_pos < len {
            self.cursor_pos += 1;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.query.chars().count();
    }

    // --- List navigation ---

    pub fn move_up(&mut self) {
        if self.list_cursor > 0 {
            self.list_cursor -= 1;
            self.clamp_scroll();
        }
    }

    pub fn move_down(&mut self) {
        if !self.matches.is_empty() && self.list_cursor + 1 < self.matches.len() {
            self.list_cursor += 1;
            self.clamp_scroll();
        }
    }

    pub fn page_up(&mut self) {
        let step = self.viewport_height.max(1);
        self.list_cursor = self.list_cursor.saturating_sub(step);
        self.clamp_scroll();
    }

    pub fn page_down(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let step = self.viewport_height.max(1);
        self.list_cursor = (self.list_cursor + step).min(self.matches.len() - 1);
        self.clamp_scroll();
    }

    pub fn move_home(&mut self) {
        self.list_cursor = 0;
        self.clamp_scroll();
    }

    pub fn move_end(&mut self) {
        if !self.matches.is_empty() {
            self.list_cursor = self.matches.len() - 1;
            self.clamp_scroll();
        }
    }

    // --- Multi-select ---

    /// Toggle multi-select on the highlighted item and optionally step the cursor.
    pub fn toggle_select(&mut self, move_down_after: bool) {
        if !self.multi {
            return;
        }
        if let Some(m) = self.matches.get(self.list_cursor) {
            let idx = m.index;
            if !self.selected.remove(&idx) {
                self.selected.insert(idx);
            }
            if move_down_after {
                self.move_down();
            } else {
                self.move_up();
            }
        }
    }

    /// Select all currently matched candidates (`Ctrl-A`).
    pub fn select_all(&mut self) {
        if !self.multi {
            return;
        }
        for m in &self.matches {
            self.selected.insert(m.index);
        }
    }

    /// Deselect all candidates (`Ctrl-D`).
    pub fn deselect_all(&mut self) {
        if !self.multi {
            return;
        }
        self.selected.clear();
    }

    /// Return the currently highlighted `FoundEntry`, if any.
    pub fn highlighted_entry(&self) -> Option<&FoundEntry> {
        self.matches
            .get(self.list_cursor)
            .and_then(|m| self.candidates.get(m.index))
    }

    /// Return the confirmed selection(s) when pressing `Enter`.
    pub fn confirm_selection(&self) -> Vec<FoundEntry> {
        if self.multi && !self.selected.is_empty() {
            let mut indices: Vec<usize> = self.selected.iter().copied().collect();
            indices.sort_unstable();
            return indices
                .into_iter()
                .filter_map(|i| self.candidates.get(i).cloned())
                .collect();
        }

        self.highlighted_entry().cloned().into_iter().collect()
    }
}

/// Spawn a background thread reading lines from piped stdin into a `Receiver<FoundEntry>`.
pub fn spawn_stdin_reader() -> Receiver<FoundEntry> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        let reader = BufReader::new(stdin.lock());
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let trimmed = line.trim_end_matches('\r');
            if trimmed.is_empty() {
                continue;
            }
            let entry = FoundEntry {
                path: PathBuf::from(trimmed),
                display: trimmed.to_string(),
                is_dir: false,
                is_symlink: false,
                is_exec: false,
            };
            if tx.send(entry).is_err() {
                break;
            }
        }
    });
    rx
}

/// Writer targeting `/dev/tty` when stdout is redirected, or `stdout` when it is a TTY.
enum TtyWriter {
    Stdout(io::Stdout),
    File(File),
    Stderr(io::Stderr),
}

impl TtyWriter {
    fn open() -> Self {
        if io::stdout().is_terminal() {
            return Self::Stdout(io::stdout());
        }
        if let Ok(file) = OpenOptions::new().read(true).write(true).open("/dev/tty") {
            return Self::File(file);
        }
        Self::Stderr(io::stderr())
    }
}

impl Write for TtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(s) => s.write(buf),
            Self::File(f) => f.write(buf),
            Self::Stderr(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(s) => s.flush(),
            Self::File(f) => f.flush(),
            Self::Stderr(s) => s.flush(),
        }
    }
}

fn restore_terminal_state() {
    let _ = disable_raw_mode();
    let mut writer = TtyWriter::open();
    let _ = execute!(writer, LeaveAlternateScreen, Show);
    let _ = writer.flush();
}

/// RAII guard that enters raw mode + alternate screen and restores them on drop.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<TtyWriter>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut writer = TtyWriter::open();
        if let Err(err) = execute!(writer, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(err);
        }

        let backend = CrosstermBackend::new(writer);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(err) => {
                restore_terminal_state();
                Err(err)
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        let _ = self.terminal.show_cursor();
    }
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal_state();
        prev(info);
    }));
}

/// Run the interactive fuzzy-finder TUI, reading candidates from `rx`.
pub fn run_tui(rx: Receiver<FoundEntry>, config: TuiConfig) -> io::Result<TuiOutcome> {
    install_panic_hook();
    let mut guard = TerminalGuard::new()?;
    let theme = Theme::new(config.no_color);
    let mut state = TuiState::new(config.query.clone(), config.case_mode, config.multi);
    let mut preview_visible = config.preview_window.visible;
    let mut preview_cache: Option<(PathBuf, Vec<String>)> = None;
    let start_time = Instant::now();

    loop {
        drain_candidates(&rx, &mut state);

        if preview_visible {
            let current_path = state.highlighted_entry().map(|e| e.path.clone());
            match current_path {
                Some(path) => {
                    let needs_refresh = preview_cache
                        .as_ref()
                        .is_none_or(|(cached, _)| cached != &path);
                    if needs_refresh {
                        let lines = compute_preview(&path, config.preview_cmd.as_deref());
                        preview_cache = Some((path, lines));
                    }
                }
                None => preview_cache = None,
            }
        }

        let preview_lines: &[String] = preview_cache
            .as_ref()
            .map(|(_, lines)| lines.as_slice())
            .unwrap_or(&[]);

        guard.terminal.draw(|frame| {
            render_ui(
                frame,
                &mut state,
                &theme,
                config.preview_window,
                preview_visible,
                preview_lines,
                start_time.elapsed().as_millis(),
            );
        })?;

        let poll_timeout = if state.loading {
            POLL_INTERVAL
        } else {
            Duration::from_millis(100)
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if let Some(outcome) = handle_key_event(key, &mut state, &mut preview_visible) {
                        return Ok(outcome);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
}

fn drain_candidates(rx: &Receiver<FoundEntry>, state: &mut TuiState) {
    if !state.loading {
        return;
    }
    let mut batch = Vec::new();
    for _ in 0..BATCH_RECV_LIMIT {
        match rx.try_recv() {
            Ok(entry) => batch.push(entry),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.loading = false;
                break;
            }
        }
    }
    if !batch.is_empty() {
        state.push_entries(batch);
    }
}

/// Handle a single key event; returns `Some(TuiOutcome)` if the TUI should exit.
pub fn handle_key_event(
    key: KeyEvent,
    state: &mut TuiState,
    preview_visible: &mut bool,
) -> Option<TuiOutcome> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    if alt && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')) {
        *preview_visible = !*preview_visible;
        return None;
    }

    match key.code {
        KeyCode::Esc => return Some(TuiOutcome::Aborted),
        KeyCode::Char('c') if ctrl => return Some(TuiOutcome::Aborted),
        KeyCode::Enter => return Some(TuiOutcome::Selected(state.confirm_selection())),

        // Navigation
        KeyCode::Up => state.move_up(),
        KeyCode::Down => state.move_down(),
        KeyCode::Char('k') | KeyCode::Char('p') if ctrl => state.move_up(),
        KeyCode::Char('j') | KeyCode::Char('n') if ctrl => state.move_down(),
        KeyCode::PageUp => state.page_up(),
        KeyCode::PageDown => state.page_down(),
        KeyCode::Home => state.move_home(),
        KeyCode::End => state.move_end(),

        // Multi-select
        KeyCode::Tab => state.toggle_select(true),
        KeyCode::BackTab => state.toggle_select(false),
        KeyCode::Char('a') if ctrl => {
            if state.multi {
                state.select_all();
            } else {
                state.move_cursor_start();
            }
        }
        KeyCode::Char('d') if ctrl => state.deselect_all(),

        // Query editing
        KeyCode::Char('u') if ctrl => state.clear_query(),
        KeyCode::Char('w') if ctrl => state.delete_word(),
        KeyCode::Char('e') if ctrl => state.move_cursor_end(),
        KeyCode::Backspace => state.backspace(),
        KeyCode::Delete => state.delete_char(),
        KeyCode::Left => state.move_cursor_left(),
        KeyCode::Right => state.move_cursor_right(),
        KeyCode::Char(ch) if !ctrl && !alt => state.insert_char(ch),

        _ => {}
    }

    None
}

fn render_ui(
    frame: &mut ratatui::Frame<'_>,
    state: &mut TuiState,
    theme: &Theme,
    preview_cfg: PreviewWindow,
    preview_visible: bool,
    preview_lines: &[String],
    elapsed_ms: u128,
) {
    let area = frame.area();
    if area.height < 3 || area.width < 10 {
        return;
    }

    let show_preview = preview_visible && area.width >= 40 && area.height >= 8;

    let (list_area, preview_area) = if show_preview {
        let dir = match preview_cfg.direction {
            PreviewDirection::Right | PreviewDirection::Left => Direction::Horizontal,
            PreviewDirection::Down | PreviewDirection::Up => Direction::Vertical,
        };
        let preview_pct = preview_cfg.percent;
        let main_pct = 100u16.saturating_sub(preview_pct);
        let constraints = match preview_cfg.direction {
            PreviewDirection::Right | PreviewDirection::Down => [
                Constraint::Percentage(main_pct),
                Constraint::Percentage(preview_pct),
            ],
            PreviewDirection::Left | PreviewDirection::Up => [
                Constraint::Percentage(preview_pct),
                Constraint::Percentage(main_pct),
            ],
        };
        let chunks = Layout::default()
            .direction(dir)
            .constraints(constraints)
            .split(area);
        match preview_cfg.direction {
            PreviewDirection::Right | PreviewDirection::Down => (chunks[0], Some(chunks[1])),
            PreviewDirection::Left | PreviewDirection::Up => (chunks[1], Some(chunks[0])),
        }
    } else {
        (area, None)
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(list_area);

    render_prompt(frame, main_chunks[0], state, theme, elapsed_ms);
    render_list(frame, main_chunks[1], state, theme);

    if let Some(p_area) = preview_area {
        let title = state
            .highlighted_entry()
            .map(|e| sanitize_control_chars(&e.display))
            .unwrap_or_else(|| "Preview".to_string());
        render_preview(frame, p_area, &title, preview_lines, theme);
    }
}

fn render_prompt(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &TuiState,
    theme: &Theme,
    elapsed_ms: u128,
) {
    let spinner = if state.loading {
        format!("{} ", spinner_char(false, elapsed_ms))
    } else {
        String::new()
    };
    let sel_info = if state.multi && !state.selected.is_empty() {
        format!(" [{}]", state.selected.len())
    } else {
        String::new()
    };
    let counts = format!(
        " {}{}/{} ",
        spinner,
        state.matched_count(),
        state.total_count()
    );
    let title = format!("rsfind{sel_info}");

    let sanitized_query = sanitize_control_chars(&state.query);
    let prompt_line = Line::from(vec![
        Span::styled("> ", theme.accent.add_modifier(Modifier::BOLD)),
        Span::styled(sanitized_query, theme.text),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Span::styled(title, theme.title))
        .title_bottom(Line::from(Span::styled(counts, theme.muted)).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(Paragraph::new(prompt_line).block(block), area);

    if inner.width > 2 && inner.height >= 1 {
        let cursor_x = inner
            .x
            .saturating_add(2)
            .saturating_add(state.cursor_pos as u16)
            .min(inner.x + inner.width - 1);
        frame.set_cursor_position((cursor_x, inner.y));
    }
}

fn render_list(frame: &mut ratatui::Frame<'_>, area: Rect, state: &mut TuiState, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border);
    let inner = block.inner(area);
    let vh = inner.height as usize;
    state.set_viewport_height(vh);

    if vh == 0 || inner.width == 0 {
        frame.render_widget(block, area);
        return;
    }

    let end = (state.scroll_offset + vh).min(state.matches.len());
    let items: Vec<ListItem<'_>> = (state.scroll_offset..end)
        .map(|row_idx| {
            let m = &state.matches[row_idx];
            let entry = &state.candidates[m.index];
            let is_cursor = row_idx == state.list_cursor;
            let is_selected = state.is_selected(m.index);

            let marker = if is_selected { "> " } else { "  " };
            let marker_style = if is_selected {
                theme.proc_running
            } else {
                theme.muted
            };

            let base_style = if is_cursor {
                theme.selected_row
            } else {
                theme.text
            };
            let hl_style = if is_cursor {
                theme
                    .selected_row
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                theme.accent.add_modifier(Modifier::BOLD)
            };

            let sanitized = sanitize_control_chars(&entry.display);
            let mut spans = Vec::with_capacity(m.indices.len() * 2 + 2);
            spans.push(Span::styled(
                marker,
                if is_cursor {
                    base_style.add_modifier(Modifier::BOLD)
                } else {
                    marker_style
                },
            ));
            push_highlighted_spans(&mut spans, &sanitized, &m.indices, base_style, hl_style);

            let item = ListItem::new(Line::from(spans));
            if is_cursor {
                item.style(theme.selected_row)
            } else {
                item
            }
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

fn push_highlighted_spans<'a>(
    spans: &mut Vec<Span<'a>>,
    text: &str,
    indices: &[u32],
    base_style: Style,
    hl_style: Style,
) {
    if indices.is_empty() {
        spans.push(Span::styled(text.to_string(), base_style));
        return;
    }

    let mut idx_pos = 0usize;
    let mut buf = String::new();
    let mut in_hl = false;

    for (char_idx, ch) in text.chars().enumerate() {
        while idx_pos < indices.len() && (indices[idx_pos] as usize) < char_idx {
            idx_pos += 1;
        }
        let matched = idx_pos < indices.len() && (indices[idx_pos] as usize) == char_idx;
        if matched != in_hl && !buf.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut buf),
                if in_hl { hl_style } else { base_style },
            ));
        }
        in_hl = matched;
        buf.push(ch);
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, if in_hl { hl_style } else { base_style }));
    }
}

fn render_preview(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    lines: &[String],
    theme: &Theme,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Span::styled(format!(" {title} "), theme.title));

    let text: Vec<Line<'_>> = lines
        .iter()
        .map(|l| Line::from(Span::styled(l.as_str(), theme.text)))
        .collect();

    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Compute preview lines using either custom `--preview <CMD>` or built-in preview.
pub fn compute_preview(path: &Path, preview_cmd: Option<&str>) -> Vec<String> {
    match preview_cmd {
        Some(cmd) => run_custom_preview(cmd, path),
        None => generate_builtin_preview(path),
    }
}

fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn run_custom_preview(cmd: &str, path: &Path) -> Vec<String> {
    let quoted = shell_quote(&path.to_string_lossy());
    let expanded = if cmd.contains("{}") {
        cmd.replace("{}", &quoted)
    } else {
        format!("{cmd} {quoted}")
    };

    match Command::new("sh").arg("-c").arg(&expanded).output() {
        Ok(output) => {
            let raw = if !output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                String::from_utf8_lossy(&output.stderr)
            };
            let lines: Vec<String> = raw
                .lines()
                .take(MAX_PREVIEW_LINES)
                .map(sanitize_control_chars)
                .collect();
            if lines.is_empty() {
                vec!["(no output)".to_string()]
            } else {
                lines
            }
        }
        Err(err) => vec![format!("Preview command failed: {err}")],
    }
}

/// Built-in preview for directories, UTF-8 text files, and binary files.
pub fn generate_builtin_preview(path: &Path) -> Vec<String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) => return vec![format!("Cannot inspect {}: {err}", path.display())],
    };

    if meta.is_dir() || path.is_dir() {
        return preview_directory(path);
    }

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(err) => return vec![format!("Cannot open {}: {err}", path.display())],
    };

    let mut buf = vec![0u8; MAX_PREVIEW_BYTES];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(err) => return vec![format!("Cannot read {}: {err}", path.display())],
    };
    buf.truncate(n);

    if buf.iter().take(8192).any(|&b| b == 0) {
        return vec![format!("Binary file ({})", format_bytes(meta.len()))];
    }

    match std::str::from_utf8(&buf) {
        Ok(text) => {
            if text.is_empty() {
                vec!["(empty file)".to_string()]
            } else {
                text.lines()
                    .take(MAX_PREVIEW_LINES)
                    .enumerate()
                    .map(|(i, line)| format!("{:>4} │ {}", i + 1, sanitize_control_chars(line)))
                    .collect()
            }
        }
        Err(_) => vec![format!("Binary file ({})", format_bytes(meta.len()))],
    }
}

fn preview_directory(path: &Path) -> Vec<String> {
    let read_dir = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(err) => return vec![format!("Cannot read directory: {err}")],
    };

    let mut entries: Vec<(bool, String)> = Vec::new();
    for entry in read_dir.flatten().take(MAX_DIR_ENTRIES) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push((is_dir, sanitize_control_chars(&name)));
    }

    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    if entries.is_empty() {
        return vec!["(empty directory)".to_string()];
    }

    entries
        .into_iter()
        .map(|(is_dir, name)| if is_dir { format!("{name}/") } else { name })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_entry(s: &str) -> FoundEntry {
        FoundEntry {
            path: PathBuf::from(s),
            display: s.to_string(),
            is_dir: false,
            is_symlink: false,
            is_exec: false,
        }
    }

    #[test]
    fn tui_state_query_editing_and_cursor_movement() {
        let mut state = TuiState::new(String::new(), CaseMode::Smart, false);
        state.push_entries([
            make_entry("src/main.rs"),
            make_entry("src/tui.rs"),
            make_entry("Cargo.toml"),
        ]);
        assert_eq!(state.total_count(), 3);
        assert_eq!(state.matched_count(), 3);

        state.insert_char('t');
        state.insert_char('u');
        state.insert_char('i');
        assert_eq!(state.query, "tui");
        assert_eq!(state.cursor_pos, 3);
        assert_eq!(state.matched_count(), 1);
        assert_eq!(state.highlighted_entry().unwrap().display, "src/tui.rs");

        state.move_cursor_left();
        state.move_cursor_left();
        assert_eq!(state.cursor_pos, 1);

        state.delete_char();
        assert_eq!(state.query, "ti");
        assert_eq!(state.cursor_pos, 1);

        state.move_cursor_end();
        assert_eq!(state.cursor_pos, 2);

        state.backspace();
        assert_eq!(state.query, "t");
        assert_eq!(state.cursor_pos, 1);

        state.insert_char(' ');
        state.insert_char('r');
        state.insert_char('s');
        assert_eq!(state.query, "t rs");
        state.delete_word();
        assert_eq!(state.query, "t ");
        state.clear_query();
        assert_eq!(state.query, "");
        assert_eq!(state.cursor_pos, 0);
        assert_eq!(state.matched_count(), 3);
    }

    #[test]
    fn tui_state_multi_select_toggling_and_confirm() {
        let mut state = TuiState::new(String::new(), CaseMode::Smart, true);
        state.push_entries([make_entry("a.rs"), make_entry("b.rs"), make_entry("c.rs")]);

        // Empty query sorts by length then original index: a.rs, b.rs, c.rs
        state.toggle_select(true); // selects a.rs, moves to b.rs
        assert_eq!(state.list_cursor, 1);
        assert!(state.is_selected(0));
        assert!(!state.is_selected(1));

        state.move_down(); // moves to c.rs
        state.toggle_select(false); // selects c.rs, moves up to b.rs
        assert_eq!(state.list_cursor, 1);
        assert!(state.is_selected(2));

        let confirmed = state.confirm_selection();
        let names: Vec<&str> = confirmed.iter().map(|e| e.display.as_str()).collect();
        assert_eq!(names, vec!["a.rs", "c.rs"]);

        // Select all & deselect all
        state.select_all();
        assert_eq!(state.selected.len(), 3);
        state.deselect_all();
        assert!(state.selected.is_empty());
        // With nothing selected, confirm falls back to highlighted item (b.rs)
        let fallback = state.confirm_selection();
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].display, "b.rs");
    }

    #[test]
    fn tui_state_scroll_offset_clamping() {
        let mut state = TuiState::new(String::new(), CaseMode::Smart, false);
        let entries: Vec<FoundEntry> = (0..20)
            .map(|i| make_entry(&format!("file_{i:02}.rs")))
            .collect();
        state.push_entries(entries);
        state.set_viewport_height(5);

        assert_eq!(state.scroll_offset, 0);
        for _ in 0..6 {
            state.move_down();
        }
        assert_eq!(state.list_cursor, 6);
        assert_eq!(state.scroll_offset, 2);

        state.page_down();
        assert_eq!(state.list_cursor, 11);
        assert_eq!(state.scroll_offset, 7);

        state.move_end();
        assert_eq!(state.list_cursor, 19);
        assert_eq!(state.scroll_offset, 15);

        state.move_home();
        assert_eq!(state.list_cursor, 0);
        assert_eq!(state.scroll_offset, 0);

        // Filtering down to 2 items clamps cursor and scroll_offset
        state.move_end();
        assert_eq!(state.list_cursor, 19);
        state.insert_char('0');
        state.insert_char('1');
        assert_eq!(state.matched_count(), 1);
        assert_eq!(state.list_cursor, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn preview_window_parsing() {
        let def = PreviewWindow::parse(None);
        assert_eq!(def.direction, PreviewDirection::Right);
        assert_eq!(def.percent, 50);
        assert!(def.visible);

        let hidden_down = PreviewWindow::parse(Some("down:40%:hidden"));
        assert_eq!(hidden_down.direction, PreviewDirection::Down);
        assert_eq!(hidden_down.percent, 40);
        assert!(!hidden_down.visible);
    }

    #[test]
    fn builtin_preview_handles_text_binary_and_directory() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("hello.txt");
        fs::write(&text_file, "line one\nline two\n").unwrap();
        let text_preview = generate_builtin_preview(&text_file);
        assert_eq!(text_preview, vec!["   1 │ line one", "   2 │ line two"]);

        let bin_file = dir.path().join("data.bin");
        fs::write(&bin_file, b"\x00\x01\x02\x03").unwrap();
        let bin_preview = generate_builtin_preview(&bin_file);
        assert_eq!(bin_preview, vec!["Binary file (4B)"]);

        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        let dir_preview = generate_builtin_preview(dir.path());
        assert_eq!(dir_preview[0], "subdir/");
        assert!(dir_preview.contains(&"hello.txt".to_string()));
    }

    #[test]
    fn key_events_abort_confirm_and_toggle_preview() {
        let mut state = TuiState::new(String::new(), CaseMode::Smart, false);
        state.push_entries([make_entry("foo.rs")]);
        let mut preview = true;

        let alt_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT);
        assert!(handle_key_event(alt_p, &mut state, &mut preview).is_none());
        assert!(!preview);

        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(esc, &mut state, &mut preview),
            Some(TuiOutcome::Aborted)
        );

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(enter, &mut state, &mut preview),
            Some(TuiOutcome::Selected(vec![make_entry("foo.rs")]))
        );
    }
}
