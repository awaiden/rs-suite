use crate::scan::Progress;
use crate::tree::{Item, SortBy};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use rs_common::fmt::{self, Fmt};
use rs_common::tui::{Theme, spinner_char};
use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Cfg {
    pub fmt: Fmt,
    pub apparent: bool,
    pub show_count: bool,
    pub show_mtime: bool,
    pub no_color: bool,
    pub no_unicode: bool,
    pub no_delete: bool,
    /// Set when scanned from the live filesystem (not imported): enables `r` and live stat in `i`.
    pub scan_opts: Option<crate::scan::Opts>,
}

pub enum Source {
    Ready(Item),
    /// Scan runs on another thread; `String` is the scanned path.
    Scanning(JoinHandle<Item>, Arc<Progress>, String),
}

struct Scanning {
    handle: JoinHandle<Item>,
    prog: Arc<Progress>,
    path: String,
    started: Instant,
    /// None: the initial scan. Some(path): rescan of the directory at that tree path.
    target: Option<Vec<usize>>,
}

struct Deleting {
    name: String,
    size: i64,
    idx: usize,
    path: Vec<usize>,
    handle: JoinHandle<io::Result<()>>,
    started: Instant,
}

struct App {
    root: Item,
    path: Vec<usize>,
    sel: ListState,
    cfg: Cfg,
    theme: Theme,
    by: SortBy,
    desc: bool,
    confirm: bool,
    help: bool,
    /// Lines of the open info window.
    info: Option<Vec<String>>,
    scanning: Option<Scanning>,
    deleting: Option<Deleting>,
    /// (text, is_error)
    status: Option<(String, bool)>,
    /// Something changed since the last draw (idle browsing needs no redraw).
    dirty: bool,
}

fn cur<'a>(root: &'a Item, path: &[usize]) -> &'a Item {
    path.iter().fold(root, |d, &i| &d.children()[i])
}

fn mtime_str(t: i64) -> String {
    // civil date from unix days (UTC)
    let z = t.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Keep the tail of a long path so the deepest dir stays visible.
fn tail(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        return s.to_string();
    }
    let keep = w.saturating_sub(1);
    format!("…{}", s.chars().skip(n - keep).collect::<String>())
}

pub fn run(src: Source, cfg: Cfg) -> io::Result<()> {
    let mut app = App::new(cfg);
    match src {
        Source::Ready(root) => app.set_root(root),
        Source::Scanning(handle, prog, path) => {
            app.scanning = Some(Scanning {
                handle,
                prog,
                path,
                started: Instant::now(),
                target: None,
            })
        }
    }
    let mut term = ratatui::init();
    let res = (|| loop {
        app.tick();
        // scanning and deleting animate (spinner, counters); otherwise draw only after a change
        if app.dirty || app.scanning.is_some() || app.deleting.is_some() {
            term.draw(|f| app.draw(f))?;
            app.dirty = false;
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let k = match event::read()? {
            Event::Key(k) => k,
            Event::Resize(..) => {
                app.dirty = true;
                continue;
            }
            _ => continue,
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        app.dirty = true;
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return Ok(());
        }
        if app.key(k.code) {
            return Ok(());
        }
    })();
    ratatui::restore();
    res
}

impl App {
    fn set_root(&mut self, root: Item) {
        self.root = root;
        self.resort();
        self.sel.select(Some(0));
    }

    fn new(cfg: Cfg) -> Self {
        let theme = Theme::new(cfg.no_color);
        Self {
            root: Item::default(),
            path: vec![],
            sel: ListState::default(),
            cfg,
            theme,
            by: SortBy::Size,
            desc: true,
            confirm: false,
            help: false,
            info: None,
            scanning: None,
            deleting: None,
            status: None,
            dirty: true,
        }
    }

    /// Lines for the info window of child `idx` of the current directory.
    fn info_lines(&self, idx: usize) -> Vec<String> {
        let it = &cur(&self.root, &self.path).children()[idx];
        let p = self.root.path_of(&self.path, idx);
        let l = |k: &str, v: String| format!("  {k:<14}{v}");
        let kind = match (it.is_dir(), it.flag()) {
            (true, _) => "directory",
            (_, '@') => "symlink / special file",
            (_, 'H') => "file (hardlink)",
            _ => "file",
        };
        let mut v = vec![
            l("Name", rs_common::sanitize_control_chars(&it.name)),
            l(
                "Path",
                tail(&rs_common::sanitize_control_chars(&p.to_string_lossy()), 54),
            ),
            l("Type", kind.into()),
            l("Disk usage", self.cfg.fmt.size(it.usage)),
            l("Apparent size", self.cfg.fmt.size(it.size)),
        ];
        if it.is_dir() {
            v.push(l("Items", fmt::count(it.display_count())));
        }
        v.push(l("Modified", mtime_str(it.mtime)));
        let status = match it.flag() {
            '!' => Some("! read error"),
            '.' => Some(". error in a subdirectory"),
            'e' => Some("e empty directory"),
            _ => None,
        };
        if let Some(s) = status {
            v.push(l("Status", s.into()));
        }
        if self.cfg.scan_opts.is_some()
            && let Ok(m) = std::fs::symlink_metadata(&p)
        {
            use std::os::unix::fs::MetadataExt;
            v.push(l("Links", m.nlink().to_string()));
            v.push(l("Inode", m.ino().to_string()));
            v.push(l("Owner", format!("{}:{}", m.uid(), m.gid())));
            v.push(l("Mode", format!("{:04o}", m.mode() & 0o7777)));
        }
        v
    }

    fn metric(&self, i: &Item) -> i64 {
        if self.cfg.apparent { i.size } else { i.usage }
    }

    /// Sort only the currently viewed directory on demand.
    fn resort(&mut self) {
        let by = match self.by {
            SortBy::Size if self.cfg.apparent => SortBy::Apparent,
            b => b,
        };
        self.root.dir_at(&self.path).sort(by, self.desc);
    }

    fn set_sort(&mut self, by: SortBy) {
        self.desc = if self.by == by {
            !self.desc
        } else {
            by != SortBy::Name
        };
        self.by = by;
        self.resort();
    }

    /// Pick up finished background work (scan or delete).
    fn tick(&mut self) {
        if self
            .scanning
            .as_ref()
            .is_some_and(|s| s.handle.is_finished())
        {
            let s = self.scanning.take().unwrap();
            match (s.handle.join(), s.target) {
                (Ok(root), None) => self.set_root(root),
                (Ok(mut item), Some(path)) => {
                    item.name = self.root.dir_at(&path).name.clone();
                    self.root.replace_at(&path, item);
                    self.resort();
                    let n = cur(&self.root, &self.path).children().len();
                    self.sel.select((n > 0).then_some(0));
                }
                (Err(_), _) => self.status = Some(("Scan thread panicked".into(), true)),
            }
            self.dirty = true;
        }
        if self
            .deleting
            .as_ref()
            .is_some_and(|d| d.handle.is_finished())
        {
            let d = self.deleting.take().unwrap();
            let d_name = rs_common::sanitize_control_chars(&d.name);
            self.status = Some(match d.handle.join() {
                Ok(Ok(())) => {
                    self.root.delete_child(&d.path, d.idx);
                    // totals changed, so sibling order may have too
                    self.resort();
                    let n = cur(&self.root, &self.path).children().len();
                    self.sel
                        .select(if n == 0 { None } else { Some(d.idx.min(n - 1)) });
                    (
                        format!("Deleted {} (freed {})", d_name, self.cfg.fmt.size(d.size)),
                        false,
                    )
                }
                Ok(Err(e)) => (format!("Error deleting {}: {e}", d_name), true),
                Err(_) => (format!("Delete of {} panicked", d_name), true),
            });
            self.dirty = true;
        }
    }

    /// Returns true to quit.
    fn key(&mut self, code: KeyCode) -> bool {
        if self.scanning.is_some() {
            return matches!(code, KeyCode::Char('q' | 'Q') | KeyCode::Esc);
        }
        if self.deleting.is_some() {
            return false; // busy: ignore keys until the delete finishes
        }
        self.status = None;
        if self.help || self.info.is_some() {
            self.help = false;
            self.info = None;
            return false;
        }
        if self.confirm {
            self.confirm = false;
            if matches!(code, KeyCode::Char('y')) {
                self.start_delete();
            }
            return false;
        }
        let n = cur(&self.root, &self.path).children().len();
        let i = self.sel.selected().unwrap_or(0);
        match code {
            KeyCode::Char('q' | 'Q') => return true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => self.start_rescan(),
            KeyCode::Char('i') if n > 0 => self.info = Some(self.info_lines(i)),
            KeyCode::Down | KeyCode::Char('j') => {
                self.sel.select(Some((i + 1).min(n.saturating_sub(1))))
            }
            KeyCode::Up | KeyCode::Char('k') => self.sel.select(Some(i.saturating_sub(1))),
            KeyCode::Home | KeyCode::Char('g') => self.sel.select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => self.sel.select(Some(n.saturating_sub(1))),
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => {
                if cur(&self.root, &self.path)
                    .children()
                    .get(i)
                    .is_some_and(Item::is_dir)
                {
                    self.path.push(i);
                    self.resort();
                    self.sel.select(Some(0));
                }
            }
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => {
                if let Some(p) = self.path.pop() {
                    let prev_name = self.root.dir_at(&self.path).children()[p].name.clone();
                    self.resort();
                    let new_idx = cur(&self.root, &self.path)
                        .children()
                        .iter()
                        .position(|c| c.name == prev_name)
                        .unwrap_or(p);
                    self.sel.select(Some(new_idx));
                }
            }
            KeyCode::Char('a') => {
                self.cfg.apparent = !self.cfg.apparent;
                self.resort();
            }
            KeyCode::Char('c') => self.cfg.show_count = !self.cfg.show_count,
            KeyCode::Char('m') => self.cfg.show_mtime = !self.cfg.show_mtime,
            KeyCode::Char('s') => self.set_sort(SortBy::Size),
            KeyCode::Char('C') => self.set_sort(SortBy::Count),
            KeyCode::Char('n') => self.set_sort(SortBy::Name),
            KeyCode::Char('M') => self.set_sort(SortBy::Mtime),
            KeyCode::Char('d') => {
                if self.cfg.no_delete {
                    self.status = Some((
                        "Deletion is disabled (--no-delete or imported file)".into(),
                        true,
                    ));
                } else if n > 0 {
                    self.confirm = true;
                }
            }
            _ => {}
        }
        false
    }

    /// Rescan the directory being viewed on a worker thread (the whole tree when at the root).
    fn start_rescan(&mut self) {
        let Some(mut opts) = self.cfg.scan_opts.clone() else {
            self.status = Some(("Rescan is not available for imported files".into(), true));
            return;
        };
        let p = match self.path.split_last() {
            None => std::path::PathBuf::from(&self.root.name),
            Some((&i, up)) => {
                // keep `--no-cross` anchored to the original root's device
                if opts.no_cross {
                    opts.root_dev = std::fs::symlink_metadata(&self.root.name)
                        .ok()
                        .map(|m| std::os::unix::fs::MetadataExt::dev(&m));
                }
                self.root.path_of(up, i)
            }
        };
        let prog = Arc::new(Progress::active());
        let pr = prog.clone();
        let pp = p.clone();
        self.scanning = Some(Scanning {
            handle: std::thread::spawn(move || crate::scan::scan(&pp, &opts, &pr)),
            prog,
            path: p.to_string_lossy().into_owned(),
            started: Instant::now(),
            target: Some(self.path.clone()),
        });
    }

    fn start_delete(&mut self) {
        let Some(idx) = self.sel.selected() else {
            return;
        };
        let p = self.root.path_of(&self.path, idx);
        let item = &cur(&self.root, &self.path).children()[idx];
        let (name, size) = (item.name.to_string(), self.metric(item));
        let handle = std::thread::spawn(move || {
            if std::fs::symlink_metadata(&p)?.file_type().is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            }
        });
        self.deleting = Some(Deleting {
            name,
            size,
            idx,
            path: self.path.clone(),
            handle,
            started: Instant::now(),
        });
    }

    fn draw(&mut self, f: &mut Frame) {
        let [head, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .areas(f.area());
        let bar = self.theme.bar_style();

        if let Some(s) = &self.scanning {
            f.render_widget(Paragraph::new(" rsdu").style(bar), head);
            let p = &s.prog;
            let lines = vec![
                Line::from(format!(
                    "{} Scanning {}",
                    spinner_char(self.cfg.no_unicode, s.started.elapsed().as_millis()),
                    tail(&rs_common::sanitize_control_chars(&s.path), 50)
                )),
                Line::from(""),
                Line::from(format!(
                    "Items:    {}",
                    fmt::count(p.items.load(Relaxed) as i64)
                )),
                Line::from(format!(
                    "Size:     {}",
                    self.cfg.fmt.size(p.bytes.load(Relaxed) as i64)
                )),
                Line::from(format!("Elapsed:  {}s", s.started.elapsed().as_secs())),
                Line::from(""),
                Line::from(format!(
                    "Current:  {}",
                    tail(
                        &rs_common::sanitize_control_chars(&p.current.lock().unwrap()),
                        48
                    )
                )),
                Line::from(""),
                Line::from("q / Esc to quit").style(
                    self.theme
                        .style_or_default(Style::default().add_modifier(Modifier::DIM)),
                ),
            ];
            popup(
                f,
                64,
                lines.len() as u16 + 2,
                " Scanning ",
                self.theme
                    .style_or_default(Style::default().fg(Color::Cyan)),
                lines,
            );
            return;
        }

        let dir = cur(&self.root, &self.path);
        let mut shown = std::path::PathBuf::from(&self.root.name);
        let mut d = &self.root;
        for &i in &self.path {
            d = &d.children()[i];
            shown.push(&d.name);
        }
        f.render_widget(
            Paragraph::new(format!(
                " rsdu  {}",
                rs_common::sanitize_control_chars(&shown.to_string_lossy())
            ))
            .style(bar),
            head,
        );

        let children = dir.children();
        let max = children
            .iter()
            .map(|c| self.metric(c))
            .max()
            .unwrap_or(0)
            .max(1);

        // Compute viewport window so we only build ListItems for visible rows
        let height = body.height as usize;
        let total = children.len();
        let selected = self
            .sel
            .selected()
            .unwrap_or(0)
            .min(total.saturating_sub(1));
        let mut offset = self.sel.offset();
        if selected < offset {
            offset = selected;
        } else if height > 0 && selected >= offset + height {
            offset = selected + 1 - height;
        }
        *self.sel.offset_mut() = offset;
        let end = (offset + height).min(total);

        const UNI_FULL: &str = "██████████";
        const UNI_EMPTY: &str = "░░░░░░░░░░";
        const ASC_FULL: &str = "##########";
        const ASC_EMPTY: &str = "          ";

        let rows: Vec<ListItem> = children[offset..end]
            .iter()
            .map(|c| {
                let m = self.metric(c);
                let filled = (m.saturating_mul(10) / max).clamp(0, 10) as usize;
                let (on, off) = if self.cfg.no_unicode {
                    (&ASC_FULL[..filled], &ASC_EMPTY[..10 - filled])
                } else {
                    (&UNI_FULL[..filled * 3], &UNI_EMPTY[..(10 - filled) * 3])
                };
                let mut spans = Vec::with_capacity(9);
                spans.push(Span::styled(
                    format!("{} ", c.flag()),
                    self.theme
                        .style_or_default(Style::default().fg(Color::DarkGray)),
                ));
                spans.push(Span::raw(format!("{:>10} ", self.cfg.fmt.size(m))));
                spans.push(Span::raw("["));
                spans.push(Span::styled(
                    on,
                    self.theme
                        .style_or_default(Style::default().fg(Color::Green)),
                ));
                spans.push(Span::styled(
                    off,
                    self.theme
                        .style_or_default(Style::default().fg(Color::DarkGray)),
                ));
                spans.push(Span::raw("] "));
                if self.cfg.show_count {
                    spans.push(Span::raw(format!("{:>6} ", fmt::count(c.display_count()))));
                }
                if self.cfg.show_mtime {
                    spans.push(Span::raw(format!("{} ", mtime_str(c.mtime))));
                }
                let cname = rs_common::sanitize_control_chars(&c.name);
                if c.is_dir() {
                    spans.push(Span::styled(
                        format!("/{cname}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::raw(cname));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let hl = if self.cfg.no_color {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        };
        let mut view_state = ListState::default();
        if total > 0 {
            view_state.select(Some(selected - offset));
        }
        f.render_stateful_widget(List::new(rows).highlight_style(hl), body, &mut view_state);

        let totals = format!(
            " Total disk usage: {}  Apparent size: {}  Items: {}",
            self.cfg.fmt.size(dir.usage),
            self.cfg.fmt.size(dir.size),
            dir.display_count()
        );
        let [l1, l2] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(foot);
        f.render_widget(Paragraph::new(totals).style(bar), l1);
        let line = match &self.status {
            Some((t, true)) => Line::from(format!(" {t}"))
                .style(self.theme.style_or_default(
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
            Some((t, false)) => Line::from(format!(" {t}")).style(
                self.theme
                    .style_or_default(Style::default().fg(Color::Green)),
            ),
            None => Line::from(" ?: help   q: quit"),
        };
        f.render_widget(Paragraph::new(line), l2);

        if self.confirm {
            if let Some(c) = self.sel.selected().and_then(|i| children.get(i)) {
                let cname = rs_common::sanitize_control_chars(&c.name);
                let label = if c.is_dir() {
                    format!("/{cname}")
                } else {
                    cname
                };
                let lines = vec![
                    Line::from(format!(
                        "Delete {label} ({})?",
                        self.cfg.fmt.size(self.metric(c))
                    )),
                    Line::from(""),
                    Line::from("y: delete   any other key: cancel"),
                ];
                popup(
                    f,
                    56,
                    5,
                    " Delete ",
                    self.theme.style_or_default(Style::default().fg(Color::Red)),
                    lines,
                );
            }
        }
        if let Some(d) = &self.deleting {
            let lines = vec![
                Line::from(format!(
                    "{} Deleting {} ({})…",
                    spinner_char(self.cfg.no_unicode, d.started.elapsed().as_millis()),
                    rs_common::sanitize_control_chars(&d.name),
                    self.cfg.fmt.size(d.size)
                )),
                Line::from(""),
                Line::from(format!("Elapsed: {}s", d.started.elapsed().as_secs())),
            ];
            popup(
                f,
                56,
                5,
                " Deleting ",
                self.theme
                    .style_or_default(Style::default().fg(Color::Yellow)),
                lines,
            );
        }
        if let Some(v) = &self.info {
            let mut lines: Vec<Line> = v.iter().map(|s| Line::from(s.as_str())).collect();
            lines.push(Line::from(""));
            lines.push(Line::from("  press any key to close"));
            popup(
                f,
                72,
                lines.len() as u16 + 2,
                " Info ",
                self.theme
                    .style_or_default(Style::default().fg(Color::Cyan)),
                lines,
            );
        }
        if self.help {
            let k = |a: &str, b: &str| Line::from(format!("  {a:<18}{b}"));
            let lines = vec![
                k("j/k ↑/↓", "move"),
                k("l/→/Enter", "open directory"),
                k("h/←/Backspace", "parent directory"),
                k("g/G", "first / last"),
                k("a", "apparent size / disk usage"),
                k("c  m", "item count / mtime column"),
                k("s C n M", "sort by size/count/name/mtime"),
                k("", "(again = reverse)"),
                k("i", "item info"),
                k("r", "rescan current directory"),
                k("d", "delete"),
                k("q", "quit"),
                Line::from(""),
                Line::from("  press any key to close"),
            ];
            popup(
                f,
                56,
                lines.len() as u16 + 2,
                " Help ",
                self.theme
                    .style_or_default(Style::default().fg(Color::Cyan)),
                lines,
            );
        }
    }
}

fn popup(f: &mut Frame, w: u16, h: u16, title: &str, border: Style, lines: Vec<Line>) {
    let a = f.area();
    let r = Rect {
        x: a.x + a.width.saturating_sub(w) / 2,
        y: a.y + a.height.saturating_sub(h) / 2,
        width: w.min(a.width),
        height: h.min(a.height),
    };
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title).border_style(border)),
        r,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn screen(app: &mut App) -> String {
        let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        t.backend().to_string()
    }

    #[test]
    fn info_window_shows_file_and_dir() {
        let mut root = Item::new_dir("/r", ' ');
        root.children_mut()
            .push(Item::new_file("f.txt", 'H', 100, 4096, 0, 7));
        let mut sub = Item::new_dir("sub", ' ');
        sub.children_mut()
            .push(Item::new_file("x", ' ', 1, 4096, 0, 0));
        root.children_mut().push(sub);
        root.update_stats();
        let mut app = App::new(Cfg {
            fmt: Fmt::default(),
            apparent: false,
            show_count: false,
            show_mtime: false,
            no_color: true,
            no_unicode: true,
            no_delete: false,
            scan_opts: None,
        });
        app.set_root(root);
        for (name, want) in [("f.txt", "file (hardlink)"), ("sub", "directory")] {
            let i = cur(&app.root, &app.path)
                .children()
                .iter()
                .position(|c| &*c.name == name)
                .unwrap();
            app.sel.select(Some(i));
            app.key(KeyCode::Char('i'));
            let out = screen(&mut app);
            assert!(out.contains(want), "{out}");
            assert!(out.contains(&format!("/r/{name}")), "{out}");
            assert!(out.contains("1970-01-01"), "{out}");
            app.key(KeyCode::Char('x'));
            assert!(app.info.is_none());
        }
    }

    fn test_cfg(no_delete: bool) -> Cfg {
        Cfg {
            fmt: Fmt::default(),
            apparent: false,
            show_count: false,
            show_mtime: false,
            no_color: true,
            no_unicode: true,
            no_delete,
            scan_opts: None,
        }
    }

    #[test]
    fn delete_confirm_flow_and_read_only_blocking() {
        let mut root = Item::new_dir("/nonexistent_rsdu_test_dir", ' ');
        root.children_mut()
            .push(Item::new_file("a.txt", ' ', 10, 4096, 0, 0));
        root.update_stats();

        // Read-only mode blocks deletion
        let mut ro = App::new(test_cfg(true));
        ro.set_root(root.clone());
        ro.sel.select(Some(0));
        ro.key(KeyCode::Char('d'));
        assert!(!ro.confirm);
        assert!(
            ro.status
                .as_ref()
                .is_some_and(|(m, err)| *err && m.contains("Deletion is disabled"))
        );

        // Normal mode: d opens confirm, n cancels without deleting
        let mut rw = App::new(test_cfg(false));
        rw.set_root(root);
        rw.sel.select(Some(0));
        rw.key(KeyCode::Char('d'));
        assert!(rw.confirm);
        rw.key(KeyCode::Char('n'));
        assert!(!rw.confirm);
        assert!(rw.deleting.is_none());
        assert_eq!(cur(&rw.root, &rw.path).children().len(), 1);
    }

    #[test]
    fn start_delete_and_tick_removes_file_and_dir_in_tempdir() {
        let dir = std::env::temp_dir().join(format!("rsdu_tui_del_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("victim.txt"), b"hello").unwrap();
        std::fs::write(dir.join("sub/inner.txt"), b"world").unwrap();

        let mut root = Item::new_dir(dir.to_str().unwrap(), ' ');
        root.children_mut()
            .push(Item::new_file("victim.txt", ' ', 5, 4096, 0, 0));
        let mut sub = Item::new_dir("sub", ' ');
        sub.children_mut()
            .push(Item::new_file("inner.txt", ' ', 5, 4096, 0, 0));
        root.children_mut().push(sub);
        root.update_stats();

        let mut app = App::new(test_cfg(false));
        app.set_root(root);
        app.sel.select(Some(0));

        // Confirm deletion via key('d') -> key('y')
        app.key(KeyCode::Char('d'));
        assert!(app.confirm);
        app.key(KeyCode::Char('y'));
        assert!(!app.confirm);
        assert!(app.deleting.is_some());

        for _ in 0..200 {
            app.tick();
            if app.deleting.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.deleting.is_none(), "delete task did not finish");
        assert_eq!(cur(&app.root, &app.path).children().len(), 1);
        assert!(!dir.join("victim.txt").exists() || !dir.join("sub").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
