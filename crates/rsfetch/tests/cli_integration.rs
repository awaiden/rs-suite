use std::process::Command;

fn rsfetch_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rsfetch"))
}

#[test]
fn help_and_version_exit_zero() {
    let help = rsfetch_cmd()
        .arg("--help")
        .output()
        .expect("failed to execute --help");
    assert!(help.status.success(), "--help should exit 0");
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("rsfetch"));
    assert!(help_stdout.contains("--json"));

    let version = rsfetch_cmd()
        .arg("--version")
        .output()
        .expect("failed to execute --version");
    assert!(version.status.success(), "--version should exit 0");
    let version_stdout = String::from_utf8_lossy(&version.stdout);
    assert!(version_stdout.contains("rsfetch"));
}

#[test]
fn list_logos_and_list_modules_output_expected_entries() {
    let logos = rsfetch_cmd()
        .arg("--list-logos")
        .output()
        .expect("failed to execute --list-logos");
    assert!(logos.status.success());
    let logos_stdout = String::from_utf8_lossy(&logos.stdout);
    let logo_lines: Vec<&str> = logos_stdout.lines().collect();
    for expected in ["linux", "arch", "ubuntu", "debian", "fedora", "nixos"] {
        assert!(
            logo_lines.contains(&expected),
            "--list-logos missing '{expected}'"
        );
    }

    let modules = rsfetch_cmd()
        .arg("--list-modules")
        .output()
        .expect("failed to execute --list-modules");
    assert!(modules.status.success());
    let modules_stdout = String::from_utf8_lossy(&modules.stdout);
    let module_lines: Vec<&str> = modules_stdout.lines().collect();
    for expected in [
        "os", "host", "kernel", "uptime", "cpu", "memory", "disk", "colors",
    ] {
        assert!(
            module_lines.contains(&expected),
            "--list-modules missing '{expected}'"
        );
    }
}

#[test]
fn json_outputs_valid_json_with_expected_keys() {
    let output = rsfetch_cmd()
        .args(["--json", "--modules", "os,kernel,uptime"])
        .output()
        .expect("failed to execute --json");
    assert!(output.status.success());

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json output must be valid JSON");
    assert!(parsed.get("user").and_then(|v| v.as_str()).is_some());
    assert!(parsed.get("hostname").and_then(|v| v.as_str()).is_some());
    assert!(parsed.get("distro").and_then(|v| v.as_str()).is_some());

    let modules = parsed
        .get("modules")
        .and_then(|v| v.as_object())
        .expect("modules should be a JSON object");
    assert!(modules.contains_key("kernel"));
    assert!(modules.contains_key("uptime"));
}

#[test]
fn no_logo_no_color_modules_produces_clean_output() {
    let output = rsfetch_cmd()
        .args(["--no-logo", "--no-color", "--modules", "os,kernel,uptime"])
        .output()
        .expect("failed to execute with --no-logo --no-color");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\x1b'),
        "output with --no-color must not contain ANSI escape sequences"
    );
    assert!(stdout.contains("OS:"));
    assert!(stdout.contains("Kernel:"));
    assert!(stdout.contains("Uptime:"));
}

#[test]
fn logo_arch_no_color_renders_arch_art_alongside_info() {
    let output = rsfetch_cmd()
        .args([
            "--logo",
            "arch",
            "--no-color",
            "--modules",
            "os,kernel,uptime",
        ])
        .output()
        .expect("failed to execute with --logo arch --no-color");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains('\x1b'));
    assert!(stdout.contains(".o+`"));
    assert!(stdout.contains("osssssso"));
    assert!(stdout.contains("Kernel:"));
}

#[test]
fn env_no_color_disables_ansi_sequences() {
    let output = rsfetch_cmd()
        .env("NO_COLOR", "1")
        .args(["--logo", "arch", "--modules", "kernel,colors"])
        .output()
        .expect("failed to execute with NO_COLOR=1");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\x1b'),
        "NO_COLOR=1 must suppress all ANSI escape sequences"
    );
    assert!(stdout.contains("Kernel:"));
}
