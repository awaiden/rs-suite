use crate::system::{ModuleEntry, ModuleId};

const NORMAL_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

const BRIGHT_NAMES: [&str; 8] = [
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// Generates the 8+8 ANSI color palette swatch string.
pub fn format_palette(no_color: bool) -> String {
    if no_color {
        return "0 1 2 3 4 5 6 7\n8 9 10 11 12 13 14 15".to_string();
    }

    let mut row1 = String::new();
    for code in 40..=47 {
        row1.push_str(&format!("\x1b[{code}m   "));
    }
    row1.push_str("\x1b[0m");

    let mut row2 = String::new();
    for code in 100..=107 {
        row2.push_str(&format!("\x1b[{code}m   "));
    }
    row2.push_str("\x1b[0m");

    format!("{row1}\n{row2}")
}

/// Collects the Colors module entry.
pub fn collect(no_color: bool) -> Option<ModuleEntry> {
    let value = format_palette(no_color);
    Some(ModuleEntry {
        id: ModuleId::Colors,
        label: ModuleId::Colors.label().to_string(),
        value,
        json_value: serde_json::json!({
            "normal": NORMAL_NAMES,
            "bright": BRIGHT_NAMES,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_ansi_and_plain_palettes() {
        let ansi = format_palette(false);
        assert!(ansi.contains("\x1b[40m"));
        assert!(ansi.contains("\x1b[47m"));
        assert!(ansi.contains("\x1b[100m"));
        assert!(ansi.contains("\x1b[107m"));
        assert!(ansi.contains("\x1b[0m"));

        let plain = format_palette(true);
        assert!(!plain.contains('\x1b'));
        assert_eq!(plain, "0 1 2 3 4 5 6 7\n8 9 10 11 12 13 14 15");
    }
}
