use rs_common::sanitize_control_chars;

use rsfetch::cli::LogoType;
use rsfetch::logo::{DistroLogo, render_logo_lines, visible_width};
use rsfetch::system::{ModuleId, SystemReport};

const DEFAULT_ACCENT: &str = "\x1b[1;36m";
const ANSI_RESET: &str = "\x1b[0m";

/// Renders the system report as terminal text, optionally side-by-side with an ASCII logo.
pub fn render_text(
    report: &SystemReport,
    logo: Option<&DistroLogo>,
    logo_type: LogoType,
    no_color: bool,
) -> String {
    let accent = logo.map_or(DEFAULT_ACCENT, |l| l.accent_color);
    let info_lines = build_info_lines(report, accent, no_color);

    let Some(logo) = logo.filter(|_| logo_type != LogoType::None) else {
        return info_lines.join("\n");
    };

    let (logo_lines, logo_width) = render_logo_lines(logo, logo_type, no_color);
    if logo_lines.is_empty() {
        return info_lines.join("\n");
    }

    let pad_target = logo_width + 3;
    let total_rows = logo_lines.len().max(info_lines.len());
    let mut out = Vec::with_capacity(total_rows);

    for i in 0..total_rows {
        let left = logo_lines.get(i).map_or("", String::as_str);
        if let Some(right) = info_lines.get(i) {
            let spaces = pad_target.saturating_sub(visible_width(left));
            out.push(format!("{left}{}{right}", " ".repeat(spaces)));
        } else {
            out.push(left.to_string());
        }
    }

    out.join("\n")
}

fn build_info_lines(report: &SystemReport, accent: &str, no_color: bool) -> Vec<String> {
    let user = sanitize_control_chars(&report.user);
    let hostname = sanitize_control_chars(&report.hostname);
    let header_len = user.chars().count() + 1 + hostname.chars().count();

    let mut lines = Vec::with_capacity(report.entries.len() + 5);
    if no_color {
        lines.push(format!("{user}@{hostname}"));
    } else {
        lines.push(format!(
            "{accent}{user}{ANSI_RESET}@{accent}{hostname}{ANSI_RESET}"
        ));
    }
    lines.push("-".repeat(header_len));

    for entry in &report.entries {
        if entry.id == ModuleId::Colors {
            if !no_color {
                lines.push(String::new());
                lines.extend(entry.value.lines().map(str::to_string));
            }
            continue;
        }

        let label = sanitize_control_chars(&entry.label);
        let value = sanitize_control_chars(&entry.value);
        if no_color {
            lines.push(format!("{label}: {value}"));
        } else {
            lines.push(format!("{accent}{label}{ANSI_RESET}: {value}"));
        }
    }

    lines
}

/// Renders the system report and selected logo metadata as pretty-printed JSON.
pub fn render_json(report: &SystemReport, logo: Option<&DistroLogo>) -> String {
    let mut modules = serde_json::Map::with_capacity(report.entries.len());
    for entry in &report.entries {
        modules.insert(entry.id.as_str().to_string(), entry.json_value.clone());
    }

    let payload = serde_json::json!({
        "user": report.user,
        "hostname": report.hostname,
        "distro": report.distro_id,
        "logo": logo.map(|l| l.name),
        "modules": modules,
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsfetch::system::ModuleEntry;

    fn sample_logo() -> DistroLogo {
        DistroLogo {
            name: "test",
            aliases: &[],
            lines: &["$1/\\", "$1\\/", "||"],
            small_lines: &["$1/\\"],
            colors: &["\x1b[1;34m"],
            accent_color: "\x1b[1;34m",
        }
    }

    fn sample_report() -> SystemReport {
        SystemReport {
            user: "alice".to_string(),
            hostname: "workstation".to_string(),
            distro_id: "arch".to_string(),
            distro_id_like: vec![],
            entries: vec![
                ModuleEntry {
                    id: ModuleId::Os,
                    label: "OS".to_string(),
                    value: "Arch Linux".to_string(),
                    json_value: serde_json::json!("Arch Linux"),
                },
                ModuleEntry {
                    id: ModuleId::Kernel,
                    label: "Kernel".to_string(),
                    value: "6.12.0".to_string(),
                    json_value: serde_json::json!("6.12.0"),
                },
                ModuleEntry {
                    id: ModuleId::Colors,
                    label: "Colors".to_string(),
                    value: "\x1b[40m   \x1b[0m\n\x1b[100m   \x1b[0m".to_string(),
                    json_value: serde_json::json!({"normal": ["black"], "bright": ["bright_black"]}),
                },
            ],
        }
    }

    #[test]
    fn render_text_without_logo_and_no_color_is_clean() {
        let report = sample_report();
        let rendered = render_text(&report, None, LogoType::Auto, true);

        assert!(!rendered.contains('\x1b'));
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines[0], "alice@workstation");
        assert_eq!(lines[1], "-----------------");
        assert_eq!(lines[2], "OS: Arch Linux");
        assert_eq!(lines[3], "Kernel: 6.12.0");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn render_text_handles_info_longer_than_logo() {
        let logo = sample_logo();
        let report = sample_report();
        let rendered = render_text(&report, Some(&logo), LogoType::Auto, true);
        let lines: Vec<&str> = rendered.lines().collect();

        // logo width is 2, so padding is 2 + 3 = 5 columns
        assert_eq!(lines[0], "/\\   alice@workstation");
        assert_eq!(lines[1], "\\/   -----------------");
        assert_eq!(lines[2], "||   OS: Arch Linux");
        assert_eq!(lines[3], "     Kernel: 6.12.0");
    }

    #[test]
    fn render_text_handles_logo_longer_than_info() {
        let logo = DistroLogo {
            name: "tall",
            aliases: &[],
            lines: &["AA", "BB", "CC", "DD", "EE"],
            small_lines: &[],
            colors: &[],
            accent_color: "\x1b[1;32m",
        };
        let report = SystemReport {
            user: "u".to_string(),
            hostname: "h".to_string(),
            distro_id: "linux".to_string(),
            distro_id_like: vec![],
            entries: vec![ModuleEntry {
                id: ModuleId::Os,
                label: "OS".to_string(),
                value: "Linux".to_string(),
                json_value: serde_json::json!("Linux"),
            }],
        };

        let rendered = render_text(&report, Some(&logo), LogoType::Auto, true);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "AA   u@h");
        assert_eq!(lines[1], "BB   ---");
        assert_eq!(lines[2], "CC   OS: Linux");
        assert_eq!(lines[3], "DD");
        assert_eq!(lines[4], "EE");
    }

    #[test]
    fn render_text_includes_colors_and_palette_when_color_enabled() {
        let logo = sample_logo();
        let report = sample_report();
        let rendered = render_text(&report, Some(&logo), LogoType::Auto, false);

        assert!(rendered.contains("\x1b[1;34malice\x1b[0m@\x1b[1;34mworkstation\x1b[0m"));
        assert!(rendered.contains("\x1b[1;34mOS\x1b[0m: Arch Linux"));
        assert!(rendered.contains("\x1b[40m   \x1b[0m"));
        assert!(rendered.contains("\x1b[100m   \x1b[0m"));
    }

    #[test]
    fn render_text_sanitizes_control_chars_in_report_fields() {
        let mut report = sample_report();
        report.user = "bob\x1b[31m".to_string();
        report.entries[0].value = "Arch\x07Linux".to_string();

        let rendered = render_text(&report, None, LogoType::None, true);
        assert!(!rendered.contains('\x1b'));
        assert!(!rendered.contains('\x07'));
        assert!(rendered.contains("bob [31m@workstation"));
        assert!(rendered.contains("OS: Arch Linux"));
    }

    #[test]
    fn render_json_outputs_expected_structure() {
        let logo = sample_logo();
        let report = sample_report();
        let json_str = render_json(&report, Some(&logo));

        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("render_json must produce valid JSON");
        assert_eq!(parsed["user"], "alice");
        assert_eq!(parsed["hostname"], "workstation");
        assert_eq!(parsed["distro"], "arch");
        assert_eq!(parsed["logo"], "test");
        assert_eq!(parsed["modules"]["os"], "Arch Linux");
        assert_eq!(parsed["modules"]["kernel"], "6.12.0");
        assert!(parsed["modules"]["colors"].is_object());
    }
}
