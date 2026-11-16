use std::fmt::Write as _;

use rs_common::sanitize_control_chars;

use crate::model::{UnitInfo, UnitStateCategory};

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_DIM: &str = "\x1b[2m";

/// Renders `units` as an aligned `systemctl`-style table for `--list` CLI mode.
pub fn render_table(units: &[UnitInfo], no_color: bool) -> String {
    let sanitized: Vec<UnitInfo> = units
        .iter()
        .map(|u| UnitInfo {
            unit: sanitize_control_chars(&u.unit),
            load: sanitize_control_chars(&u.load),
            active: sanitize_control_chars(&u.active),
            sub: sanitize_control_chars(&u.sub),
            description: sanitize_control_chars(&u.description),
        })
        .collect();

    let mut w_unit = "UNIT".len();
    let mut w_load = "LOAD".len();
    let mut w_active = "ACTIVE".len();
    let mut w_sub = "SUB".len();

    for u in &sanitized {
        w_unit = w_unit.max(u.unit.len());
        w_load = w_load.max(u.load.len());
        w_active = w_active.max(u.active.len());
        w_sub = w_sub.max(u.sub.len());
    }

    let mut out = String::new();
    let header = format!(
        "{:<w_unit$}  {:<w_load$}  {:<w_active$}  {:<w_sub$}  DESCRIPTION",
        "UNIT", "LOAD", "ACTIVE", "SUB"
    );
    if no_color {
        let _ = writeln!(out, "{header}");
    } else {
        let _ = writeln!(out, "{ANSI_BOLD}{header}{ANSI_RESET}");
    }

    for u in &sanitized {
        let active_padded = format!("{:<w_active$}", u.active);
        let sub_padded = format!("{:<w_sub$}", u.sub);

        let (active_col, sub_col) = if no_color {
            (active_padded, sub_padded)
        } else {
            let color = state_ansi_color(u.state_category());
            (
                format!("{color}{active_padded}{ANSI_RESET}"),
                format!("{color}{sub_padded}{ANSI_RESET}"),
            )
        };

        let _ = writeln!(
            out,
            "{:<w_unit$}  {:<w_load$}  {}  {}  {}",
            u.unit, u.load, active_col, sub_col, u.description
        );
    }

    let noun = if sanitized.len() == 1 {
        "unit"
    } else {
        "units"
    };
    let _ = writeln!(out, "{} {noun} listed.", sanitized.len());
    out
}

fn state_ansi_color(cat: UnitStateCategory) -> &'static str {
    match cat {
        UnitStateCategory::Active => ANSI_GREEN,
        UnitStateCategory::Failed => ANSI_RED,
        UnitStateCategory::Transitioning => ANSI_YELLOW,
        UnitStateCategory::Inactive => ANSI_DIM,
    }
}

/// Renders `units` as a pretty-printed JSON array for `--json` CLI mode.
pub fn render_json(units: &[UnitInfo]) -> Result<String, String> {
    let values: Vec<_> = units.iter().map(UnitInfo::to_json_value).collect();
    serde_json::to_string_pretty(&values).map_err(|e| format!("failed to serialize JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<UnitInfo> {
        vec![
            UnitInfo {
                unit: "sshd.service".into(),
                load: "loaded".into(),
                active: "active".into(),
                sub: "running".into(),
                description: "OpenSSH\x1b[31m Daemon".into(),
            },
            UnitInfo {
                unit: "nginx.service".into(),
                load: "loaded".into(),
                active: "failed".into(),
                sub: "failed".into(),
                description: "Web server".into(),
            },
        ]
    }

    #[test]
    fn render_table_no_color_sanitizes_control_chars() {
        let out = render_table(&sample(), true);
        assert!(!out.contains('\x1b'));
        assert!(out.contains("UNIT"));
        assert!(out.contains("sshd.service"));
        assert!(out.contains("OpenSSH [31m Daemon"));
        assert!(out.contains("2 units listed."));
    }

    #[test]
    fn render_table_with_color_includes_ansi_sequences() {
        let out = render_table(&sample(), false);
        assert!(out.contains(ANSI_GREEN));
        assert!(out.contains(ANSI_RED));
    }

    #[test]
    fn render_json_produces_valid_array() {
        let json = render_json(&sample()).expect("should serialize JSON");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let arr = parsed.as_array().expect("JSON array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["unit"], "sshd.service");
        assert_eq!(arr[1]["active"], "failed");
    }
}
