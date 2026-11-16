use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};

use crate::cli::Args;

/// Options controlling file discovery and content matching.
#[derive(Debug, Clone)]
pub struct SearchOpts {
    pub pattern: String,
    pub paths: Vec<PathBuf>,
    pub fixed_strings: bool,
    pub ignore_case: bool,
    pub smart_case: bool,
    pub before_context: usize,
    pub after_context: usize,
    pub hidden: bool,
    pub no_ignore: bool,
    pub globs: Vec<String>,
    pub max_depth: Option<usize>,
}

impl SearchOpts {
    /// Builds `SearchOpts` from parsed CLI arguments and an explicit pattern.
    pub fn from_args(args: &Args, pattern: String) -> Self {
        Self {
            pattern,
            paths: args.effective_paths(),
            fixed_strings: args.fixed_strings,
            ignore_case: args.ignore_case,
            smart_case: args.smart_case,
            before_context: args.before_lines(),
            after_context: args.after_lines(),
            hidden: args.hidden,
            no_ignore: args.no_ignore,
            globs: args.globs.clone(),
            max_depth: args.max_depth,
        }
    }
}

/// A single matching or context line within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    /// 1-based line number.
    pub line_number: usize,
    /// Line text without trailing line endings.
    pub text: String,
    /// `true` if this line matched the pattern, `false` if it is a context line.
    pub is_match: bool,
    /// Byte offsets `(start, end)` of pattern matches within `text`.
    pub spans: Vec<(usize, usize)>,
}

/// All matching hunks discovered in a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    pub path: PathBuf,
    /// Total number of matching lines in this file.
    pub match_count: usize,
    /// Contiguous groups of lines (matches plus merged context).
    pub hunks: Vec<Vec<LineMatch>>,
}

/// Compiles the search regex according to `fixed_strings`, `ignore_case`, and `smart_case`.
pub fn build_regex(opts: &SearchOpts) -> Result<Regex, regex::Error> {
    let raw = if opts.fixed_strings {
        regex::escape(&opts.pattern)
    } else {
        opts.pattern.clone()
    };
    let case_insensitive =
        opts.ignore_case || (opts.smart_case && !opts.pattern.chars().any(char::is_uppercase));
    RegexBuilder::new(&raw)
        .case_insensitive(case_insensitive)
        .build()
}

/// Walks the configured paths and returns sorted candidate file paths.
pub fn collect_files(opts: &SearchOpts) -> Result<Vec<PathBuf>, String> {
    let first = opts
        .paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));

    let mut builder = WalkBuilder::new(&first);
    for path in opts.paths.iter().skip(1) {
        builder.add(path);
    }

    builder
        .hidden(!opts.hidden)
        .git_ignore(!opts.no_ignore)
        .git_global(!opts.no_ignore)
        .git_exclude(!opts.no_ignore)
        .ignore(!opts.no_ignore)
        .max_depth(opts.max_depth);

    if !opts.globs.is_empty() {
        let base = if first.is_file() {
            first.parent().unwrap_or_else(|| Path::new("."))
        } else {
            first.as_path()
        };
        let mut overrides = OverrideBuilder::new(base);
        for glob in &opts.globs {
            overrides
                .add(glob)
                .map_err(|e| format!("invalid glob '{glob}': {e}"))?;
        }
        let built = overrides
            .build()
            .map_err(|e| format!("failed to build glob overrides: {e}"))?;
        builder.overrides(built);
    }

    let mut files = Vec::new();
    for entry in builder.build() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            files.push(entry.into_path());
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

/// Searches a single file, skipping binary files containing NUL bytes and merging context ranges.
pub fn search_file(
    path: &Path,
    re: &Regex,
    before_context: usize,
    after_context: usize,
) -> Option<FileMatches> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }

    let content = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut match_indices: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let spans: Vec<(usize, usize)> = re.find_iter(line).map(|m| (m.start(), m.end())).collect();
        if !spans.is_empty() {
            match_indices.push((idx, spans));
        }
    }

    if match_indices.is_empty() {
        return None;
    }

    let match_count = match_indices.len();
    let max_idx = lines.len() - 1;
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &(idx, _) in &match_indices {
        let start = idx.saturating_sub(before_context);
        let end = (idx + after_context).min(max_idx);
        if let Some(last) = ranges.last_mut() {
            if start <= last.1 + 1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        ranges.push((start, end));
    }

    let mut hunks = Vec::with_capacity(ranges.len());
    let mut m_ptr = 0;
    for (start, end) in ranges {
        let mut hunk = Vec::with_capacity(end - start + 1);
        for (i, line) in lines.iter().enumerate().take(end + 1).skip(start) {
            let (is_match, spans) = if let Some((m_idx, m_spans)) = match_indices.get(m_ptr)
                && *m_idx == i
            {
                m_ptr += 1;
                (true, m_spans.clone())
            } else {
                (false, Vec::new())
            };
            hunk.push(LineMatch {
                line_number: i + 1,
                text: (*line).to_string(),
                is_match,
                spans,
            });
        }
        hunks.push(hunk);
    }

    Some(FileMatches {
        path: path.to_path_buf(),
        match_count,
        hunks,
    })
}

/// Runs parallel search across all discovered files and returns results sorted by path.
pub fn search(opts: &SearchOpts) -> Result<Vec<FileMatches>, String> {
    let re = build_regex(opts).map_err(|e| format!("invalid pattern: {e}"))?;
    let files = collect_files(opts)?;
    let mut results: Vec<FileMatches> = files
        .par_iter()
        .filter_map(|path| search_file(path, &re, opts.before_context, opts.after_context))
        .collect();
    results.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(results)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    fn base_opts(root: &Path, pattern: &str) -> SearchOpts {
        SearchOpts {
            pattern: pattern.to_string(),
            paths: vec![root.to_path_buf()],
            fixed_strings: false,
            ignore_case: false,
            smart_case: false,
            before_context: 0,
            after_context: 0,
            hidden: false,
            no_ignore: false,
            globs: Vec::new(),
            max_depth: None,
        }
    }

    #[test]
    fn respects_gitignore_and_no_ignore() {
        let dir = TempDir::new().unwrap();
        // Initialize a .git directory so `ignore` recognizes `.gitignore`.
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("kept.txt"), "hello world\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "hello ignored\n").unwrap();

        let opts = base_opts(dir.path(), "hello");
        let res = search(&opts).unwrap();
        assert_eq!(res.len(), 1);
        assert!(res[0].path.ends_with("kept.txt"));

        let mut no_ignore_opts = opts;
        no_ignore_opts.no_ignore = true;
        let res_all = search(&no_ignore_opts).unwrap();
        assert_eq!(res_all.len(), 2);
        assert!(res_all[0].path.ends_with("ignored.txt"));
        assert!(res_all[1].path.ends_with("kept.txt"));
    }

    #[test]
    fn fixed_strings_escapes_regex_metacharacters() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "foo.bar\nfooxbar\n").unwrap();

        let mut opts = base_opts(dir.path(), "foo.bar");
        let regex_res = search(&opts).unwrap();
        assert_eq!(regex_res[0].match_count, 2);

        opts.fixed_strings = true;
        let fixed_res = search(&opts).unwrap();
        assert_eq!(fixed_res[0].match_count, 1);
        assert_eq!(fixed_res[0].hunks[0][0].line_number, 1);
    }

    #[test]
    fn case_sensitivity_ignore_and_smart_case() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("case.txt"), "Alpha\nalpha\nALPHA\n").unwrap();

        // Default: case sensitive
        let mut opts = base_opts(dir.path(), "Alpha");
        assert_eq!(search(&opts).unwrap()[0].match_count, 1);

        // -i: ignore case
        opts.ignore_case = true;
        assert_eq!(search(&opts).unwrap()[0].match_count, 3);

        // -S: smart case (lowercase pattern -> insensitive, uppercase -> sensitive)
        opts.ignore_case = false;
        opts.smart_case = true;
        opts.pattern = "alpha".to_string();
        assert_eq!(search(&opts).unwrap()[0].match_count, 3);

        opts.pattern = "Alpha".to_string();
        assert_eq!(search(&opts).unwrap()[0].match_count, 1);
    }

    #[test]
    fn context_lines_merge_overlapping_and_adjacent_ranges_without_duplicates() {
        let dir = TempDir::new().unwrap();
        let content = (1..=10)
            .map(|i| {
                if i == 2 || i == 4 || i == 9 {
                    format!("line {i} MATCH")
                } else {
                    format!("line {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(dir.path().join("ctx.txt"), content).unwrap();

        let mut opts = base_opts(dir.path(), "MATCH");
        opts.before_context = 1;
        opts.after_context = 1;

        let res = search(&opts).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].match_count, 3);
        // Line 2 -> [1..=3], Line 4 -> [3..=5] => merged [1..=5] (lines 1..=5)
        // Line 9 -> [8..=10] (lines 8..=10)
        assert_eq!(res[0].hunks.len(), 2);

        let hunk1_lines: Vec<usize> = res[0].hunks[0].iter().map(|l| l.line_number).collect();
        assert_eq!(hunk1_lines, vec![1, 2, 3, 4, 5]);
        assert!(res[0].hunks[0][1].is_match);
        assert!(!res[0].hunks[0][2].is_match);
        assert!(res[0].hunks[0][3].is_match);

        let hunk2_lines: Vec<usize> = res[0].hunks[1].iter().map(|l| l.line_number).collect();
        assert_eq!(hunk2_lines, vec![8, 9, 10]);
    }

    #[test]
    fn skips_binary_files_with_nul_bytes() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("bin.dat"), b"hello\0world\n").unwrap();
        fs::write(dir.path().join("txt.txt"), b"hello world\n").unwrap();

        let opts = base_opts(dir.path(), "hello");
        let res = search(&opts).unwrap();
        assert_eq!(res.len(), 1);
        assert!(res[0].path.ends_with("txt.txt"));
    }

    #[test]
    fn hidden_glob_and_max_depth_filters() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join(".secret"), "needle\n").unwrap();
        fs::write(dir.path().join("top.rs"), "needle\n").unwrap();
        fs::write(dir.path().join("sub/deep.rs"), "needle\n").unwrap();
        fs::write(dir.path().join("sub/deep.txt"), "needle\n").unwrap();

        let mut opts = base_opts(dir.path(), "needle");
        opts.globs = vec!["*.rs".to_string()];
        let res = search(&opts).unwrap();
        assert_eq!(res.len(), 2);

        opts.max_depth = Some(1);
        let shallow = search(&opts).unwrap();
        assert_eq!(shallow.len(), 1);
        assert!(shallow[0].path.ends_with("top.rs"));

        let mut hidden_opts = base_opts(dir.path(), "needle");
        hidden_opts.hidden = true;
        let hidden_res = search(&hidden_opts).unwrap();
        assert!(hidden_res.iter().any(|f| f.path.ends_with(".secret")));
    }
}
