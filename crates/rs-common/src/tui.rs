use ratatui::style::{Color, Modifier, Style};

/// Color and style palette supporting both color and `--no-color` modes.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub no_color: bool,
    pub text: Style,
    pub muted: Style,
    pub title: Style,
    pub border: Style,
    pub accent: Style,
    pub table_header: Style,
    pub table_header_active: Style,
    pub selected_row: Style,
    pub status_bar: Style,
    pub status_alert: Style,
    pub proc_running: Style,
    pub proc_zombie: Style,
    pub usage_low: Style,
    pub usage_mid: Style,
    pub usage_high: Style,
    pub bar_highlight: Style,
}

impl Theme {
    /// Builds a `Theme` for the requested color mode.
    pub fn new(no_color: bool) -> Self {
        if no_color {
            Self {
                no_color: true,
                text: Style::default(),
                muted: Style::default(),
                title: Style::default().add_modifier(Modifier::BOLD),
                border: Style::default(),
                accent: Style::default().add_modifier(Modifier::BOLD),
                table_header: Style::default().add_modifier(Modifier::BOLD),
                table_header_active: Style::default()
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                selected_row: Style::default().add_modifier(Modifier::REVERSED),
                status_bar: Style::default().add_modifier(Modifier::REVERSED),
                status_alert: Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                proc_running: Style::default().add_modifier(Modifier::BOLD),
                proc_zombie: Style::default().add_modifier(Modifier::DIM),
                usage_low: Style::default(),
                usage_mid: Style::default(),
                usage_high: Style::default().add_modifier(Modifier::BOLD),
                bar_highlight: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            }
        } else {
            Self {
                no_color: false,
                text: Style::default().fg(Color::White),
                muted: Style::default().fg(Color::DarkGray),
                title: Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                border: Style::default().fg(Color::DarkGray),
                accent: Style::default().fg(Color::Cyan),
                table_header: Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
                table_header_active: Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                selected_row: Style::default().bg(Color::DarkGray).fg(Color::White),
                status_bar: Style::default().bg(Color::DarkGray).fg(Color::White),
                status_alert: Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                proc_running: Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                proc_zombie: Style::default().fg(Color::Red),
                usage_low: Style::default().fg(Color::Green),
                usage_mid: Style::default().fg(Color::Yellow),
                usage_high: Style::default().fg(Color::Red),
                bar_highlight: Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            }
        }
    }

    /// Returns a threshold-colored style for percentage usage bars (0..=100).
    pub fn usage_style(&self, pct: f64) -> Style {
        if pct >= 85.0 {
            self.usage_high
        } else if pct >= 50.0 {
            self.usage_mid
        } else {
            self.usage_low
        }
    }

    /// Returns `Style::default()` when `no_color` is enabled, otherwise `s`.
    pub fn style_or_default(&self, s: Style) -> Style {
        if self.no_color { Style::default() } else { s }
    }

    /// Returns the header/footer bar style.
    pub fn bar_style(&self) -> Style {
        self.bar_highlight
    }
}

/// Returns the spinner frame for the given elapsed milliseconds in Braille or ASCII mode.
pub fn spinner_char(no_unicode: bool, elapsed_millis: u128) -> char {
    let frames: &[char] = if no_unicode {
        &['|', '/', '-', '\\']
    } else {
        &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
    };
    frames[(elapsed_millis / 100) as usize % frames.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_color_vs_no_color() {
        let color = Theme::new(false);
        assert!(!color.no_color);
        assert_eq!(color.text, Style::default().fg(Color::White));
        assert_eq!(color.usage_low, Style::default().fg(Color::Green));
        assert_eq!(color.usage_mid, Style::default().fg(Color::Yellow));
        assert_eq!(color.usage_high, Style::default().fg(Color::Red));
        assert_eq!(
            color.bar_style(),
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        );

        let plain = Theme::new(true);
        assert!(plain.no_color);
        assert_eq!(plain.text, Style::default());
        assert_eq!(plain.usage_low, Style::default());
        assert_eq!(plain.usage_mid, Style::default());
        assert_eq!(
            plain.usage_high,
            Style::default().add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            plain.bar_style(),
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        );
    }

    #[test]
    fn usage_style_thresholds() {
        let theme = Theme::new(false);
        assert_eq!(theme.usage_style(0.0), theme.usage_low);
        assert_eq!(theme.usage_style(49.9), theme.usage_low);
        assert_eq!(theme.usage_style(50.0), theme.usage_mid);
        assert_eq!(theme.usage_style(84.9), theme.usage_mid);
        assert_eq!(theme.usage_style(85.0), theme.usage_high);
        assert_eq!(theme.usage_style(100.0), theme.usage_high);
    }

    #[test]
    fn style_or_default_respects_no_color() {
        let sample = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        assert_eq!(Theme::new(false).style_or_default(sample), sample);
        assert_eq!(Theme::new(true).style_or_default(sample), Style::default());
    }

    #[test]
    fn spinner_frames_ascii_and_unicode() {
        assert_eq!(spinner_char(true, 0), '|');
        assert_eq!(spinner_char(true, 100), '/');
        assert_eq!(spinner_char(true, 200), '-');
        assert_eq!(spinner_char(true, 300), '\\');
        assert_eq!(spinner_char(true, 400), '|');

        assert_eq!(spinner_char(false, 0), '⠋');
        assert_eq!(spinner_char(false, 100), '⠙');
        assert_eq!(spinner_char(false, 900), '⠏');
        assert_eq!(spinner_char(false, 1000), '⠋');
    }
}
