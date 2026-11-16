use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use ignore::overrides::OverrideBuilder;
use ignore::{WalkBuilder, WalkState};

use crate::cli::Args;
use crate::filter::EntryFilter;

/// A matched filesystem entry produced by the directory walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundEntry {
    pub path: PathBuf,
    pub display: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_exec: bool,
}

/// Configuration for parallel directory traversal.
#[derive(Debug, Clone)]
pub struct WalkOpts {
    pub roots: Vec<PathBuf>,
    pub filter: EntryFilter,
    pub hidden: bool,
    pub no_ignore: bool,
    pub no_ignore_vcs: bool,
    pub follow_links: bool,
    pub max_depth: Option<usize>,
    pub min_depth: Option<usize>,
    pub same_file_system: bool,
    pub exclude: Vec<String>,
    pub threads: Option<usize>,
    pub need_metadata: bool,
    pub max_results: Option<usize>,
}

impl WalkOpts {
    /// Build `WalkOpts` from parsed CLI `Args`.
    pub fn from_args(args: &Args) -> Result<Self, String> {
        let filter = EntryFilter::from_args(args)?;
        let need_metadata = filter.needs_metadata() || args.list_details;
        Ok(Self {
            roots: args.effective_paths(),
            filter,
            hidden: args.include_hidden(),
            no_ignore: args.ignore_disabled(),
            no_ignore_vcs: args.no_ignore_vcs,
            follow_links: args.follow,
            max_depth: args.max_depth,
            min_depth: args.min_depth,
            same_file_system: args.one_file_system,
            exclude: args.exclude.clone(),
            threads: args.threads,
            need_metadata,
            max_results: args.max_results,
        })
    }
}

/// Stream matching entries over `tx` using `ignore::WalkBuilder` in parallel.
pub fn walk_stream(opts: &WalkOpts, tx: Sender<FoundEntry>) -> Result<(), String> {
    if opts.roots.is_empty() {
        return Ok(());
    }

    let mut builder = WalkBuilder::new(&opts.roots[0]);
    for root in &opts.roots[1..] {
        builder.add(root);
    }

    builder
        .threads(opts.threads.unwrap_or(0))
        .hidden(!opts.hidden)
        .follow_links(opts.follow_links)
        .max_depth(opts.max_depth)
        .min_depth(opts.min_depth)
        .same_file_system(opts.same_file_system);

    if opts.no_ignore {
        builder
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false);
    } else if opts.no_ignore_vcs {
        builder
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(true)
            .parents(true);
    } else {
        builder
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .parents(true);
    }

    if !opts.exclude.is_empty() {
        let mut overrides = OverrideBuilder::new(&opts.roots[0]);
        for pat in &opts.exclude {
            let rule = if pat.starts_with('!') {
                pat.clone()
            } else {
                format!("!{pat}")
            };
            overrides
                .add(&rule)
                .map_err(|e| format!("invalid exclude pattern '{pat}': {e}"))?;
        }
        let built = overrides
            .build()
            .map_err(|e| format!("failed to build exclude overrides: {e}"))?;
        builder.overrides(built);
    }

    let filter = Arc::new(opts.filter.clone());
    let need_metadata = opts.need_metadata;
    let include_root = opts.min_depth == Some(0);
    let max_results = opts.max_results;
    let sent_count = Arc::new(AtomicUsize::new(0));

    let walker = builder.build_parallel();
    walker.run(|| {
        let tx = tx.clone();
        let filter = Arc::clone(&filter);
        let sent_count = Arc::clone(&sent_count);

        Box::new(move |result| {
            let entry = match result {
                Ok(e) => e,
                Err(_) => return WalkState::Continue,
            };

            if entry.depth() == 0 && !include_root {
                return WalkState::Continue;
            }

            let path = entry.path();

            // Avoid stat() syscalls unless filters or -l require metadata.
            let meta = if need_metadata {
                entry.metadata().ok()
            } else {
                None
            };

            if !filter.matches(path, meta.as_ref()) {
                return WalkState::Continue;
            }

            if let Some(limit) = max_results {
                let prev = sent_count.fetch_add(1, Ordering::Relaxed);
                if prev >= limit {
                    return WalkState::Quit;
                }
            }

            let ft = entry.file_type();
            let is_dir = ft.map(|f| f.is_dir()).unwrap_or(false);
            let is_symlink = ft.map(|f| f.is_symlink()).unwrap_or(false);
            let is_exec = meta.as_ref().is_some_and(is_executable_meta);

            let found = FoundEntry {
                path: path.to_path_buf(),
                display: format_display_path(path),
                is_dir,
                is_symlink,
                is_exec,
            };

            if tx.send(found).is_err() {
                return WalkState::Quit;
            }

            if let Some(limit) = max_results {
                if sent_count.load(Ordering::Relaxed) >= limit {
                    return WalkState::Quit;
                }
            }

            WalkState::Continue
        })
    });

    Ok(())
}

/// Collect all matching entries into a `Vec<FoundEntry>`.
pub fn walk_collect(opts: &WalkOpts) -> Result<Vec<FoundEntry>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    walk_stream(opts, tx)?;
    Ok(rx.into_iter().collect())
}

fn format_display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("./") {
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }
    s.into_owned()
}

#[cfg(unix)]
fn is_executable_meta(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && (meta.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_executable_meta(meta: &std::fs::Metadata) -> bool {
    meta.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn walk_collect_respects_hidden_exclude_and_max_depth() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("alpha.rs"), "fn main() {}").unwrap();
        fs::write(root.join("beta.log"), "log").unwrap();
        fs::write(root.join(".secret"), "hidden").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/gamma.rs"), "pub fn g() {}").unwrap();
        fs::create_dir(root.join("sub/deep")).unwrap();
        fs::write(root.join("sub/deep/delta.rs"), "pub fn d() {}").unwrap();

        // Default: hidden excluded, .log excluded via -E, max_depth 2 omits sub/deep/delta.rs.
        let args = Args::try_parse_from([
            "rsfind",
            "-E",
            "*.log",
            "--max-depth",
            "2",
            "--search-path",
            root.to_str().unwrap(),
        ])
        .unwrap();
        let opts = WalkOpts::from_args(&args).unwrap();
        assert!(!opts.need_metadata);

        let entries = walk_collect(&opts).unwrap();
        let names: Vec<&str> = entries
            .iter()
            .filter_map(|e| e.path.file_name().and_then(|n| n.to_str()))
            .collect();

        assert!(names.contains(&"alpha.rs"));
        assert!(names.contains(&"sub"));
        assert!(names.contains(&"gamma.rs"));
        assert!(!names.contains(&"beta.log"));
        assert!(!names.contains(&".secret"));
        assert!(!names.contains(&"delta.rs"));

        // With -H, hidden file is included.
        let args_hidden = Args::try_parse_from([
            "rsfind",
            "-H",
            "-t",
            "f",
            "--search-path",
            root.to_str().unwrap(),
        ])
        .unwrap();
        let opts_hidden = WalkOpts::from_args(&args_hidden).unwrap();
        assert!(opts_hidden.need_metadata);
        let hidden_entries = walk_collect(&opts_hidden).unwrap();
        let hidden_names: Vec<&str> = hidden_entries
            .iter()
            .filter_map(|e| e.path.file_name().and_then(|n| n.to_str()))
            .collect();
        assert!(hidden_names.contains(&".secret"));
        assert!(hidden_names.contains(&"delta.rs"));
    }

    #[test]
    fn walk_stream_respects_max_results() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for i in 0..10 {
            fs::write(root.join(format!("file_{i}.txt")), "x").unwrap();
        }

        let args =
            Args::try_parse_from(["rsfind", "-1", "3", "--search-path", root.to_str().unwrap()])
                .unwrap();
        let opts = WalkOpts::from_args(&args).unwrap();
        let entries = walk_collect(&opts).unwrap();
        assert_eq!(entries.len(), 3);
    }
}
