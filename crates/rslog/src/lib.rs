//! Interactive Linux log and systemd journald viewer with filtering and TUI.

pub mod cli;
pub mod model;
pub mod output;
pub mod source;
pub mod tui;

/// Returns the crate binary name.
pub fn bin_name() -> &'static str {
    "rslog"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_name_matches_crate() {
        assert_eq!(bin_name(), "rslog");
    }
}
