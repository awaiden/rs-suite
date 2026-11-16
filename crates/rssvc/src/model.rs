use std::fmt;

use rs_common::sanitize_control_chars;
use serde_json::{Value, json};

/// Coarse state category for styling and status grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitStateCategory {
    Active,
    Failed,
    Transitioning,
    Inactive,
}

/// Metadata for a single systemd unit returned by `systemctl list-units`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInfo {
    pub unit: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

impl UnitInfo {
    /// Returns the unit type suffix (for example, `"service"` from `"sshd.service"`).
    pub fn unit_type(&self) -> &str {
        self.unit.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
    }

    /// Classifies the unit's active state into a display category.
    pub fn state_category(&self) -> UnitStateCategory {
        let active = self.active.trim().to_ascii_lowercase();
        let sub = self.sub.trim().to_ascii_lowercase();
        if active == "failed" || sub == "failed" {
            UnitStateCategory::Failed
        } else if matches!(active.as_str(), "activating" | "deactivating" | "reloading") {
            UnitStateCategory::Transitioning
        } else if active == "active" {
            UnitStateCategory::Active
        } else {
            UnitStateCategory::Inactive
        }
    }

    /// Returns `true` if this unit matches the optional type, state, and substring filters.
    pub fn matches_filter(
        &self,
        unit_type: Option<&str>,
        state: Option<&str>,
        query: Option<&str>,
    ) -> bool {
        if let Some(ty) = unit_type.map(str::trim).filter(|s| !s.is_empty()) {
            let expected = ty.trim_start_matches('.');
            if !self.unit_type().eq_ignore_ascii_case(expected) {
                return false;
            }
        }

        if let Some(st) = state.map(str::trim).filter(|s| !s.is_empty())
            && !self.active.eq_ignore_ascii_case(st)
            && !self.sub.eq_ignore_ascii_case(st)
            && !self.load.eq_ignore_ascii_case(st)
        {
            return false;
        }

        if let Some(q) = query.map(str::trim).filter(|s| !s.is_empty()) {
            let needle = q.to_ascii_lowercase();
            let hit = self.unit.to_ascii_lowercase().contains(&needle)
                || self.description.to_ascii_lowercase().contains(&needle)
                || self.active.to_ascii_lowercase().contains(&needle)
                || self.sub.to_ascii_lowercase().contains(&needle);
            if !hit {
                return false;
            }
        }

        true
    }

    /// Converts this unit into a sanitized `serde_json::Value`.
    pub fn to_json_value(&self) -> Value {
        json!({
            "unit": sanitize_control_chars(&self.unit),
            "load": sanitize_control_chars(&self.load),
            "active": sanitize_control_chars(&self.active),
            "sub": sanitize_control_chars(&self.sub),
            "description": sanitize_control_chars(&self.description),
        })
    }
}

/// Mutating systemd unit actions supported by `rssvc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Reload,
    Enable,
    Disable,
}

impl ServiceAction {
    /// Returns the `systemctl` subcommand name.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reload => "reload",
            Self::Enable => "enable",
            Self::Disable => "disable",
        }
    }

    /// Returns a capitalized human-readable label.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
            Self::Reload => "Reload",
            Self::Enable => "Enable",
            Self::Disable => "Disable",
        }
    }
}

impl fmt::Display for ServiceAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parses `systemctl list-units --output=json` output into [`UnitInfo`] records.
pub fn parse_units_json(input: &str) -> Result<Vec<UnitInfo>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let value: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid systemctl JSON: {e}"))?;
    let items = value
        .as_array()
        .ok_or_else(|| "expected JSON array from systemctl list-units".to_string())?;

    let mut units = Vec::with_capacity(items.len());
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let unit = obj
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if unit.is_empty() {
            continue;
        }
        units.push(UnitInfo {
            unit,
            load: json_str(obj.get("load")),
            active: json_str(obj.get("active")),
            sub: json_str(obj.get("sub")),
            description: json_str(obj.get("description")),
        });
    }
    Ok(units)
}

fn json_str(val: Option<&Value>) -> String {
    val.and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Parses plain tabular `systemctl list-units` output (e.g. `--plain --no-legend`).
pub fn parse_units_tabular(input: &str) -> Vec<UnitInfo> {
    let mut units = Vec::new();
    for raw_line in input.lines() {
        let line = raw_line
            .trim_start()
            .trim_start_matches(['●', '*', '×'])
            .trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("UNIT ")
            || line.starts_with("LOAD ")
            || line.starts_with("ACTIVE ")
            || line.starts_with("SUB ")
            || line.ends_with("loaded units listed.")
            || line.starts_with("To show all installed")
        {
            continue;
        }

        if let Some(info) = parse_tabular_line(line) {
            units.push(info);
        }
    }
    units
}

fn parse_tabular_line(line: &str) -> Option<UnitInfo> {
    let mut rest = line.trim_start();
    let mut cols = [""; 4];
    for col in &mut cols {
        let end = rest.find(char::is_whitespace)?;
        *col = &rest[..end];
        rest = rest[end..].trim_start();
    }
    if cols[0].is_empty() {
        return None;
    }
    Some(UnitInfo {
        unit: cols[0].to_string(),
        load: cols[1].to_string(),
        active: cols[2].to_string(),
        sub: cols[3].to_string(),
        description: rest.trim().to_string(),
    })
}

/// Parses `systemctl list-units` output, trying JSON first and falling back to plain tabular text.
pub fn parse_units(input: &str) -> Vec<UnitInfo> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('[')
        && let Ok(parsed) = parse_units_json(trimmed)
    {
        return parsed;
    }
    parse_units_tabular(input)
}

/// Filters a slice of [`UnitInfo`] by optional unit type, state, and substring query.
pub fn filter_units(
    units: &[UnitInfo],
    unit_type: Option<&str>,
    state: Option<&str>,
    query: Option<&str>,
) -> Vec<UnitInfo> {
    units
        .iter()
        .filter(|u| u.matches_filter(unit_type, state, query))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_units() -> Vec<UnitInfo> {
        vec![
            UnitInfo {
                unit: "sshd.service".into(),
                load: "loaded".into(),
                active: "active".into(),
                sub: "running".into(),
                description: "OpenSSH Daemon".into(),
            },
            UnitInfo {
                unit: "nginx.service".into(),
                load: "loaded".into(),
                active: "failed".into(),
                sub: "failed".into(),
                description: "A high performance web server".into(),
            },
            UnitInfo {
                unit: "fstrim.timer".into(),
                load: "loaded".into(),
                active: "inactive".into(),
                sub: "dead".into(),
                description: "Discard unused blocks once a week".into(),
            },
        ]
    }

    #[test]
    fn parses_json_unit_list() {
        let json = r#"[
            {
                "unit": "sshd.service",
                "load": "loaded",
                "active": "active",
                "sub": "running",
                "description": "OpenSSH Daemon"
            },
            {
                "unit": "docker.socket",
                "load": "loaded",
                "active": "active",
                "sub": "listening",
                "description": null
            }
        ]"#;

        let units = parse_units(json);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].unit, "sshd.service");
        assert_eq!(units[0].description, "OpenSSH Daemon");
        assert_eq!(units[1].unit, "docker.socket");
        assert_eq!(units[1].sub, "listening");
        assert_eq!(units[1].description, "");
    }

    #[test]
    fn parses_tabular_unit_list_with_bullets_and_legends() {
        let raw = "\
UNIT                         LOAD   ACTIVE SUB     DESCRIPTION
  sshd.service               loaded active running OpenSSH Daemon
● nginx.service              loaded failed failed  A high performance web server
  fstrim.timer               loaded inactive dead  Discard unused blocks once a week

LOAD   = Reflects whether the unit definition was properly loaded.
3 loaded units listed.
";

        let units = parse_units(raw);
        assert_eq!(units, sample_units());
    }

    #[test]
    fn filters_by_type_state_and_query() {
        let units = sample_units();

        let services = filter_units(&units, Some("service"), None, None);
        assert_eq!(services.len(), 2);

        let failed = filter_units(&units, None, Some("failed"), None);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].unit, "nginx.service");

        let sub_match = filter_units(&units, None, Some("dead"), None);
        assert_eq!(sub_match.len(), 1);
        assert_eq!(sub_match[0].unit, "fstrim.timer");

        let query_match = filter_units(&units, None, None, Some("openssh"));
        assert_eq!(query_match.len(), 1);
        assert_eq!(query_match[0].unit, "sshd.service");
    }

    #[test]
    fn state_category_maps_expected_variants() {
        let units = sample_units();
        assert_eq!(units[0].state_category(), UnitStateCategory::Active);
        assert_eq!(units[1].state_category(), UnitStateCategory::Failed);
        assert_eq!(units[2].state_category(), UnitStateCategory::Inactive);

        let reloading = UnitInfo {
            unit: "foo.service".into(),
            load: "loaded".into(),
            active: "reloading".into(),
            sub: "reload".into(),
            description: String::new(),
        };
        assert_eq!(reloading.state_category(), UnitStateCategory::Transitioning);
    }

    #[test]
    fn service_action_strings() {
        assert_eq!(ServiceAction::Start.as_str(), "start");
        assert_eq!(ServiceAction::Stop.as_str(), "stop");
        assert_eq!(ServiceAction::Restart.as_str(), "restart");
        assert_eq!(ServiceAction::Reload.as_str(), "reload");
        assert_eq!(ServiceAction::Enable.as_str(), "enable");
        assert_eq!(ServiceAction::Disable.as_str(), "disable");
    }
}
