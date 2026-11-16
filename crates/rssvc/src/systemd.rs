use std::process::Command;

use crate::model::{ServiceAction, UnitInfo, parse_units, parse_units_json, parse_units_tabular};

/// Result of executing (or previewing in dry-run mode) a systemd action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub executed: bool,
    pub command: String,
    pub output: String,
}

/// Wrapper around `systemctl` and `journalctl` supporting `--user` and `--dry-run`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemdClient {
    pub user: bool,
    pub dry_run: bool,
}

impl SystemdClient {
    /// Creates a new [`SystemdClient`].
    pub const fn new(user: bool, dry_run: bool) -> Self {
        Self { user, dry_run }
    }

    /// Lists loaded systemd units, trying JSON output first and falling back to plain tabular.
    pub fn list_units(
        &self,
        all: bool,
        unit_type: Option<&str>,
        state: Option<&str>,
    ) -> Result<Vec<UnitInfo>, String> {
        let json_args = list_units_args(self.user, all, unit_type, state, true);
        if let Ok(out) = run_command("systemctl", &json_args)
            && out.status.success()
        {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(units) = parse_units_json(&stdout) {
                return Ok(units);
            }
            let fallback = parse_units(&stdout);
            if !fallback.is_empty() {
                return Ok(fallback);
            }
        }

        let plain_args = list_units_args(self.user, all, unit_type, state, false);
        let out = run_command("systemctl", &plain_args)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("systemctl list-units failed with {}", out.status)
            } else {
                stderr
            });
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(parse_units_tabular(&stdout))
    }

    /// Fetches `systemctl status <unit>` output.
    ///
    /// Note that `systemctl status` returns non-zero exit codes (for example, 3) for inactive
    /// units while still writing valid status details to stdout.
    pub fn unit_status(&self, unit: &str) -> Result<String, String> {
        let args = status_args(self.user, unit);
        let out = run_command("systemctl", &args)?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stdout.trim().is_empty() {
            Ok(stdout.into_owned())
        } else if !stderr.trim().is_empty() {
            if out.status.success() {
                Ok(stderr.into_owned())
            } else {
                Err(stderr.trim().to_string())
            }
        } else if out.status.success() {
            Ok(String::new())
        } else {
            Err(format!(
                "systemctl status {unit} failed with {}",
                out.status
            ))
        }
    }

    /// Fetches recent journald log lines for `unit` via `journalctl -u <unit>`.
    pub fn unit_logs(&self, unit: &str, lines: usize) -> Result<Vec<String>, String> {
        let args = journal_args(self.user, unit, lines);
        let out = run_command("journalctl", &args)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("journalctl -u {unit} failed with {}", out.status)
            } else {
                stderr
            });
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(stdout.lines().map(ToString::to_string).collect())
    }

    /// Executes (or previews when `self.dry_run` is `true`) a [`ServiceAction`] on `unit`.
    pub fn perform_action(
        &self,
        action: ServiceAction,
        unit: &str,
    ) -> Result<ActionOutcome, String> {
        let command = format_action_command(self.user, action, unit);
        if self.dry_run {
            return Ok(ActionOutcome {
                executed: false,
                output: format!("[dry-run] {command}"),
                command,
            });
        }

        let args = action_args(self.user, action, unit);
        let out = run_command("systemctl", &args)?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();

        if !out.status.success() {
            let msg = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("command exited with {}", out.status)
            };
            return Err(format!("{command}: {msg}"));
        }

        let output = if !stdout.is_empty() {
            stdout
        } else if !stderr.is_empty() {
            stderr
        } else {
            format!("ok: {command}")
        };

        Ok(ActionOutcome {
            executed: true,
            command,
            output,
        })
    }
}

/// Builds `systemctl list-units` arguments.
pub fn list_units_args(
    user: bool,
    all: bool,
    unit_type: Option<&str>,
    state: Option<&str>,
    json: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if user {
        args.push("--user".to_string());
    }
    args.push("list-units".to_string());
    args.push("--no-pager".to_string());
    if json {
        args.push("--output=json".to_string());
    } else {
        args.push("--plain".to_string());
        args.push("--no-legend".to_string());
    }
    if all {
        args.push("--all".to_string());
    }
    if let Some(ty) = unit_type.map(str::trim).filter(|s| !s.is_empty()) {
        args.push(format!("--type={ty}"));
    }
    if let Some(st) = state.map(str::trim).filter(|s| !s.is_empty()) {
        args.push(format!("--state={st}"));
    }
    args
}

/// Builds `systemctl status` arguments.
pub fn status_args(user: bool, unit: &str) -> Vec<String> {
    let mut args = Vec::new();
    if user {
        args.push("--user".to_string());
    }
    args.push("status".to_string());
    args.push(unit.to_string());
    args.push("--no-pager".to_string());
    args.push("-l".to_string());
    args
}

/// Builds `systemctl <action> <unit>` arguments.
pub fn action_args(user: bool, action: ServiceAction, unit: &str) -> Vec<String> {
    let mut args = Vec::new();
    if user {
        args.push("--user".to_string());
    }
    args.push(action.as_str().to_string());
    args.push(unit.to_string());
    args
}

/// Formats the exact `systemctl` command string for a [`ServiceAction`].
pub fn format_action_command(user: bool, action: ServiceAction, unit: &str) -> String {
    let args = action_args(user, action, unit);
    format!("systemctl {}", args.join(" "))
}

/// Builds `journalctl` arguments for reading recent unit logs.
pub fn journal_args(user: bool, unit: &str, lines: usize) -> Vec<String> {
    let mut args = Vec::new();
    if user {
        args.push("--user".to_string());
    }
    args.push("-u".to_string());
    args.push(unit.to_string());
    args.push("-n".to_string());
    args.push(lines.to_string());
    args.push("--no-pager".to_string());
    args
}

fn run_command(program: &str, args: &[String]) -> Result<std::process::Output, String> {
    Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to execute {program}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_list_units_args_for_system_and_user() {
        let sys_args = list_units_args(false, false, Some("service"), Some("failed"), true);
        assert_eq!(
            sys_args,
            vec![
                "list-units",
                "--no-pager",
                "--output=json",
                "--type=service",
                "--state=failed",
            ]
        );

        let user_args = list_units_args(true, true, None, None, false);
        assert_eq!(
            user_args,
            vec![
                "--user",
                "list-units",
                "--no-pager",
                "--plain",
                "--no-legend",
                "--all",
            ]
        );
    }

    #[test]
    fn builds_journal_and_status_args() {
        assert_eq!(
            status_args(true, "pipewire.service"),
            vec!["--user", "status", "pipewire.service", "--no-pager", "-l"]
        );
        assert_eq!(
            journal_args(false, "sshd.service", 50),
            vec!["-u", "sshd.service", "-n", "50", "--no-pager"]
        );
    }

    #[test]
    fn formats_action_commands_and_respects_dry_run() {
        assert_eq!(
            format_action_command(false, ServiceAction::Restart, "nginx.service"),
            "systemctl restart nginx.service"
        );
        assert_eq!(
            format_action_command(true, ServiceAction::Enable, "syncthing.service"),
            "systemctl --user enable syncthing.service"
        );

        let client = SystemdClient::new(true, true);
        let outcome = client
            .perform_action(ServiceAction::Stop, "nonexistent.service")
            .expect("dry-run should succeed without invoking systemctl");

        assert!(!outcome.executed);
        assert_eq!(outcome.command, "systemctl --user stop nonexistent.service");
        assert_eq!(
            outcome.output,
            "[dry-run] systemctl --user stop nonexistent.service"
        );
    }
}
