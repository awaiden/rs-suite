use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::cli::CliArgs;
use crate::model::{
    ListenMode, SocketFilter, SocketRecord, SocketState, SortColumn, filter_and_sort,
};

/// Active input mode of the `rssock` TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Filter,
    Help,
    Detail,
}

/// Interactive TUI state for filtering, sorting, and navigating sockets.
#[derive(Debug, Clone)]
pub struct AppState {
    pub all_records: Vec<SocketRecord>,
    pub filtered_records: Vec<SocketRecord>,
    pub show_tcp: bool,
    pub show_udp: bool,
    pub show_unix: bool,
    pub listen_mode: ListenMode,
    pub state_filter: Option<SocketState>,
    pub numeric: bool,
    pub sort_column: SortColumn,
    pub reverse: bool,
    pub filter_query: String,
    pub input_mode: InputMode,
    pub selected: usize,
    pub paused: bool,
    pub no_color: bool,
    pub should_quit: bool,
    pub page_size: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            all_records: Vec::new(),
            filtered_records: Vec::new(),
            show_tcp: true,
            show_udp: true,
            show_unix: true,
            listen_mode: ListenMode::All,
            state_filter: None,
            numeric: false,
            sort_column: SortColumn::Proto,
            reverse: false,
            filter_query: String::new(),
            input_mode: InputMode::Normal,
            selected: 0,
            paused: false,
            no_color: false,
            should_quit: false,
            page_size: 20,
        }
    }
}

impl AppState {
    /// Initializes TUI state from parsed CLI arguments.
    ///
    /// In TUI mode, `listen_mode` defaults to [`ListenMode::All`] unless `-l`
    /// is passed without `-a`.
    #[must_use]
    pub fn from_cli(args: &CliArgs) -> Self {
        let any_proto = args.tcp || args.udp || args.unix;
        let listen_mode = if args.listening && !args.all {
            ListenMode::Listening
        } else {
            ListenMode::All
        };

        Self {
            show_tcp: !any_proto || args.tcp,
            show_udp: !any_proto || args.udp,
            show_unix: !any_proto || args.unix,
            listen_mode,
            state_filter: args.state,
            numeric: args.numeric,
            sort_column: args.sort.into(),
            reverse: args.reverse,
            filter_query: args.filter.clone().unwrap_or_default(),
            no_color: args.no_color,
            ..Self::default()
        }
    }

    /// Replaces the raw socket snapshot and recomputes filtered/sorted rows.
    pub fn update_records(&mut self, records: Vec<SocketRecord>) {
        self.all_records = records;
        self.recompute_filtered();
    }

    /// Reapplies active protocol, listen-mode, state, query, and sort settings.
    pub fn recompute_filtered(&mut self) {
        let query = (!self.filter_query.trim().is_empty()).then(|| self.filter_query.clone());
        let filter = SocketFilter {
            tcp: self.show_tcp,
            udp: self.show_udp,
            unix: self.show_unix,
            listen_mode: self.listen_mode,
            state: self.state_filter,
            query,
        };
        self.filtered_records =
            filter_and_sort(&self.all_records, &filter, self.sort_column, self.reverse);
        self.clamp_selected();
    }

    /// Clamps `selected` to the bounds of `filtered_records`.
    pub fn clamp_selected(&mut self) {
        if self.filtered_records.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.filtered_records.len() - 1);
        }
    }

    /// Returns the currently selected socket record, if any.
    #[must_use]
    pub fn selected_record(&self) -> Option<&SocketRecord> {
        self.filtered_records.get(self.selected)
    }

    /// Cycles to the next sort column.
    #[must_use]
    pub const fn next_sort_column(col: SortColumn) -> SortColumn {
        match col {
            SortColumn::Proto => SortColumn::State,
            SortColumn::State => SortColumn::RecvQ,
            SortColumn::RecvQ => SortColumn::SendQ,
            SortColumn::SendQ => SortColumn::Local,
            SortColumn::Local => SortColumn::Remote,
            SortColumn::Remote => SortColumn::Pid,
            SortColumn::Pid => SortColumn::Process,
            SortColumn::Process => SortColumn::Proto,
        }
    }

    fn move_down(&mut self, delta: usize) {
        if self.filtered_records.is_empty() {
            self.selected = 0;
            return;
        }
        let max_idx = self.filtered_records.len() - 1;
        self.selected = self.selected.saturating_add(delta).min(max_idx);
    }

    fn move_up(&mut self, delta: usize) {
        self.selected = self.selected.saturating_sub(delta);
    }

    /// Processes a terminal key event and returns `true` if the application should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.should_quit = true;
            return true;
        }

        match self.input_mode {
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::Help => self.handle_help_key(key),
            InputMode::Detail => self.handle_detail_key(key),
            InputMode::Normal => self.handle_normal_key(key),
        }

        self.should_quit
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.filter_query.pop();
                self.recompute_filtered();
            }
            KeyCode::F(10) => {
                self.should_quit = true;
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.filter_query.push(c);
                self.recompute_filtered();
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::F(1) => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('q') | KeyCode::F(10) => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.input_mode = InputMode::Help;
            }
            KeyCode::Char('q') | KeyCode::F(10) => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => {
                self.should_quit = true;
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_down(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(1),
            KeyCode::PageDown => self.move_down(self.page_size.max(1)),
            KeyCode::PageUp => self.move_up(self.page_size.max(1)),
            KeyCode::Char('g') | KeyCode::Home => {
                self.selected = 0;
            }
            KeyCode::Char('G') | KeyCode::End => {
                if !self.filtered_records.is_empty() {
                    self.selected = self.filtered_records.len() - 1;
                }
            }
            KeyCode::Char('t') => {
                self.show_tcp = !self.show_tcp;
                self.recompute_filtered();
            }
            KeyCode::Char('u') => {
                self.show_udp = !self.show_udp;
                self.recompute_filtered();
            }
            KeyCode::Char('x') => {
                self.show_unix = !self.show_unix;
                self.recompute_filtered();
            }
            KeyCode::Char('l') => {
                self.listen_mode = if self.listen_mode == ListenMode::Listening {
                    ListenMode::All
                } else {
                    ListenMode::Listening
                };
                self.recompute_filtered();
            }
            KeyCode::Char('a') => {
                self.listen_mode = if self.listen_mode == ListenMode::All {
                    ListenMode::NonListening
                } else {
                    ListenMode::All
                };
                self.recompute_filtered();
            }
            KeyCode::Char('s') => {
                self.sort_column = Self::next_sort_column(self.sort_column);
                self.recompute_filtered();
            }
            KeyCode::Char('r') => {
                self.reverse = !self.reverse;
                self.recompute_filtered();
            }
            KeyCode::Char('n') => {
                self.numeric = !self.numeric;
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
            }
            KeyCode::Enter => {
                if !self.filtered_records.is_empty() {
                    self.input_mode = InputMode::Detail;
                }
            }
            KeyCode::Char(' ' | 'p') => {
                self.paused = !self.paused;
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.input_mode = InputMode::Help;
            }
            KeyCode::Esc if !self.filter_query.is_empty() => {
                self.filter_query.clear();
                self.recompute_filtered();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use clap::Parser;

    use super::*;
    use crate::model::{Endpoint, ProcessInfo, Protocol, SocketKind, UnixSocketType};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn inet_record(
        protocol: Protocol,
        state: SocketState,
        local_port: u16,
        remote_port: u16,
        recv_q: u32,
        inode: u64,
        proc_info: Option<(u32, &str)>,
    ) -> SocketRecord {
        SocketRecord {
            protocol,
            state,
            recv_q,
            send_q: 0,
            kind: SocketKind::Inet {
                local: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                    port: local_port,
                },
                remote: Endpoint {
                    ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    port: remote_port,
                },
            },
            uid: 1000,
            inode,
            process: proc_info.map(|(pid, name)| ProcessInfo {
                pid,
                name: name.to_string(),
                cmdline: format!("/usr/bin/{name}"),
                uid: 1000,
            }),
        }
    }

    fn unix_record(inode: u64, path: Option<&str>, state: SocketState) -> SocketRecord {
        SocketRecord {
            protocol: Protocol::Unix,
            state,
            recv_q: 0,
            send_q: 0,
            kind: SocketKind::Unix {
                unix_type: UnixSocketType::Stream,
                flags: 0,
                ref_count: 2,
                path: path.map(str::to_string),
            },
            uid: 0,
            inode,
            process: None,
        }
    }

    fn sample_records() -> Vec<SocketRecord> {
        vec![
            inet_record(
                Protocol::Tcp,
                SocketState::Listen,
                22,
                0,
                0,
                100,
                Some((500, "sshd")),
            ),
            inet_record(
                Protocol::Tcp,
                SocketState::Established,
                443,
                51234,
                128,
                200,
                Some((1200, "nginx")),
            ),
            inet_record(
                Protocol::Udp,
                SocketState::Unconn,
                5353,
                0,
                64,
                300,
                Some((900, "avahi-daemon")),
            ),
            unix_record(
                400,
                Some("/run/dbus/system_bus_socket"),
                SocketState::Listen,
            ),
        ]
    }

    #[test]
    fn from_cli_defaults_to_all_listen_mode_in_tui() {
        let args = CliArgs::try_parse_from(["rssock", "--tui"]).unwrap();
        let app = AppState::from_cli(&args);
        assert_eq!(app.listen_mode, ListenMode::All);
        assert!(app.show_tcp && app.show_udp && app.show_unix);

        let listen_args = CliArgs::try_parse_from(["rssock", "--tui", "-l"]).unwrap();
        let listen_app = AppState::from_cli(&listen_args);
        assert_eq!(listen_app.listen_mode, ListenMode::Listening);
    }

    #[test]
    fn protocol_and_listen_mode_toggles_filter_records() {
        let mut app = AppState::default();
        app.update_records(sample_records());
        assert_eq!(app.filtered_records.len(), 4);

        app.handle_key(key(KeyCode::Char('t')));
        assert!(!app.show_tcp);
        assert_eq!(app.filtered_records.len(), 2);

        app.handle_key(key(KeyCode::Char('u')));
        assert!(!app.show_udp);
        assert_eq!(app.filtered_records.len(), 1);

        app.handle_key(key(KeyCode::Char('t')));
        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(app.filtered_records.len(), 4);

        // Toggle Listening
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.listen_mode, ListenMode::Listening);
        assert_eq!(app.filtered_records.len(), 2);

        // Toggle back to All
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.listen_mode, ListenMode::All);
        assert_eq!(app.filtered_records.len(), 4);

        // Toggle NonListening via 'a'
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(app.listen_mode, ListenMode::NonListening);
        assert_eq!(app.filtered_records.len(), 2);
    }

    #[test]
    fn sorting_cycles_and_reverses() {
        let mut app = AppState::default();
        app.update_records(sample_records());
        assert_eq!(app.sort_column, SortColumn::Proto);

        // Cycle Proto -> State -> RecvQ
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::State);
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.sort_column, SortColumn::RecvQ);

        // Lowest recv_q first
        assert_eq!(app.filtered_records[0].recv_q, 0);

        // Reverse -> highest recv_q (128) first
        app.handle_key(key(KeyCode::Char('r')));
        assert!(app.reverse);
        assert_eq!(app.filtered_records[0].recv_q, 128);
    }

    #[test]
    fn navigation_keys_move_and_clamp_selection() {
        let mut app = AppState {
            page_size: 2,
            ..AppState::default()
        };
        app.update_records(sample_records());
        assert_eq!(app.selected, 0);

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);

        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.selected, 3);

        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected, 3);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected, 2);

        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.selected, 0);

        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.selected, 3);

        app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn filter_mode_typing_and_esc_clearing() {
        let mut app = AppState::default();
        app.update_records(sample_records());

        app.handle_key(key(KeyCode::Char('/')));
        assert_eq!(app.input_mode, InputMode::Filter);

        for c in "nginx".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.filter_query, "nginx");
        assert_eq!(app.filtered_records.len(), 1);
        assert_eq!(
            app.filtered_records[0].process.as_ref().unwrap().name,
            "nginx"
        );

        // Backspace narrows query
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.filter_query, "ngin");

        // Enter confirms filter and returns to Normal
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.filtered_records.len(), 1);

        // Esc in Normal mode clears active filter
        app.handle_key(key(KeyCode::Esc));
        assert!(app.filter_query.is_empty());
        assert_eq!(app.filtered_records.len(), 4);
    }

    #[test]
    fn overlays_pause_numeric_and_quit_keybindings() {
        let mut app = AppState::default();
        app.update_records(sample_records());

        // Help overlay
        app.handle_key(key(KeyCode::Char('?')));
        assert_eq!(app.input_mode, InputMode::Help);
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);

        // Detail popup
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Detail);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);

        // Pause & numeric
        app.handle_key(key(KeyCode::Char(' ')));
        assert!(app.paused);
        app.handle_key(key(KeyCode::Char('p')));
        assert!(!app.paused);
        app.handle_key(key(KeyCode::Char('n')));
        assert!(app.numeric);

        // Ctrl+C quits
        let quit = app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(quit);
        assert!(app.should_quit);
    }
}
