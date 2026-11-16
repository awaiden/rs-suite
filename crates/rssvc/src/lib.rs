//! Interactive systemd service manager and inspector with TUI.

pub mod cli;
pub mod model;
pub mod output;
pub mod systemd;
pub mod tui;

/// Returns the crate binary name.
pub fn bin_name() -> &'static str {
    "rssvc"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_name_matches_crate() {
        assert_eq!(bin_name(), "rssvc");
    }
}
