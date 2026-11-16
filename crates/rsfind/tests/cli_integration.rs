use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

use tempfile::TempDir;

fn rsfind_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rsfind")
}

fn run_rsfind<I, S>(args: I, cwd: &Path) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new(rsfind_bin())
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute rsfind binary")
}

fn stdout_lines(output: &Output) -> Vec<String> {
    assert!(
        output.status.success(),
        "rsfind failed with {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(ToString::to_string)
        .collect();
    lines.sort();
    lines
}

#[test]
fn respects_gitignore_ignore_hidden_and_unrestricted_flags() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create .git so `ignore` honors .gitignore.
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "*.log\nignored_dir/\n").unwrap();
    fs::write(root.join(".ignore"), "*.tmp\n").unwrap();

    fs::write(root.join("visible.txt"), "ok").unwrap();
    fs::write(root.join("debug.log"), "log").unwrap();
    fs::write(root.join("scratch.tmp"), "tmp").unwrap();
    fs::write(root.join(".hidden_file"), "secret").unwrap();
    fs::create_dir(root.join("ignored_dir")).unwrap();
    fs::write(root.join("ignored_dir/nested.txt"), "nested").unwrap();

    // Default: respects .gitignore, .ignore, and skips hidden files.
    let out = run_rsfind(["--no-interactive", "-t", "f"], root);
    assert_eq!(stdout_lines(&out), vec!["visible.txt"]);

    // -I (--no-ignore): includes .gitignore and .ignore matches, still skips hidden.
    let out = run_rsfind(["--no-interactive", "-I", "-t", "f"], root);
    assert_eq!(
        stdout_lines(&out),
        vec![
            "debug.log",
            "ignored_dir/nested.txt",
            "scratch.tmp",
            "visible.txt",
        ]
    );

    // -H (--hidden): includes hidden files (still respects ignore files, and .git is hidden).
    let out = run_rsfind(["--no-interactive", "-H", "-E", ".git", "-t", "f"], root);
    assert_eq!(
        stdout_lines(&out),
        vec![".gitignore", ".hidden_file", ".ignore", "visible.txt",]
    );

    // -u: equivalent to --no-ignore.
    let out = run_rsfind(["--no-interactive", "-u", "-t", "f"], root);
    assert_eq!(
        stdout_lines(&out),
        vec![
            "debug.log",
            "ignored_dir/nested.txt",
            "scratch.tmp",
            "visible.txt",
        ]
    );

    // -uu: equivalent to --no-ignore --hidden.
    let out = run_rsfind(["--no-interactive", "-uu", "-E", ".git", "-t", "f"], root);
    assert_eq!(
        stdout_lines(&out),
        vec![
            ".gitignore",
            ".hidden_file",
            ".ignore",
            "debug.log",
            "ignored_dir/nested.txt",
            "scratch.tmp",
            "visible.txt",
        ]
    );
}

#[test]
fn filters_by_type_extension_glob_regex_depth_exclude_size_and_time() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("sub/deep")).unwrap();
    fs::write(root.join("alpha.rs"), "fn main() {}").unwrap();
    fs::write(root.join("Beta.txt"), "hello world!!").unwrap();
    fs::write(root.join("empty.txt"), "").unwrap();
    fs::write(root.join("sub/lib.rs"), "pub fn x() {}").unwrap();
    fs::write(root.join("sub/deep/far.rs"), "pub fn y() {}").unwrap();

    #[cfg(unix)]
    {
        let script = root.join("run.sh");
        fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();

        symlink(root.join("alpha.rs"), root.join("link_alpha")).unwrap();
    }

    // -t f (files)
    let files = stdout_lines(&run_rsfind(["--no-interactive", "-t", "f"], root));
    assert!(files.contains(&"alpha.rs".to_string()));
    assert!(files.contains(&"empty.txt".to_string()));
    assert!(!files.contains(&"sub".to_string()));

    // -t d (directories)
    let dirs = stdout_lines(&run_rsfind(["--no-interactive", "-t", "d"], root));
    assert_eq!(dirs, vec!["sub", "sub/deep"]);

    // -t e (empty files/dirs)
    let empty = stdout_lines(&run_rsfind(["--no-interactive", "-t", "e"], root));
    assert_eq!(empty, vec!["empty.txt"]);

    #[cfg(unix)]
    {
        // -t l (symlink)
        let links = stdout_lines(&run_rsfind(["--no-interactive", "-t", "l"], root));
        assert_eq!(links, vec!["link_alpha"]);

        // -t x (executable)
        let execs = stdout_lines(&run_rsfind(["--no-interactive", "-t", "x"], root));
        assert_eq!(execs, vec!["run.sh"]);
    }

    // -e extension
    let rs_files = stdout_lines(&run_rsfind(["--no-interactive", "-e", "rs"], root));
    assert_eq!(rs_files, vec!["alpha.rs", "sub/deep/far.rs", "sub/lib.rs"]);

    // -g glob
    let glob_files = stdout_lines(&run_rsfind(["--no-interactive", "-g", "*.txt"], root));
    assert_eq!(glob_files, vec!["Beta.txt", "empty.txt"]);

    // Regex (default) + smart-case: lowercase matches case-insensitively, uppercase case-sensitively.
    let regex_ci = stdout_lines(&run_rsfind(["--no-interactive", "beta"], root));
    assert_eq!(regex_ci, vec!["Beta.txt"]);
    let regex_cs = stdout_lines(&run_rsfind(["--no-interactive", "Beta"], root));
    assert_eq!(regex_cs, vec!["Beta.txt"]);
    let regex_cs_miss = stdout_lines(&run_rsfind(["--no-interactive", "BETA"], root));
    assert!(regex_cs_miss.is_empty());

    // -d max-depth
    let depth2 = stdout_lines(&run_rsfind(
        ["--no-interactive", "-d", "2", "-e", "rs"],
        root,
    ));
    assert_eq!(depth2, vec!["alpha.rs", "sub/lib.rs"]);

    // -E exclude
    let excluded = stdout_lines(&run_rsfind(
        ["--no-interactive", "-e", "rs", "-E", "deep"],
        root,
    ));
    assert_eq!(excluded, vec!["alpha.rs", "sub/lib.rs"]);

    // -S size
    let big = stdout_lines(&run_rsfind(
        ["--no-interactive", "-t", "f", "-S", "+13B"],
        root,
    ));
    assert!(big.contains(&"Beta.txt".to_string()));
    assert!(!big.contains(&"empty.txt".to_string()));

    let tiny = stdout_lines(&run_rsfind(
        ["--no-interactive", "-t", "f", "-S", "-0B"],
        root,
    ));
    assert_eq!(tiny, vec!["empty.txt"]);

    // --changed-within (all newly created files are within 1h)
    let recent = stdout_lines(&run_rsfind(
        [
            "--no-interactive",
            "-t",
            "f",
            "--changed-within",
            "1h",
            "-e",
            "rs",
        ],
        root,
    ));
    assert_eq!(recent, vec!["alpha.rs", "sub/deep/far.rs", "sub/lib.rs"]);
}

#[test]
fn output_formatting_print0_absolute_and_max_results() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::write(root.join("one.txt"), "1").unwrap();
    fs::write(root.join("two.txt"), "2").unwrap();
    fs::write(root.join("three.txt"), "3").unwrap();

    // -0 (--print0)
    let out0 = run_rsfind(["--no-interactive", "-0", "-t", "f"], root);
    assert!(out0.status.success());
    let mut parts: Vec<String> = out0
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    parts.sort();
    assert_eq!(parts, vec!["one.txt", "three.txt", "two.txt"]);

    // -a (--absolute-path)
    let abs_lines = stdout_lines(&run_rsfind(["--no-interactive", "-a", "-t", "f"], root));
    assert_eq!(abs_lines.len(), 3);
    for line in &abs_lines {
        assert!(
            Path::new(line).is_absolute(),
            "expected absolute path, got {line}"
        );
    }

    // -1 (--max-results)
    let max1 = stdout_lines(&run_rsfind(
        ["--no-interactive", "-1", "1", "-t", "f"],
        root,
    ));
    assert_eq!(max1.len(), 1);
}

#[test]
fn exec_and_exec_batch_with_placeholders() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/alpha.txt"), "a").unwrap();
    fs::write(root.join("sub/beta.txt"), "b").unwrap();

    // -x with {} and {/.}
    let marker_dir = TempDir::new().unwrap();
    let out_x = run_rsfind(
        [
            "-e",
            "txt",
            "-x",
            "sh",
            "-c",
            "printf '%s:%s\\n' \"$1\" \"$2\" > \"$3/$2.out\"",
            "_",
            "{}",
            "{/.}",
            marker_dir.path().to_str().unwrap(),
        ],
        root,
    );
    assert!(
        out_x.status.success(),
        "exec failed: {}",
        String::from_utf8_lossy(&out_x.stderr)
    );
    let alpha_out = fs::read_to_string(marker_dir.path().join("alpha.out")).unwrap();
    assert_eq!(alpha_out.trim(), "./sub/alpha.txt:alpha");
    let beta_out = fs::read_to_string(marker_dir.path().join("beta.out")).unwrap();
    assert_eq!(beta_out.trim(), "./sub/beta.txt:beta");

    // -X (--exec-batch) with {/.}
    let batch_file = marker_dir.path().join("batch.txt");
    let out_batch = run_rsfind(
        [
            "-e",
            "txt",
            "-X",
            "sh",
            "-c",
            "printf '%s\\n' \"$@\" > \"$0\"",
            batch_file.to_str().unwrap(),
            "{/.}",
        ],
        root,
    );
    assert!(
        out_batch.status.success(),
        "exec-batch failed: {}",
        String::from_utf8_lossy(&out_batch.stderr)
    );
    let mut batch_lines: Vec<String> = fs::read_to_string(&batch_file)
        .unwrap()
        .lines()
        .map(ToString::to_string)
        .collect();
    batch_lines.sort();
    assert_eq!(batch_lines, vec!["alpha", "beta"]);
}

#[test]
fn conflicting_flags_exit_with_error() {
    let tmp = TempDir::new().unwrap();
    let out = run_rsfind(["--no-interactive", "-g", "-F", "foo"], tmp.path());
    assert_eq!(out.status.code(), Some(1));
}
