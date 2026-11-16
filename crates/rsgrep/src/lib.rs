//! Fast parallel content search with interactive TUI.

pub mod cli;
pub mod output;
pub mod search;
pub mod tui;

/// Returns the crate binary name.
pub fn bin_name() -> &'static str {
    "rsgrep"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_name_matches_crate() {
        assert_eq!(bin_name(), "rsgrep");
    }
}
