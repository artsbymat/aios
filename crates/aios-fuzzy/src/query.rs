//! Extended `skim`-style query parser and composite match engines.
//!
//! Supported syntax:
//! - `foo`         : fuzzy subsequence match
//! - `'foo`        : exact contiguous substring match
//! - `^foo`        : prefix match
//! - `foo$`        : suffix match
//! - `^foo$`       : full-string exact match
//! - `!foo`        : inverse/negation (also `!'foo`, `!^foo`, `!foo$`, `!^foo$`)
//! - `foo bar`     : logical AND (scores sum, highlight indices merge)
//! - `foo | bar`   : logical OR (highest scoring branch wins)
//! - `foo\ bar`    : escaped space inside a single token

use std::fmt;

use crate::scorer::{
    SkimV2Matcher, fold_char, match_exact_full, match_exact_substring, match_prefix, match_suffix,
};
use crate::{CaseMatching, MatchEngine, MatchEngineFactory};

fn resolve_case(pattern: &str, mode: CaseMatching) -> bool {
    match mode {
        CaseMatching::Respect => true,
        CaseMatching::Ignore => false,
        CaseMatching::Smart => pattern.chars().any(char::is_uppercase),
    }
}

/// Matches every haystack with score `0` (used for empty queries).
#[derive(Debug, Default, Clone, Copy)]
pub struct MatchAllEngine;

impl MatchEngine for MatchAllEngine {
    fn match_text(&self, _haystack: &str) -> Option<(i64, Vec<usize>)> {
        Some((0, Vec::new()))
    }
}

/// Fuzzy match engine backed by [`SkimV2Matcher`].
#[derive(Debug, Clone)]
pub struct FuzzyEngine {
    matcher: SkimV2Matcher,
}

impl FuzzyEngine {
    #[must_use]
    pub fn new(pattern: &str, case: CaseMatching) -> Self {
        let sensitive = resolve_case(pattern, case);
        Self {
            matcher: SkimV2Matcher::new(pattern, sensitive),
        }
    }
}

impl MatchEngine for FuzzyEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        self.matcher.match_text(haystack)
    }
}

/// Exact contiguous substring engine (`'term`).
#[derive(Debug, Clone)]
pub struct ExactEngine {
    pattern_orig: Vec<char>,
    pattern_folded: Vec<char>,
    case_sensitive: bool,
}

impl ExactEngine {
    #[must_use]
    pub fn new(pattern: &str, case: CaseMatching) -> Self {
        let pattern_orig: Vec<char> = pattern.chars().collect();
        let pattern_folded: Vec<char> = pattern_orig.iter().map(|&c| fold_char(c)).collect();
        Self {
            pattern_orig,
            pattern_folded,
            case_sensitive: resolve_case(pattern, case),
        }
    }
}

impl MatchEngine for ExactEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        match_exact_substring(
            haystack,
            &self.pattern_orig,
            &self.pattern_folded,
            self.case_sensitive,
        )
    }
}

/// Prefix match engine (`^term`).
#[derive(Debug, Clone)]
pub struct PrefixEngine {
    pattern_orig: Vec<char>,
    pattern_folded: Vec<char>,
    case_sensitive: bool,
}

impl PrefixEngine {
    #[must_use]
    pub fn new(pattern: &str, case: CaseMatching) -> Self {
        let pattern_orig: Vec<char> = pattern.chars().collect();
        let pattern_folded: Vec<char> = pattern_orig.iter().map(|&c| fold_char(c)).collect();
        Self {
            pattern_orig,
            pattern_folded,
            case_sensitive: resolve_case(pattern, case),
        }
    }
}

impl MatchEngine for PrefixEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        match_prefix(
            haystack,
            &self.pattern_orig,
            &self.pattern_folded,
            self.case_sensitive,
        )
    }
}

/// Suffix match engine (`term$`).
#[derive(Debug, Clone)]
pub struct SuffixEngine {
    pattern_orig: Vec<char>,
    pattern_folded: Vec<char>,
    case_sensitive: bool,
}

impl SuffixEngine {
    #[must_use]
    pub fn new(pattern: &str, case: CaseMatching) -> Self {
        let pattern_orig: Vec<char> = pattern.chars().collect();
        let pattern_folded: Vec<char> = pattern_orig.iter().map(|&c| fold_char(c)).collect();
        Self {
            pattern_orig,
            pattern_folded,
            case_sensitive: resolve_case(pattern, case),
        }
    }
}

impl MatchEngine for SuffixEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        match_suffix(
            haystack,
            &self.pattern_orig,
            &self.pattern_folded,
            self.case_sensitive,
        )
    }
}

/// Full-string exact match engine (`^term$`).
#[derive(Debug, Clone)]
pub struct ExactFullEngine {
    pattern_orig: Vec<char>,
    pattern_folded: Vec<char>,
    case_sensitive: bool,
}

impl ExactFullEngine {
    #[must_use]
    pub fn new(pattern: &str, case: CaseMatching) -> Self {
        let pattern_orig: Vec<char> = pattern.chars().collect();
        let pattern_folded: Vec<char> = pattern_orig.iter().map(|&c| fold_char(c)).collect();
        Self {
            pattern_orig,
            pattern_folded,
            case_sensitive: resolve_case(pattern, case),
        }
    }
}

impl MatchEngine for ExactFullEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        match_exact_full(
            haystack,
            &self.pattern_orig,
            &self.pattern_folded,
            self.case_sensitive,
        )
    }
}

/// Negates an inner match engine (`!term`, `!'term`, `!^term`, `!term$`).
pub struct InverseEngine {
    inner: Box<dyn MatchEngine>,
}

impl fmt::Debug for InverseEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InverseEngine").finish_non_exhaustive()
    }
}

impl InverseEngine {
    #[must_use]
    pub fn new(inner: Box<dyn MatchEngine>) -> Self {
        Self { inner }
    }
}

impl MatchEngine for InverseEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        if self.inner.match_text(haystack).is_some() {
            None
        } else {
            Some((0, Vec::new()))
        }
    }
}

/// Requires all child engines to match; sums scores and merges highlight indices.
pub struct AndEngine {
    engines: Vec<Box<dyn MatchEngine>>,
}

impl fmt::Debug for AndEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AndEngine")
            .field("count", &self.engines.len())
            .finish()
    }
}

impl AndEngine {
    #[must_use]
    pub fn new(engines: Vec<Box<dyn MatchEngine>>) -> Self {
        Self { engines }
    }
}

impl MatchEngine for AndEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        let mut total_score: i64 = 0;
        let mut indices = Vec::new();

        for engine in &self.engines {
            let (score, part_indices) = engine.match_text(haystack)?;
            total_score = total_score.saturating_add(score);
            indices.extend(part_indices);
        }

        indices.sort_unstable();
        indices.dedup();
        Some((total_score, indices))
    }
}

/// Succeeds if any child engine matches, choosing the highest-scoring branch.
pub struct OrEngine {
    engines: Vec<Box<dyn MatchEngine>>,
}

impl fmt::Debug for OrEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrEngine")
            .field("count", &self.engines.len())
            .finish()
    }
}

impl OrEngine {
    #[must_use]
    pub fn new(engines: Vec<Box<dyn MatchEngine>>) -> Self {
        Self { engines }
    }
}

impl MatchEngine for OrEngine {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        let mut best: Option<(i64, Vec<usize>)> = None;
        for engine in &self.engines {
            if let Some(candidate) = engine.match_text(haystack)
                && best.as_ref().is_none_or(|b| candidate.0 > b.0)
            {
                best = Some(candidate);
            }
        }
        best
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RawToken {
    Or,
    Term(String),
}

/// Tokenizes raw user queries while preserving escaped spaces (`\ `).
fn tokenize(query: &str) -> Vec<RawToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = query.chars().peekable();
    let mut had_escape = false;

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek()
                && (next.is_whitespace() || next == '|' || next == '\\')
            {
                current.push(chars.next().unwrap_or(next));
                had_escape = true;
                continue;
            }
            current.push(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                if !had_escape && current == "|" {
                    tokens.push(RawToken::Or);
                } else {
                    tokens.push(RawToken::Term(std::mem::take(&mut current)));
                }
                current.clear();
                had_escape = false;
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        if !had_escape && current == "|" {
            tokens.push(RawToken::Or);
        } else {
            tokens.push(RawToken::Term(current));
        }
    }

    tokens
}

fn compile_term(raw: &str, case: CaseMatching) -> Box<dyn MatchEngine> {
    let (inverse, rest) = if let Some(stripped) = raw.strip_prefix('!')
        && !stripped.is_empty()
    {
        (true, stripped)
    } else {
        (false, raw)
    };

    let inner: Box<dyn MatchEngine> = if let Some(exact) = rest.strip_prefix('\'') {
        if exact.is_empty() {
            Box::new(MatchAllEngine)
        } else {
            Box::new(ExactEngine::new(exact, case))
        }
    } else {
        let has_prefix = rest.starts_with('^');
        let has_suffix = rest.ends_with('$');

        match (has_prefix, has_suffix) {
            (true, true) => {
                let inner_str = &rest[1..rest.len() - 1];
                Box::new(ExactFullEngine::new(inner_str, case))
            }
            (true, false) => {
                let inner_str = &rest[1..];
                if inner_str.is_empty() {
                    Box::new(MatchAllEngine)
                } else {
                    Box::new(PrefixEngine::new(inner_str, case))
                }
            }
            (false, true) => {
                let inner_str = &rest[..rest.len() - 1];
                if inner_str.is_empty() {
                    Box::new(MatchAllEngine)
                } else {
                    Box::new(SuffixEngine::new(inner_str, case))
                }
            }
            (false, false) => Box::new(FuzzyEngine::new(rest, case)),
        }
    };

    if inverse {
        Box::new(InverseEngine::new(inner))
    } else {
        inner
    }
}

/// Parses raw search input into a composite `AndEngine` / `OrEngine` tree.
#[derive(Debug, Default, Clone, Copy)]
pub struct QueryParser;

impl QueryParser {
    /// Compiles `query` into an executable [`MatchEngine`].
    ///
    /// Precedence follows `skim`/`fzf`: `|` binds tighter than whitespace `AND`,
    /// so `foo bar | baz` evaluates as `foo AND (bar OR baz)`.
    #[must_use]
    pub fn parse(query: &str, case: CaseMatching) -> Box<dyn MatchEngine> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Box::new(MatchAllEngine);
        }

        // Group adjacent terms connected by `RawToken::Or` into OR-clauses,
        // then combine all clauses with AND.
        let mut and_groups: Vec<Vec<String>> = Vec::new();
        let mut expect_or_rhs = false;

        for token in tokens {
            match token {
                RawToken::Or => {
                    if !and_groups.is_empty() {
                        expect_or_rhs = true;
                    }
                }
                RawToken::Term(term) => {
                    if expect_or_rhs {
                        if let Some(last_group) = and_groups.last_mut() {
                            last_group.push(term);
                        } else {
                            and_groups.push(vec![term]);
                        }
                        expect_or_rhs = false;
                    } else {
                        and_groups.push(vec![term]);
                    }
                }
            }
        }

        if and_groups.is_empty() {
            return Box::new(MatchAllEngine);
        }

        let mut compiled_and: Vec<Box<dyn MatchEngine>> = Vec::with_capacity(and_groups.len());
        for group in and_groups {
            if group.len() == 1 {
                compiled_and.push(compile_term(&group[0], case));
            } else {
                let branches = group.iter().map(|term| compile_term(term, case)).collect();
                compiled_and.push(Box::new(OrEngine::new(branches)));
            }
        }

        if compiled_and.len() == 1 {
            compiled_and
                .pop()
                .unwrap_or_else(|| Box::new(MatchAllEngine))
        } else {
            Box::new(AndEngine::new(compiled_and))
        }
    }
}

/// Default [`MatchEngineFactory`] compiling extended `skim`-style queries.
#[derive(Debug, Default, Clone, Copy)]
pub struct QueryEngineFactory;

impl MatchEngineFactory for QueryEngineFactory {
    fn create_engine(&self, query: &str, case: CaseMatching) -> Box<dyn MatchEngine> {
        QueryParser::parse(query, case)
    }
}
