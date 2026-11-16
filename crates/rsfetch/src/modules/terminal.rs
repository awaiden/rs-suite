use std::collections::HashSet;
use std::fs;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

const KNOWN_TERMINALS: &[(&str, &str)] = &[
    ("kitty", "kitty"),
    ("alacritty", "Alacritty"),
    ("wezterm-gui", "WezTerm"),
    ("wezterm", "WezTerm"),
    ("foot", "foot"),
    ("ghostty", "Ghostty"),
    ("konsole", "Konsole"),
    ("gnome-terminal-server", "GNOME Terminal"),
    ("gnome-terminal", "GNOME Terminal"),
    ("xterm", "xterm"),
    ("urxvt", "urxvt"),
    ("st", "st"),
    ("tmux: server", "tmux"),
    ("tmux", "tmux"),
    ("screen", "screen"),
];

/// Parses `(ppid, comm)` from a `/proc/<pid>/stat` line.
///
/// Handles `comm` containing spaces or nested parentheses by locating the last `')'`.
pub fn parse_proc_stat(stat: &str) -> Option<(i32, String)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    if close <= open {
        return None;
    }
    let comm = stat[open + 1..close].to_string();
    let rest = stat[close + 1..].trim_start();
    // Fields after ')': 0=state, 1=ppid
    let ppid_str = rest.split_whitespace().nth(1)?;
    let ppid: i32 = ppid_str.parse().ok()?;
    Some((ppid, comm))
}

/// Matches a process `comm` name against known terminal emulators.
pub fn match_known_terminal(comm: &str) -> Option<&'static str> {
    let clean = comm.trim();
    let lower = clean.to_lowercase();
    KNOWN_TERMINALS
        .iter()
        .find(|(pat, _)| lower == *pat || lower.starts_with(&format!("{pat}:")))
        .map(|(_, display)| *display)
}

fn walk_proc_for_terminal() -> Option<String> {
    let mut pid = unsafe { libc::getppid() };
    let mut visited = HashSet::new();

    for _ in 0..32 {
        if pid <= 1 || !visited.insert(pid) {
            break;
        }
        let stat_path = format!("/proc/{pid}/stat");
        let Ok(stat) = fs::read_to_string(stat_path) else {
            break;
        };
        let Some((ppid, comm)) = parse_proc_stat(&stat) else {
            break;
        };
        if let Some(known) = match_known_terminal(&comm) {
            return Some(known.to_string());
        }
        pid = ppid;
    }
    None
}

/// Collects the Terminal module entry.
pub fn collect() -> Option<ModuleEntry> {
    let value = std::env::var("TERM_PROGRAM")
        .ok()
        .map(|s| sanitize_control_chars(s.trim()))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("TERMINAL")
                .ok()
                .map(|s| sanitize_control_chars(s.trim()))
                .filter(|s| !s.is_empty())
        })
        .or_else(walk_proc_for_terminal)
        .or_else(|| {
            std::env::var("TERM")
                .ok()
                .map(|s| sanitize_control_chars(s.trim()))
                .filter(|s| !s.is_empty())
        })?;

    Some(ModuleEntry {
        id: ModuleId::Terminal,
        label: ModuleId::Terminal.label().to_string(),
        value: value.clone(),
        json_value: serde_json::json!({
            "name": value,
            "term": std::env::var("TERM").ok().map(|s| sanitize_control_chars(&s)),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_with_spaces_and_parens() {
        let line = "1234 (tmux: server) S 1000 1234 1234 0 -1 4194304 100 0 0 0";
        assert_eq!(
            parse_proc_stat(line),
            Some((1000, "tmux: server".to_string()))
        );

        let weird = "555 (my (weird) proc) R 42 555 555 0 -1 0";
        assert_eq!(
            parse_proc_stat(weird),
            Some((42, "my (weird) proc".to_string()))
        );
    }

    #[test]
    fn matches_known_terminals() {
        assert_eq!(match_known_terminal("kitty"), Some("kitty"));
        assert_eq!(match_known_terminal("wezterm-gui"), Some("WezTerm"));
        assert_eq!(match_known_terminal("tmux: server"), Some("tmux"));
        assert_eq!(match_known_terminal("bash"), None);
    }
}
