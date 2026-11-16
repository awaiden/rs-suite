use std::cmp::Ordering;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::cli::{CliArgs, SortColumn, UnitFamily, ViewMode};
use crate::model::{
    CaptureStatus, ConnectionTraffic, NetSnapshot, ProcessTraffic, RemoteTraffic, TrafficCounters,
};

const PAGE_STEP: usize = 10;

/// Active input / overlay mode for the `rsnet` TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Filter,
    Help,
}

/// Complete interactive TUI state for `rsnet`.
#[derive(Debug, Clone)]
pub struct AppState {
    pub snapshot: NetSnapshot,
    pub view_mode: ViewMode,
    pub sort_column: SortColumn,
    pub sort_desc: bool,
    pub selected_row: usize,
    pub scroll_offset: usize,
    pub filter_text: String,
    pub input_mode: InputMode,
    pub paused: bool,
    pub cumulative: bool,
    pub unit_family: UnitFamily,
    pub no_resolve: bool,
    pub no_color: bool,
    pub should_quit: bool,
    pub status_message: Option<(String, Instant)>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            snapshot: empty_snapshot(),
            view_mode: ViewMode::All,
            sort_column: SortColumn::Total,
            sort_desc: true,
            selected_row: 0,
            scroll_offset: 0,
            filter_text: String::new(),
            input_mode: InputMode::Normal,
            paused: false,
            cumulative: false,
            unit_family: UnitFamily::BinBytes,
            no_resolve: false,
            no_color: false,
            should_quit: false,
            status_message: None,
        }
    }
}

impl AppState {
    /// Constructs an `AppState` initialized from parsed CLI arguments.
    pub fn from_cli(args: &CliArgs) -> Self {
        Self {
            view_mode: ViewMode::from_args(args),
            sort_column: args.sort,
            sort_desc: default_sort_desc(args.sort),
            filter_text: args.filter.clone().unwrap_or_default(),
            cumulative: args.total,
            unit_family: args.unit_family,
            no_resolve: args.no_resolve,
            no_color: args.no_color,
            ..Self::default()
        }
    }

    /// Replaces the current snapshot unless paused, and clamps selection bounds.
    pub fn update_snapshot(&mut self, snapshot: NetSnapshot) {
        if self.paused {
            return;
        }
        self.snapshot = snapshot;
        self.clamp_selection();
    }

    /// Sets a transient status message stamped with the current instant.
    pub fn set_status_message(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    /// Clears `status_message` if it has been displayed for at least `ttl`.
    pub fn clear_expired_status(&mut self, ttl: Duration) {
        if let Some((_, created_at)) = &self.status_message
            && created_at.elapsed() >= ttl
        {
            self.status_message = None;
        }
    }

    /// Adjusts `scroll_offset` so `selected_row` remains visible within `viewport_height`.
    pub fn adjust_scroll(&mut self, viewport_height: usize) {
        let total = self.active_row_count();
        if viewport_height == 0 || total == 0 {
            self.scroll_offset = 0;
            return;
        }
        if self.selected_row < self.scroll_offset {
            self.scroll_offset = self.selected_row;
        } else if self.selected_row >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_row + 1 - viewport_height;
        }
        let max_offset = total.saturating_sub(viewport_height);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    /// Returns filtered and sorted process rows for rendering.
    pub fn visible_processes(&self) -> Vec<&ProcessTraffic> {
        let needle = normalized_filter(&self.filter_text);
        let mut rows: Vec<&ProcessTraffic> = self
            .snapshot
            .processes
            .iter()
            .filter(|item| match needle.as_deref() {
                None => true,
                Some(q) => {
                    contains_ignore_case(&item.process.name, q)
                        || contains_ignore_case(&item.process.cmdline, q)
                        || item.process.pid.to_string().contains(q)
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            compare_rows(
                a.counters,
                a.process.pid,
                &a.process.name,
                b.counters,
                b.process.pid,
                &b.process.name,
                self.sort_column,
                self.sort_desc,
            )
        });
        rows
    }

    /// Returns filtered and sorted connection rows for rendering.
    pub fn visible_connections(&self) -> Vec<&ConnectionTraffic> {
        let needle = normalized_filter(&self.filter_text);
        let mut rows: Vec<&ConnectionTraffic> = self
            .snapshot
            .connections
            .iter()
            .filter(|item| match needle.as_deref() {
                None => true,
                Some(q) => {
                    let proc_match = item.process.as_ref().is_some_and(|p| {
                        contains_ignore_case(&p.name, q) || p.pid.to_string().contains(q)
                    });
                    let host_match = !self.no_resolve
                        && item
                            .hostname
                            .as_deref()
                            .is_some_and(|h| contains_ignore_case(h, q));
                    proc_match
                        || host_match
                        || item.key.local.ip.to_string().contains(q)
                        || item.key.remote.ip.to_string().contains(q)
                        || item.key.local.port.to_string().contains(q)
                        || item.key.remote.port.to_string().contains(q)
                        || contains_ignore_case(&item.key.protocol.to_string(), q)
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            let a_pid = a.process.as_ref().map_or(u32::MAX, |p| p.pid);
            let b_pid = b.process.as_ref().map_or(u32::MAX, |p| p.pid);
            let a_name = a
                .process
                .as_ref()
                .map(|p| p.name.as_str())
                .unwrap_or_default();
            let b_name = b
                .process
                .as_ref()
                .map(|p| p.name.as_str())
                .unwrap_or_default();
            compare_rows(
                a.counters,
                a_pid,
                a_name,
                b.counters,
                b_pid,
                b_name,
                self.sort_column,
                self.sort_desc,
            )
        });
        rows
    }

    /// Returns filtered and sorted remote address rows for rendering.
    pub fn visible_remotes(&self) -> Vec<&RemoteTraffic> {
        let needle = normalized_filter(&self.filter_text);
        let mut rows: Vec<&RemoteTraffic> = self
            .snapshot
            .remotes
            .iter()
            .filter(|item| match needle.as_deref() {
                None => true,
                Some(q) => {
                    let host_match = !self.no_resolve
                        && item
                            .hostname
                            .as_deref()
                            .is_some_and(|h| contains_ignore_case(h, q));
                    host_match || item.ip.to_string().contains(q)
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            let a_label = (!self.no_resolve)
                .then_some(a.hostname.as_deref())
                .flatten()
                .map(str::to_string)
                .unwrap_or_else(|| a.ip.to_string());
            let b_label = (!self.no_resolve)
                .then_some(b.hostname.as_deref())
                .flatten()
                .map(str::to_string)
                .unwrap_or_else(|| b.ip.to_string());
            compare_rows(
                a.counters,
                0,
                &a_label,
                b.counters,
                0,
                &b_label,
                self.sort_column,
                self.sort_desc,
            )
        });
        rows
    }

    /// Returns the number of rows in the currently focused pane.
    pub fn active_row_count(&self) -> usize {
        match self.view_mode {
            ViewMode::All | ViewMode::Processes => self.visible_processes().len(),
            ViewMode::Connections => self.visible_connections().len(),
            ViewMode::Addresses => self.visible_remotes().len(),
        }
    }

    /// Processes a terminal key event and returns `true` if the application should quit.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.should_quit = true;
            return true;
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::Help => self.handle_help_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if ctrl || alt {
            return false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => {
                self.should_quit = true;
                return true;
            }
            KeyCode::Tab => {
                self.view_mode = next_view_mode(self.view_mode);
                self.reset_selection();
            }
            KeyCode::Char('0') => {
                self.view_mode = ViewMode::All;
                self.reset_selection();
            }
            KeyCode::Char('1') => {
                self.view_mode = ViewMode::Processes;
                self.reset_selection();
            }
            KeyCode::Char('2') => {
                self.view_mode = ViewMode::Connections;
                self.reset_selection();
            }
            KeyCode::Char('3') => {
                self.view_mode = ViewMode::Addresses;
                self.reset_selection();
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_by(-1),
            KeyCode::PageDown => self.move_selection_by(PAGE_STEP as isize),
            KeyCode::PageUp => self.move_selection_by(-(PAGE_STEP as isize)),
            KeyCode::Home | KeyCode::Char('g') => self.reset_selection(),
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_row = self.active_row_count().saturating_sub(1);
            }
            KeyCode::Char('s') => {
                self.sort_column = next_sort_column(self.sort_column);
                self.sort_desc = default_sort_desc(self.sort_column);
                self.clamp_selection();
            }
            KeyCode::Char('r') => {
                self.sort_desc = !self.sort_desc;
                self.clamp_selection();
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
            }
            KeyCode::Char(' ') | KeyCode::Char('p') => {
                self.paused = !self.paused;
                let msg = if self.paused {
                    "Capture display paused"
                } else {
                    "Capture display resumed"
                };
                self.set_status_message(msg);
            }
            KeyCode::Char('t') => {
                self.cumulative = !self.cumulative;
                let msg = if self.cumulative {
                    "Showing cumulative totals"
                } else {
                    "Showing per-second rates"
                };
                self.set_status_message(msg);
            }
            KeyCode::Char('u') => {
                self.unit_family = next_unit_family(self.unit_family);
                self.set_status_message(format!("Unit: {}", unit_family_label(self.unit_family)));
            }
            KeyCode::Char('n') => {
                self.no_resolve = !self.no_resolve;
                let msg = if self.no_resolve {
                    "Reverse DNS disabled"
                } else {
                    "Reverse DNS enabled"
                };
                self.set_status_message(msg);
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.input_mode = InputMode::Help;
            }
            KeyCode::Esc if !self.filter_text.is_empty() => {
                self.filter_text.clear();
                self.clamp_selection();
            }
            _ => {}
        }
        false
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if ctrl || alt {
            return false;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.filter_text.pop();
                self.clamp_selection();
            }
            KeyCode::Char(c) if !c.is_control() => {
                self.filter_text.push(c);
                self.clamp_selection();
            }
            _ => {}
        }
        false
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc
            | KeyCode::Enter
            | KeyCode::Char('q')
            | KeyCode::Char('?')
            | KeyCode::F(1) => {
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
        false
    }

    fn reset_selection(&mut self) {
        self.selected_row = 0;
        self.scroll_offset = 0;
    }

    fn clamp_selection(&mut self) {
        let count = self.active_row_count();
        if count == 0 {
            self.selected_row = 0;
            self.scroll_offset = 0;
        } else if self.selected_row >= count {
            self.selected_row = count - 1;
        }
    }

    fn move_selection_by(&mut self, delta: isize) {
        let count = self.active_row_count();
        if count == 0 {
            self.selected_row = 0;
            self.scroll_offset = 0;
            return;
        }
        let max_idx = count - 1;
        let next = if delta >= 0 {
            self.selected_row
                .saturating_add(delta as usize)
                .min(max_idx)
        } else {
            self.selected_row.saturating_sub(delta.unsigned_abs())
        };
        self.selected_row = next;
    }
}

/// Returns a short human-readable label for a `UnitFamily`.
pub fn unit_family_label(unit: UnitFamily) -> &'static str {
    match unit {
        UnitFamily::BinBytes => "BinBytes",
        UnitFamily::BinBits => "BinBits",
        UnitFamily::SiBytes => "SiBytes",
        UnitFamily::SiBits => "SiBits",
    }
}

/// Returns a short human-readable label for a `SortColumn`.
pub fn sort_column_label(col: SortColumn) -> &'static str {
    match col {
        SortColumn::Total => "Total",
        SortColumn::Rx => "Rx",
        SortColumn::Tx => "Tx",
        SortColumn::Pid => "PID",
        SortColumn::Name => "Name",
    }
}

/// Returns a short human-readable label for a `ViewMode`.
pub fn view_mode_label(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::All => "All",
        ViewMode::Processes => "Processes",
        ViewMode::Connections => "Connections",
        ViewMode::Addresses => "Addresses",
    }
}

fn empty_snapshot() -> NetSnapshot {
    NetSnapshot {
        interval_secs: 1.0,
        status: CaptureStatus::Live,
        total: TrafficCounters::default(),
        interfaces: Vec::new(),
        processes: Vec::new(),
        connections: Vec::new(),
        remotes: Vec::new(),
    }
}

fn default_sort_desc(col: SortColumn) -> bool {
    !matches!(col, SortColumn::Pid | SortColumn::Name)
}

fn next_view_mode(mode: ViewMode) -> ViewMode {
    match mode {
        ViewMode::All => ViewMode::Processes,
        ViewMode::Processes => ViewMode::Connections,
        ViewMode::Connections => ViewMode::Addresses,
        ViewMode::Addresses => ViewMode::All,
    }
}

fn next_sort_column(col: SortColumn) -> SortColumn {
    match col {
        SortColumn::Total => SortColumn::Rx,
        SortColumn::Rx => SortColumn::Tx,
        SortColumn::Tx => SortColumn::Pid,
        SortColumn::Pid => SortColumn::Name,
        SortColumn::Name => SortColumn::Total,
    }
}

fn next_unit_family(unit: UnitFamily) -> UnitFamily {
    match unit {
        UnitFamily::BinBytes => UnitFamily::BinBits,
        UnitFamily::BinBits => UnitFamily::SiBytes,
        UnitFamily::SiBytes => UnitFamily::SiBits,
        UnitFamily::SiBits => UnitFamily::BinBytes,
    }
}

fn normalized_filter(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    haystack.to_ascii_lowercase().contains(needle_lower)
}

#[allow(clippy::too_many_arguments)]
fn compare_rows(
    a_counters: TrafficCounters,
    a_pid: u32,
    a_name: &str,
    b_counters: TrafficCounters,
    b_pid: u32,
    b_name: &str,
    sort_col: SortColumn,
    sort_desc: bool,
) -> Ordering {
    let ord = match sort_col {
        SortColumn::Total => a_counters
            .total_bytes()
            .cmp(&b_counters.total_bytes())
            .then_with(|| a_name.cmp(b_name))
            .then_with(|| a_pid.cmp(&b_pid)),
        SortColumn::Rx => a_counters
            .rx_bytes
            .cmp(&b_counters.rx_bytes)
            .then_with(|| a_counters.total_bytes().cmp(&b_counters.total_bytes())),
        SortColumn::Tx => a_counters
            .tx_bytes
            .cmp(&b_counters.tx_bytes)
            .then_with(|| a_counters.total_bytes().cmp(&b_counters.total_bytes())),
        SortColumn::Pid => a_pid.cmp(&b_pid).then_with(|| a_name.cmp(b_name)),
        SortColumn::Name => a_name
            .to_ascii_lowercase()
            .cmp(&b_name.to_ascii_lowercase())
            .then_with(|| a_pid.cmp(&b_pid)),
    };

    if sort_desc { ord.reverse() } else { ord }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use clap::Parser;

    use super::*;
    use crate::model::{ConnectionKey, LocalSocket, ProcessInfo, Protocol, RemoteSocket};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn sample_snapshot() -> NetSnapshot {
        let ip1 = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        let ip2 = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let local = LocalSocket {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            port: 50000,
        };
        NetSnapshot {
            interval_secs: 1.0,
            status: CaptureStatus::Live,
            total: TrafficCounters {
                rx_bytes: 3000,
                tx_bytes: 1000,
                rx_packets: 10,
                tx_packets: 5,
            },
            interfaces: Vec::new(),
            processes: vec![
                ProcessTraffic {
                    process: ProcessInfo {
                        pid: 100,
                        name: "curl".to_string(),
                        cmdline: "curl https://example.com".to_string(),
                        uid: 1000,
                    },
                    counters: TrafficCounters {
                        rx_bytes: 2000,
                        tx_bytes: 500,
                        rx_packets: 6,
                        tx_packets: 2,
                    },
                    connection_count: 1,
                },
                ProcessTraffic {
                    process: ProcessInfo {
                        pid: 200,
                        name: "firefox".to_string(),
                        cmdline: "firefox".to_string(),
                        uid: 1000,
                    },
                    counters: TrafficCounters {
                        rx_bytes: 1000,
                        tx_bytes: 500,
                        rx_packets: 4,
                        tx_packets: 3,
                    },
                    connection_count: 1,
                },
            ],
            connections: vec![
                ConnectionTraffic {
                    key: ConnectionKey {
                        protocol: Protocol::Tcp,
                        local,
                        remote: RemoteSocket { ip: ip1, port: 443 },
                    },
                    process: Some(ProcessInfo {
                        pid: 100,
                        name: "curl".to_string(),
                        cmdline: "curl".to_string(),
                        uid: 1000,
                    }),
                    hostname: Some("example.com".to_string()),
                    counters: TrafficCounters {
                        rx_bytes: 2000,
                        tx_bytes: 500,
                        rx_packets: 6,
                        tx_packets: 2,
                    },
                },
                ConnectionTraffic {
                    key: ConnectionKey {
                        protocol: Protocol::Udp,
                        local,
                        remote: RemoteSocket { ip: ip2, port: 53 },
                    },
                    process: Some(ProcessInfo {
                        pid: 200,
                        name: "firefox".to_string(),
                        cmdline: "firefox".to_string(),
                        uid: 1000,
                    }),
                    hostname: Some("one.one.one.one".to_string()),
                    counters: TrafficCounters {
                        rx_bytes: 1000,
                        tx_bytes: 500,
                        rx_packets: 4,
                        tx_packets: 3,
                    },
                },
            ],
            remotes: vec![
                RemoteTraffic {
                    ip: ip1,
                    hostname: Some("example.com".to_string()),
                    counters: TrafficCounters {
                        rx_bytes: 2000,
                        tx_bytes: 500,
                        rx_packets: 6,
                        tx_packets: 2,
                    },
                    connection_count: 1,
                },
                RemoteTraffic {
                    ip: ip2,
                    hostname: Some("one.one.one.one".to_string()),
                    counters: TrafficCounters {
                        rx_bytes: 1000,
                        tx_bytes: 500,
                        rx_packets: 4,
                        tx_packets: 3,
                    },
                    connection_count: 1,
                },
            ],
        }
    }

    #[test]
    fn from_cli_initializes_state() {
        let args = CliArgs::try_parse_from([
            "rsnet",
            "-p",
            "-s",
            "rx",
            "-f",
            "curl",
            "-t",
            "-u",
            "si-bits",
            "-n",
            "--no-color",
        ])
        .expect("cli args should parse");
        let app = AppState::from_cli(&args);
        assert_eq!(app.view_mode, ViewMode::Processes);
        assert_eq!(app.sort_column, SortColumn::Rx);
        assert!(app.sort_desc);
        assert_eq!(app.filter_text, "curl");
        assert!(app.cumulative);
        assert_eq!(app.unit_family, UnitFamily::SiBits);
        assert!(app.no_resolve);
        assert!(app.no_color);
    }

    #[test]
    fn quit_keybindings_set_should_quit() {
        for quit_key in [
            key(KeyCode::Char('q')),
            key(KeyCode::F(10)),
            ctrl_key(KeyCode::Char('c')),
        ] {
            let mut app = AppState::default();
            assert!(app.handle_key_event(quit_key));
            assert!(app.should_quit);
        }
    }

    #[test]
    fn view_mode_switching_via_tab_and_digits() {
        let mut app = AppState::default();
        assert_eq!(app.view_mode, ViewMode::All);

        assert!(!app.handle_key_event(key(KeyCode::Tab)));
        assert_eq!(app.view_mode, ViewMode::Processes);
        assert!(!app.handle_key_event(key(KeyCode::Tab)));
        assert_eq!(app.view_mode, ViewMode::Connections);
        assert!(!app.handle_key_event(key(KeyCode::Tab)));
        assert_eq!(app.view_mode, ViewMode::Addresses);
        assert!(!app.handle_key_event(key(KeyCode::Tab)));
        assert_eq!(app.view_mode, ViewMode::All);

        app.handle_key_event(key(KeyCode::Char('2')));
        assert_eq!(app.view_mode, ViewMode::Connections);
        app.handle_key_event(key(KeyCode::Char('3')));
        assert_eq!(app.view_mode, ViewMode::Addresses);
        app.handle_key_event(key(KeyCode::Char('1')));
        assert_eq!(app.view_mode, ViewMode::Processes);
        app.handle_key_event(key(KeyCode::Char('0')));
        assert_eq!(app.view_mode, ViewMode::All);
    }

    #[test]
    fn navigation_clamps_to_row_bounds() {
        let mut app = AppState::default();
        app.update_snapshot(sample_snapshot());
        assert_eq!(app.selected_row, 0);

        app.handle_key_event(key(KeyCode::Char('j')));
        assert_eq!(app.selected_row, 1);
        app.handle_key_event(key(KeyCode::Down));
        assert_eq!(app.selected_row, 1);

        app.handle_key_event(key(KeyCode::Char('k')));
        assert_eq!(app.selected_row, 0);
        app.handle_key_event(key(KeyCode::Up));
        assert_eq!(app.selected_row, 0);

        app.adjust_scroll(1);
        app.handle_key_event(key(KeyCode::Down));
        app.adjust_scroll(1);
        assert_eq!(app.scroll_offset, 1);
    }

    #[test]
    fn sort_and_reverse_and_toggles_work() {
        let mut app = AppState::default();
        assert_eq!(app.sort_column, SortColumn::Total);
        assert!(app.sort_desc);

        app.handle_key_event(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::Rx);
        app.handle_key_event(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::Tx);
        app.handle_key_event(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::Pid);
        assert!(!app.sort_desc);
        app.handle_key_event(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::Name);
        assert!(!app.sort_desc);
        app.handle_key_event(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::Total);
        assert!(app.sort_desc);

        app.handle_key_event(key(KeyCode::Char('r')));
        assert!(!app.sort_desc);

        app.handle_key_event(key(KeyCode::Char(' ')));
        assert!(app.paused);
        app.handle_key_event(key(KeyCode::Char('p')));
        assert!(!app.paused);

        app.handle_key_event(key(KeyCode::Char('t')));
        assert!(app.cumulative);
        app.handle_key_event(key(KeyCode::Char('t')));
        assert!(!app.cumulative);

        assert_eq!(app.unit_family, UnitFamily::BinBytes);
        app.handle_key_event(key(KeyCode::Char('u')));
        assert_eq!(app.unit_family, UnitFamily::BinBits);
        app.handle_key_event(key(KeyCode::Char('u')));
        assert_eq!(app.unit_family, UnitFamily::SiBytes);
        app.handle_key_event(key(KeyCode::Char('u')));
        assert_eq!(app.unit_family, UnitFamily::SiBits);
        app.handle_key_event(key(KeyCode::Char('u')));
        assert_eq!(app.unit_family, UnitFamily::BinBytes);

        assert!(!app.no_resolve);
        app.handle_key_event(key(KeyCode::Char('n')));
        assert!(app.no_resolve);
    }

    #[test]
    fn filter_and_help_modes_work() {
        let mut app = AppState::default();
        app.update_snapshot(sample_snapshot());

        app.handle_key_event(key(KeyCode::Char('/')));
        assert_eq!(app.input_mode, InputMode::Filter);

        app.handle_key_event(key(KeyCode::Char('c')));
        app.handle_key_event(key(KeyCode::Char('u')));
        app.handle_key_event(key(KeyCode::Char('r')));
        app.handle_key_event(key(KeyCode::Char('l')));
        assert_eq!(app.filter_text, "curl");
        assert_eq!(app.visible_processes().len(), 1);

        app.handle_key_event(key(KeyCode::Backspace));
        assert_eq!(app.filter_text, "cur");
        app.handle_key_event(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);

        // Esc in normal mode clears active filter
        app.handle_key_event(key(KeyCode::Esc));
        assert_eq!(app.filter_text, "");
        assert_eq!(app.visible_processes().len(), 2);

        app.handle_key_event(key(KeyCode::Char('?')));
        assert_eq!(app.input_mode, InputMode::Help);
        app.handle_key_event(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);

        app.handle_key_event(key(KeyCode::F(1)));
        assert_eq!(app.input_mode, InputMode::Help);
        app.handle_key_event(key(KeyCode::F(1)));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn paused_ignores_snapshot_updates_and_status_ttl_expires() {
        let mut app = AppState::default();
        app.update_snapshot(sample_snapshot());
        assert_eq!(app.snapshot.processes.len(), 2);

        app.paused = true;
        let mut empty = empty_snapshot();
        empty.total.rx_bytes = 9999;
        app.update_snapshot(empty);
        assert_eq!(app.snapshot.processes.len(), 2);

        app.set_status_message("hello");
        assert!(app.status_message.is_some());
        app.clear_expired_status(Duration::from_secs(10));
        assert!(app.status_message.is_some());
        app.clear_expired_status(Duration::ZERO);
        assert!(app.status_message.is_none());
    }
}
