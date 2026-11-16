use std::cmp::Reverse;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Case sensitivity mode for fuzzy and extended search matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseMode {
    /// Case-insensitive unless the term contains an uppercase character.
    #[default]
    Smart,
    /// Always case-sensitive.
    Sensitive,
    /// Always case-insensitive.
    Ignore,
}

impl CaseMode {
    fn to_nucleo(self) -> CaseMatching {
        match self {
            CaseMode::Smart => CaseMatching::Smart,
            CaseMode::Sensitive => CaseMatching::Respect,
            CaseMode::Ignore => CaseMatching::Ignore,
        }
    }
}

/// A single matched item with its original index, score, and matched character positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    /// Index of the item in the input slice.
    pub index: usize,
    /// Combined match score (higher is better; `0` for empty or negative-only queries).
    pub score: u32,
    /// Sorted, deduplicated Unicode scalar indices of matched characters in the item.
    pub indices: Vec<u32>,
}

/// A parsed `fzf`-style extended search query consisting of OR-separated (`|`)
/// clauses of space-separated AND terms.
#[derive(Debug, Clone)]
pub struct ParsedQuery {
    clauses: Vec<Pattern>,
}

impl ParsedQuery {
    /// Parse a raw query string into OR clauses of AND-ed `nucleo_matcher` patterns.
    pub fn parse(query: &str, case_mode: CaseMode) -> Self {
        let case = case_mode.to_nucleo();
        let clauses = split_or_clauses(query)
            .into_iter()
            .filter_map(|clause| {
                let pat = Pattern::parse(&clause, case, Normalization::Smart);
                (!pat.atoms.is_empty()).then_some(pat)
            })
            .collect();
        Self { clauses }
    }

    /// Returns `true` if the query has no active terms (matches everything).
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }
}

/// Split a query on unescaped `|` characters, unescaping `\|` to `|` within each clause.
fn split_or_clauses(query: &str) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut current = String::with_capacity(query.len());
    let mut escaped = false;

    for ch in query.chars() {
        if escaped {
            if ch == '|' {
                current.push('|');
            } else {
                current.push('\\');
                current.push(ch);
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            clauses.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }

    if escaped {
        current.push('\\');
    }
    clauses.push(current);
    clauses
}

/// Reusable fuzzy matching engine backed by `nucleo_matcher::Matcher`.
///
/// Retains the matcher's internal scratch slab (~135 KiB) and UTF-32 conversion
/// buffer across queries so interactive TUI updates avoid repeated allocations.
pub struct FuzzyEngine {
    matcher: Matcher,
    utf32_buf: Vec<char>,
    scratch_indices: Vec<u32>,
}

impl Default for FuzzyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyEngine {
    /// Create a new reusable fuzzy matching engine configured for file paths.
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            utf32_buf: Vec::new(),
            scratch_indices: Vec::new(),
        }
    }

    /// Score a single item against a parsed query without computing highlight indices.
    pub fn score_item(&mut self, query: &ParsedQuery, text: &str) -> Option<u32> {
        if query.is_empty() {
            return Some(0);
        }

        let haystack = Utf32Str::new(text, &mut self.utf32_buf);
        let mut best: Option<u32> = None;
        for clause in &query.clauses {
            if let Some(score) = clause.score(haystack, &mut self.matcher) {
                best = Some(best.map_or(score, |prev| prev.max(score)));
            }
        }
        best
    }

    /// Match a single item against a parsed query, returning `(score, sorted_indices)`.
    pub fn match_item(&mut self, query: &ParsedQuery, text: &str) -> Option<(u32, Vec<u32>)> {
        if query.is_empty() {
            return Some((0, Vec::new()));
        }

        let haystack = Utf32Str::new(text, &mut self.utf32_buf);
        let mut best_score: Option<u32> = None;
        let mut best_indices = Vec::new();

        for clause in &query.clauses {
            self.scratch_indices.clear();
            if let Some(score) =
                clause.indices(haystack, &mut self.matcher, &mut self.scratch_indices)
            {
                if best_score.is_none_or(|prev| score > prev) {
                    best_score = Some(score);
                    best_indices.clear();
                    best_indices.extend_from_slice(&self.scratch_indices);
                }
            }
        }

        best_score.map(|score| {
            best_indices.sort_unstable();
            best_indices.dedup();
            (score, best_indices)
        })
    }

    /// Filter and rank `items` against `query`, sorting by descending score,
    /// then shorter string length, then original index for stability.
    pub fn rank_items<T: AsRef<str>>(
        &mut self,
        query: &str,
        items: &[T],
        case_mode: CaseMode,
    ) -> Vec<MatchResult> {
        let parsed = ParsedQuery::parse(query, case_mode);
        let mut results: Vec<MatchResult> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                self.match_item(&parsed, item.as_ref())
                    .map(|(score, indices)| MatchResult {
                        index,
                        score,
                        indices,
                    })
            })
            .collect();

        results.sort_by_key(|r| (Reverse(r.score), items[r.index].as_ref().len(), r.index));
        results
    }
}

/// Convenience one-shot function to filter and rank `items` against `query`.
///
/// For repeated matching (such as an interactive TUI loop), prefer reusing a
/// [`FuzzyEngine`] instance to avoid reallocating the matcher's scratch buffer.
pub fn rank_items<T: AsRef<str>>(
    query: &str,
    items: &[T],
    case_mode: CaseMode,
) -> Vec<MatchResult> {
    let mut engine = FuzzyEngine::new();
    engine.rank_items(query, items, case_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched_strings<'a>(items: &'a [&str], results: &[MatchResult]) -> Vec<&'a str> {
        results.iter().map(|r| items[r.index]).collect()
    }

    #[test]
    fn empty_query_matches_all_sorted_by_length_then_index() {
        let items = ["src/main.rs", "a.rs", "b.rs", "Cargo.toml"];
        let results = rank_items("", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &results),
            vec!["a.rs", "b.rs", "Cargo.toml", "src/main.rs"]
        );
        assert!(results.iter().all(|r| r.score == 0 && r.indices.is_empty()));

        let ws_results = rank_items("   ", &items, CaseMode::Smart);
        assert_eq!(ws_results, results);
    }

    #[test]
    fn fuzzy_match_and_indices_positions() {
        let items = ["src/fuzzy.rs", "src/filter.rs", "README.md"];
        let results = rank_items("fzy", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &results), vec!["src/fuzzy.rs"]);
        // 'f' at 4, 'z' at 6 or 7, 'y' at 8 in "src/fuzzy.rs"
        let chars: Vec<char> = items[results[0].index].chars().collect();
        let matched: String = results[0]
            .indices
            .iter()
            .map(|&i| chars[i as usize])
            .collect();
        assert_eq!(matched, "fzy");
        assert!(results[0].score > 0);
    }

    #[test]
    fn exact_substring_operator_single_quote() {
        let items = ["src/fuzzy.rs", "src/f_u_z_z.rs"];
        let results = rank_items("'fuzz", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &results), vec!["src/fuzzy.rs"]);
        assert_eq!(results[0].indices, vec![4, 5, 6, 7]);
    }

    #[test]
    fn prefix_and_suffix_and_exact_operators() {
        let items = ["src/main.rs", "tests/main.rs", "src/main.rs.bak", "main.rs"];

        let prefix = rank_items("^src/", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &prefix),
            vec!["src/main.rs", "src/main.rs.bak"]
        );

        let suffix = rank_items(".rs$", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &suffix),
            vec!["main.rs", "src/main.rs", "tests/main.rs"]
        );

        let exact = rank_items("^main.rs$", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &exact), vec!["main.rs"]);
    }

    #[test]
    fn negated_operators() {
        let items = [
            "src/main.rs",
            "src/test_helper.rs",
            "tests/integration.rs",
            "src/lib.rs.bak",
        ];

        // !term (negated substring)
        let neg_plain = rank_items(".rs$ !test", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &neg_plain), vec!["src/main.rs"]);

        // !'term (negated explicit substring)
        let neg_quote = rank_items(".rs$ !'helper", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &neg_quote),
            vec!["src/main.rs", "tests/integration.rs"]
        );

        // !^term (negated prefix)
        let neg_prefix = rank_items(".rs$ !^tests/", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &neg_prefix),
            vec!["src/main.rs", "src/test_helper.rs"]
        );

        // !term$ (negated suffix)
        let neg_suffix = rank_items("^src/ !.bak$", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &neg_suffix),
            vec!["src/main.rs", "src/test_helper.rs"]
        );

        // Negated-only query
        let neg_only = rank_items("!test", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &neg_only),
            vec!["src/main.rs", "src/lib.rs.bak"]
        );
        assert!(neg_only.iter().all(|r| r.indices.is_empty()));
    }

    #[test]
    fn and_and_or_clauses() {
        let items = [
            "src/fuzzy.rs",
            "src/filter.rs",
            "tests/fuzzy_test.rs",
            "Cargo.toml",
            "README.md",
        ];

        // AND: must match both ^src/ and .rs$
        let and_res = rank_items("^src/ .rs$", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &and_res),
            vec!["src/fuzzy.rs", "src/filter.rs"]
        );

        // OR: match (^src/ AND fuzzy) OR Cargo.toml
        let or_res = rank_items("^src/ fuzzy | ^Cargo.toml$", &items, CaseMode::Smart);
        let matched = matched_strings(&items, &or_res);
        assert_eq!(matched.len(), 2);
        assert!(matched.contains(&"src/fuzzy.rs"));
        assert!(matched.contains(&"Cargo.toml"));

        // Trailing pipe should not match everything
        let trailing_pipe = rank_items("Cargo |", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &trailing_pipe), vec!["Cargo.toml"]);
    }

    #[test]
    fn escaped_spaces_and_pipes() {
        let items = ["my file.txt", "a|b.txt", "other.txt"];

        let escaped_space = rank_items("'my\\ file", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &escaped_space), vec!["my file.txt"]);

        let escaped_pipe = rank_items("'a\\|b", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &escaped_pipe), vec!["a|b.txt"]);
    }

    #[test]
    fn case_modes_smart_sensitive_ignore() {
        let items = ["src/Main.rs", "src/main.rs"];

        // Smart: lowercase query matches both
        let smart_lower = rank_items("'main", &items, CaseMode::Smart);
        assert_eq!(smart_lower.len(), 2);

        // Smart: uppercase in query becomes case-sensitive
        let smart_upper = rank_items("'Main", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &smart_upper), vec!["src/Main.rs"]);

        // Sensitive: lowercase query only matches lowercase
        let sensitive = rank_items("'main", &items, CaseMode::Sensitive);
        assert_eq!(matched_strings(&items, &sensitive), vec!["src/main.rs"]);

        // Ignore: uppercase query matches both
        let ignore = rank_items("'Main", &items, CaseMode::Ignore);
        assert_eq!(ignore.len(), 2);
    }

    #[test]
    fn unicode_matching_and_character_indices() {
        let items = ["docs/café_résumé.md", "docs/other.md"];
        let results = rank_items("'café", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &results),
            vec!["docs/café_résumé.md"]
        );
        // Character indices (not UTF-8 byte offsets): "docs/" = 5 chars, "café" = indices 5..9
        assert_eq!(results[0].indices, vec![5, 6, 7, 8]);

        // Smart normalization: ASCII 'e' matches 'é'
        let norm_results = rank_items("'cafe", &items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&items, &norm_results),
            vec!["docs/café_résumé.md"]
        );
    }

    #[test]
    fn ranking_prefers_higher_score_then_shorter_path_then_original_index() {
        // Exact/tighter match should score higher than gapped match;
        // equal scores should break ties by shorter length, then original index.
        let items = ["a_b_c_d.rs", "bb_abc.rs", "aa_abc.rs", "abc.rs"];
        let results = rank_items("abc", &items, CaseMode::Smart);
        assert_eq!(results[0].index, 3); // "abc.rs" (highest score & shortest)
        assert_eq!(results.last().unwrap().index, 0); // "a_b_c_d.rs" (widest gaps -> lowest score)

        // Equal score tie-breaking: shorter length first, then original index
        let tie_items = ["zz_foo_long.rs", "foo_b.rs", "foo_a.rs"];
        let tie_results = rank_items("^foo", &tie_items, CaseMode::Smart);
        assert_eq!(
            matched_strings(&tie_items, &tie_results),
            vec!["foo_b.rs", "foo_a.rs"]
        );
    }

    #[test]
    fn reusable_engine_produces_consistent_results() {
        let mut engine = FuzzyEngine::new();
        let items = ["src/cli.rs", "src/fuzzy.rs", "src/walk.rs"];

        let r1 = engine.rank_items("cli", &items, CaseMode::Smart);
        let r2 = engine.rank_items("walk", &items, CaseMode::Smart);
        assert_eq!(matched_strings(&items, &r1), vec!["src/cli.rs"]);
        assert_eq!(matched_strings(&items, &r2), vec!["src/walk.rs"]);
    }
}
