use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};

use rayon::prelude::*;

use crate::walk::FoundEntry;

const PLACEHOLDERS: [&str; 5] = ["{}", "{/}", "{//}", "{.}", "{/.}"];

/// Return true if `arg` contains any `-x`/`-X` placeholder token.
pub fn has_placeholder(arg: &str) -> bool {
    PLACEHOLDERS.iter().any(|p| arg.contains(p))
}

/// Expand all placeholder tokens in `arg` using `path`.
pub fn expand_arg(arg: &str, path: &Path) -> String {
    if !has_placeholder(arg) {
        return arg.to_string();
    }

    let full = path.to_string_lossy();
    let basename = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_else(|| full.clone());
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy(),
        _ => std::borrow::Cow::Borrowed("."),
    };
    let without_ext = path.with_extension("");
    let without_ext_str = without_ext.to_string_lossy();
    let basename_without_ext = Path::new(basename.as_ref())
        .with_extension("")
        .to_string_lossy()
        .into_owned();

    arg.replace("{//}", &parent)
        .replace("{/.}", &basename_without_ext)
        .replace("{/}", &basename)
        .replace("{.}", &without_ext_str)
        .replace("{}", &full)
}

/// Expand a command template for a single entry (`-x/--exec`).
///
/// If no placeholder is present in `template`, the entry's path is appended.
pub fn expand_single(template: &[String], path: &Path) -> Vec<String> {
    let any_placeholder = template.iter().any(|a| has_placeholder(a));
    let mut out: Vec<String> = template.iter().map(|a| expand_arg(a, path)).collect();
    if !any_placeholder {
        out.push(path.to_string_lossy().into_owned());
    }
    out
}

/// Expand a command template for a batch of entries (`-X/--exec-batch`).
///
/// Arguments containing placeholders are repeated once per entry; if no placeholder
/// is present in `template`, all entry paths are appended at the end.
pub fn expand_batch(template: &[String], entries: &[FoundEntry]) -> Vec<String> {
    let any_placeholder = template.iter().any(|a| has_placeholder(a));
    let mut out = Vec::with_capacity(template.len() + entries.len());

    for arg in template {
        if has_placeholder(arg) {
            for entry in entries {
                out.push(expand_arg(arg, &entry.path));
            }
        } else {
            out.push(arg.clone());
        }
    }

    if !any_placeholder {
        for entry in entries {
            out.push(entry.path.to_string_lossy().into_owned());
        }
    }

    out
}

/// Execute `template` in parallel across `entries` (`-x/--exec`), returning `0` if all
/// invocations succeed or a non-zero exit code if any child fails.
pub fn exec_parallel(template: &[String], entries: &[FoundEntry]) -> Result<i32, String> {
    if template.is_empty() {
        return Err("empty command for -x/--exec".to_string());
    }
    if entries.is_empty() {
        return Ok(0);
    }

    let exit_code = AtomicI32::new(0);

    entries.par_iter().for_each(|entry| {
        let cmd_args = expand_single(template, &entry.path);
        match Command::new(&cmd_args[0]).args(&cmd_args[1..]).status() {
            Ok(status) => {
                if !status.success() {
                    let code = status.code().unwrap_or(1);
                    let _ =
                        exit_code.compare_exchange(0, code, Ordering::Relaxed, Ordering::Relaxed);
                }
            }
            Err(err) => {
                eprintln!("rsfind: failed to execute '{}': {err}", cmd_args[0]);
                let _ = exit_code.compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
            }
        }
    });

    Ok(exit_code.load(Ordering::Relaxed))
}

/// Execute `template` once with all `entries` (`-X/--exec-batch`).
pub fn exec_batch(template: &[String], entries: &[FoundEntry]) -> Result<i32, String> {
    if template.is_empty() {
        return Err("empty command for -X/--exec-batch".to_string());
    }
    if entries.is_empty() {
        return Ok(0);
    }

    let cmd_args = expand_batch(template, entries);
    let status = Command::new(&cmd_args[0])
        .args(&cmd_args[1..])
        .status()
        .map_err(|e| format!("failed to execute '{}': {e}", cmd_args[0]))?;

    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(p: &str) -> FoundEntry {
        FoundEntry {
            path: PathBuf::from(p),
            display: p.to_string(),
            is_dir: false,
            is_symlink: false,
            is_exec: false,
        }
    }

    #[test]
    fn placeholder_expansion_all_variants() {
        let p = Path::new("src/nested/main.rs");
        assert_eq!(expand_arg("{}", p), "src/nested/main.rs");
        assert_eq!(expand_arg("{/}", p), "main.rs");
        assert_eq!(expand_arg("{//}", p), "src/nested");
        assert_eq!(expand_arg("{.}", p), "src/nested/main");
        assert_eq!(expand_arg("{/.}", p), "main");
        assert_eq!(expand_arg("{}.bak", p), "src/nested/main.rs.bak");
    }

    #[test]
    fn placeholder_expansion_edge_cases() {
        let bare = Path::new("README.md");
        assert_eq!(expand_arg("{//}", bare), ".");
        assert_eq!(expand_arg("{/}", bare), "README.md");
        assert_eq!(expand_arg("{/.}", bare), "README");
        assert_eq!(expand_arg("{.}", bare), "README");

        let root_child = Path::new("/etc/hosts");
        assert_eq!(expand_arg("{//}", root_child), "/etc");
        assert_eq!(expand_arg("{/.}", root_child), "hosts");
    }

    #[test]
    fn expand_single_appends_path_when_no_placeholder() {
        let tpl = vec!["wc".to_string(), "-l".to_string()];
        assert_eq!(
            expand_single(&tpl, Path::new("a/b.txt")),
            vec!["wc", "-l", "a/b.txt"]
        );

        let tpl_with = vec!["cp".to_string(), "{}".to_string(), "{}.bak".to_string()];
        assert_eq!(
            expand_single(&tpl_with, Path::new("a/b.txt")),
            vec!["cp", "a/b.txt", "a/b.txt.bak"]
        );
    }

    #[test]
    fn expand_batch_handles_with_and_without_placeholders() {
        let entries = vec![entry("a.rs"), entry("dir/b.rs")];

        let no_ph = vec!["rm".to_string(), "-f".to_string()];
        assert_eq!(
            expand_batch(&no_ph, &entries),
            vec!["rm", "-f", "a.rs", "dir/b.rs"]
        );

        let with_ph = vec!["echo".to_string(), "{/.}".to_string(), "done".to_string()];
        assert_eq!(
            expand_batch(&with_ph, &entries),
            vec!["echo", "a", "b", "done"]
        );
    }

    #[test]
    fn exec_parallel_and_batch_exit_codes() {
        let entries = vec![entry("a"), entry("b")];
        let ok_tpl = vec!["true".to_string()];
        assert_eq!(exec_parallel(&ok_tpl, &entries).unwrap(), 0);
        assert_eq!(exec_batch(&ok_tpl, &entries).unwrap(), 0);

        let fail_tpl = vec!["false".to_string()];
        assert_ne!(exec_parallel(&fail_tpl, &entries).unwrap(), 0);
        assert_ne!(exec_batch(&fail_tpl, &entries).unwrap(), 0);

        assert_eq!(exec_parallel(&fail_tpl, &[]).unwrap(), 0);
        assert_eq!(exec_batch(&fail_tpl, &[]).unwrap(), 0);
    }
}
