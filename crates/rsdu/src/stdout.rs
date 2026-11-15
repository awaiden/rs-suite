use crate::tree::{Item, SortBy};
use rs_common::fmt::{self, Fmt};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::Write;

pub struct Cfg {
    pub fmt: Fmt,
    pub apparent: bool,
    pub show_count: bool,
    pub color: bool,
    pub reverse: bool,
    pub top: usize,
    pub depth: usize,
    pub summarize: bool,
}

fn paint(on: bool, code: &str, s: String) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s
    }
}

struct TopEntry<'a> {
    metric: i64,
    seq: usize,
    path: String,
    item: &'a Item,
}

impl PartialEq for TopEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.metric == other.metric && self.seq == other.seq
    }
}

impl Eq for TopEntry<'_> {}

impl PartialOrd for TopEntry<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TopEntry<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Max-heap on (Reverse(metric), seq) so the smallest metric (or latest tie) is at peek()
        Reverse(self.metric)
            .cmp(&Reverse(other.metric))
            .then_with(|| self.seq.cmp(&other.seq))
    }
}

pub fn print(root: &mut Item, scan_path: &str, c: &Cfg, w: &mut impl Write) -> std::io::Result<()> {
    let metric = |i: &Item| if c.apparent { i.size } else { i.usage };
    let size = |i: &Item| paint(c.color, "33", format!("{:>9}", c.fmt.size(metric(i))));
    let by = if c.apparent {
        SortBy::Apparent
    } else {
        SortBy::Size
    };
    let path_line = |w: &mut dyn Write, i: &Item, p: &str| {
        let clean = rs_common::sanitize_control_chars(p);
        let p = if i.is_dir() {
            paint(c.color, "34", clean)
        } else {
            clean
        };
        writeln!(w, "{} {p}", size(i))
    };

    if c.summarize {
        let base = std::path::Path::new(scan_path)
            .file_name()
            .map_or(scan_path.to_string(), |n| n.to_string_lossy().into());
        let base = rs_common::sanitize_control_chars(&base);
        return writeln!(w, "{} {base}", size(root));
    }
    if c.top > 0 {
        let mut heap = BinaryHeap::with_capacity(c.top);
        let mut seq = 0;
        collect_top(
            root,
            scan_path.trim_end_matches('/'),
            c.top,
            &metric,
            &mut seq,
            &mut heap,
        );
        let mut top_items = heap.into_vec();
        top_items.sort_unstable_by_key(|e| (Reverse(e.metric), e.seq));
        for e in top_items {
            path_line(w, e.item, &e.path)?;
        }
        return Ok(());
    }
    if c.depth > 0 {
        return walk(root, scan_path, 0, c, by, &path_line, w);
    }
    root.sort(by, !c.reverse);
    for f in root.children() {
        let clean = rs_common::sanitize_control_chars(&f.name);
        let name = if f.is_dir() {
            paint(c.color, "34", format!("/{clean}"))
        } else {
            clean
        };
        let cnt = if c.show_count {
            format!(
                " {:>11}",
                paint(c.color, "31", fmt::count(f.display_count()))
            )
        } else {
            String::new()
        };
        writeln!(w, "{} {}{cnt} {name}", f.flag(), size(f))?;
    }
    Ok(())
}

fn collect_top<'a>(
    d: &'a Item,
    prefix: &str,
    top: usize,
    metric: &impl Fn(&Item) -> i64,
    seq: &mut usize,
    heap: &mut BinaryHeap<TopEntry<'a>>,
) {
    for ch in d.children() {
        if ch.is_dir() {
            let p = format!("{prefix}/{}", ch.name);
            collect_top(ch, &p, top, metric, seq, heap);
        } else {
            let m = metric(ch);
            *seq += 1;
            if heap.len() < top {
                heap.push(TopEntry {
                    metric: m,
                    seq: *seq,
                    path: format!("{prefix}/{}", ch.name),
                    item: ch,
                });
            } else if let Some(min) = heap.peek()
                && m > min.metric
            {
                heap.pop();
                heap.push(TopEntry {
                    metric: m,
                    seq: *seq,
                    path: format!("{prefix}/{}", ch.name),
                    item: ch,
                });
            }
        }
    }
}

fn walk(
    d: &mut Item,
    path: &str,
    level: usize,
    c: &Cfg,
    by: SortBy,
    line: &dyn Fn(&mut dyn Write, &Item, &str) -> std::io::Result<()>,
    w: &mut impl Write,
) -> std::io::Result<()> {
    line(w, d, path)?;
    if level < c.depth {
        d.sort(by, !c.reverse);
        let prefix = path.trim_end_matches('/');
        for ch in d.children_mut() {
            let p = format!("{prefix}/{}", ch.name);
            if ch.is_dir() {
                walk(ch, &p, level + 1, c, by, line, w)?;
            } else {
                line(w, ch, &p)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> Item {
        let mut root = Item::new_dir("/root", ' ');
        let mut sub = Item::new_dir("sub", ' ');
        sub.children_mut()
            .push(Item::new_file("big.bin", ' ', 5000, 8192, 0, 0));
        sub.children_mut()
            .push(Item::new_file("tiny.txt", ' ', 10, 4096, 0, 0));
        root.children_mut().push(sub);
        root.children_mut()
            .push(Item::new_file("mid.dat", ' ', 1000, 4096, 0, 0));
        root.update_stats();
        root
    }

    fn base_cfg() -> Cfg {
        Cfg {
            fmt: Fmt {
                no_prefix: true,
                ..Default::default()
            },
            apparent: true,
            show_count: false,
            color: false,
            reverse: false,
            top: 0,
            depth: 0,
            summarize: false,
        }
    }

    #[test]
    fn summarize_prints_single_total_line() {
        let mut root = sample_tree();
        let cfg = Cfg {
            summarize: true,
            ..base_cfg()
        };
        let mut buf = Vec::new();
        print(&mut root, "/tmp/myroot", &cfg, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.contains("6010 myroot"), "{out}");
    }

    #[test]
    fn top_prints_largest_files_descending() {
        let mut root = sample_tree();
        let cfg = Cfg {
            top: 2,
            ..base_cfg()
        };
        let mut buf = Vec::new();
        print(&mut root, "/tmp/myroot", &cfg, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].contains("5000 /tmp/myroot/sub/big.bin"), "{out}");
        assert!(lines[1].contains("1000 /tmp/myroot/mid.dat"), "{out}");
    }

    #[test]
    fn depth_limits_tree_traversal() {
        let mut root = sample_tree();
        let cfg = Cfg {
            depth: 1,
            ..base_cfg()
        };
        let mut buf = Vec::new();
        print(&mut root, "/tmp/myroot", &cfg, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("/tmp/myroot\n"), "{out}");
        assert!(out.contains("/tmp/myroot/sub\n"), "{out}");
        assert!(out.contains("/tmp/myroot/mid.dat\n"), "{out}");
        assert!(!out.contains("big.bin"), "{out}");
    }
}
