use std::io::{self, IsTerminal, Write};

use serde_json::json;

use crate::cli::Args;
use crate::search::{FileMatches, LineMatch};

/// Formatting options for CLI output.
#[derive(Debug, Clone, Copy)]
pub struct OutputOpts {
    pub color: bool,
    pub line_number: bool,
    pub show_filename: bool,
    pub has_context: bool,
    pub files_with_matches: bool,
    pub count: bool,
    pub json: bool,
}

impl OutputOpts {
    /// Builds `OutputOpts` from CLI arguments and stdout TTY detection.
    pub fn from_args(args: &Args) -> Self {
        let color = !args.no_color && io::stdout().is_terminal();
        Self {
            color,
            line_number: args.line_number,
            show_filename: args.should_show_filename(),
            has_context: args.before_lines() > 0 || args.after_lines() > 0,
            files_with_matches: args.files_with_matches,
            count: args.count,
            json: args.json,
        }
    }
}

/// Writes search results to `out` according to `opts`.
pub fn write_results<W: Write>(
    out: &mut W,
    results: &[FileMatches],
    opts: &OutputOpts,
) -> io::Result<()> {
    if opts.json {
        return write_json(out, results, opts);
    }

    if opts.files_with_matches {
        for file in results {
            writeln!(out, "{}", file.path.display())?;
        }
        return Ok(());
    }

    if opts.count {
        for file in results {
            if opts.show_filename {
                write_colored(
                    out,
                    &file.path.display().to_string(),
                    "\x1b[35m",
                    opts.color,
                )?;
                write_colored(out, ":", "\x1b[36m", opts.color)?;
            }
            writeln!(out, "{}", file.match_count)?;
        }
        return Ok(());
    }

    let mut need_separator = false;
    for file in results {
        let path_str = file.path.display().to_string();
        for hunk in &file.hunks {
            if opts.has_context && need_separator {
                write_colored(out, "--", "\x1b[36m", opts.color)?;
                writeln!(out)?;
            }
            for line in hunk {
                write_line(out, &path_str, line, opts)?;
            }
            if opts.has_context {
                need_separator = true;
            }
        }
    }

    Ok(())
}

fn write_line<W: Write>(
    out: &mut W,
    path_str: &str,
    line: &LineMatch,
    opts: &OutputOpts,
) -> io::Result<()> {
    let sep = if line.is_match { ":" } else { "-" };
    if opts.show_filename {
        write_colored(out, path_str, "\x1b[35m", opts.color)?;
        write_colored(out, sep, "\x1b[36m", opts.color)?;
    }
    if opts.line_number {
        write_colored(out, &line.line_number.to_string(), "\x1b[32m", opts.color)?;
        write_colored(out, sep, "\x1b[36m", opts.color)?;
    }

    if opts.color && line.is_match && !line.spans.is_empty() {
        write_highlighted_text(out, &line.text, &line.spans)?;
        writeln!(out)
    } else {
        writeln!(out, "{}", line.text)
    }
}

fn write_highlighted_text<W: Write>(
    out: &mut W,
    text: &str,
    spans: &[(usize, usize)],
) -> io::Result<()> {
    let mut cursor = 0;
    for &(start, end) in spans {
        if start > cursor && start <= text.len() {
            out.write_all(&text.as_bytes()[cursor..start])?;
        }
        if end > start && end <= text.len() {
            out.write_all(b"\x1b[1;31m")?;
            out.write_all(&text.as_bytes()[start..end])?;
            out.write_all(b"\x1b[0m")?;
        }
        cursor = end.max(cursor);
    }
    if cursor < text.len() {
        out.write_all(&text.as_bytes()[cursor..])?;
    }
    Ok(())
}

fn write_colored<W: Write>(out: &mut W, text: &str, ansi: &str, color: bool) -> io::Result<()> {
    if color {
        write!(out, "{ansi}{text}\x1b[0m")
    } else {
        write!(out, "{text}")
    }
}

fn write_json<W: Write>(out: &mut W, results: &[FileMatches], opts: &OutputOpts) -> io::Result<()> {
    for file in results {
        let path = file.path.display().to_string();
        if opts.files_with_matches {
            let val = json!({
                "type": "file",
                "path": path,
            });
            serde_json::to_writer(&mut *out, &val)?;
            writeln!(out)?;
            continue;
        }
        if opts.count {
            let val = json!({
                "type": "count",
                "path": path,
                "count": file.match_count,
            });
            serde_json::to_writer(&mut *out, &val)?;
            writeln!(out)?;
            continue;
        }
        for hunk in &file.hunks {
            for line in hunk {
                let kind = if line.is_match { "match" } else { "context" };
                let submatches: Vec<_> = line
                    .spans
                    .iter()
                    .map(|&(start, end)| json!({ "start": start, "end": end }))
                    .collect();
                let val = json!({
                    "type": kind,
                    "path": path,
                    "line_number": line.line_number,
                    "text": line.text,
                    "submatches": submatches,
                });
                serde_json::to_writer(&mut *out, &val)?;
                writeln!(out)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sample_results() -> Vec<FileMatches> {
        vec![FileMatches {
            path: PathBuf::from("a.txt"),
            match_count: 2,
            hunks: vec![
                vec![
                    LineMatch {
                        line_number: 1,
                        text: "hello world".to_string(),
                        is_match: true,
                        spans: vec![(0, 5)],
                    },
                    LineMatch {
                        line_number: 2,
                        text: "after".to_string(),
                        is_match: false,
                        spans: vec![],
                    },
                ],
                vec![LineMatch {
                    line_number: 10,
                    text: "hello again".to_string(),
                    is_match: true,
                    spans: vec![(0, 5)],
                }],
            ],
        }]
    }

    #[test]
    fn plain_output_with_context_separators() {
        let opts = OutputOpts {
            color: false,
            line_number: true,
            show_filename: true,
            has_context: true,
            files_with_matches: false,
            count: false,
            json: false,
        };
        let mut buf = Vec::new();
        write_results(&mut buf, &sample_results(), &opts).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(
            out,
            "a.txt:1:hello world\na.txt-2-after\n--\na.txt:10:hello again\n"
        );
    }

    #[test]
    fn files_with_matches_and_count_modes() {
        let results = sample_results();

        let l_opts = OutputOpts {
            color: false,
            line_number: false,
            show_filename: true,
            has_context: false,
            files_with_matches: true,
            count: false,
            json: false,
        };
        let mut buf = Vec::new();
        write_results(&mut buf, &results, &l_opts).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a.txt\n");

        let c_opts = OutputOpts {
            files_with_matches: false,
            count: true,
            ..l_opts
        };
        let mut buf = Vec::new();
        write_results(&mut buf, &results, &c_opts).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a.txt:2\n");
    }

    #[test]
    fn json_lines_output_is_valid_ndjson() {
        let opts = OutputOpts {
            color: false,
            line_number: true,
            show_filename: true,
            has_context: true,
            files_with_matches: false,
            count: false,
            json: true,
        };
        let mut buf = Vec::new();
        write_results(&mut buf, &sample_results(), &opts).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).expect("valid json line"))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["type"], "match");
        assert_eq!(lines[0]["path"], "a.txt");
        assert_eq!(lines[0]["line_number"], 1);
        assert_eq!(lines[0]["submatches"][0]["start"], 0);
        assert_eq!(lines[0]["submatches"][0]["end"], 5);
        assert_eq!(lines[1]["type"], "context");
        assert_eq!(lines[2]["type"], "match");
    }

    #[test]
    fn no_color_omits_ansi_escapes_and_color_includes_them() {
        let mut opts = OutputOpts {
            color: true,
            line_number: true,
            show_filename: true,
            has_context: false,
            files_with_matches: false,
            count: false,
            json: false,
        };
        let mut colored = Vec::new();
        write_results(&mut colored, &sample_results(), &opts).unwrap();
        assert!(String::from_utf8(colored).unwrap().contains("\x1b[1;31m"));

        opts.color = false;
        let mut plain = Vec::new();
        write_results(&mut plain, &sample_results(), &opts).unwrap();
        assert!(!String::from_utf8(plain).unwrap().contains('\x1b'));
    }
}
