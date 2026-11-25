//! Clean-room `SkimV2`-inspired Smith-Waterman DP fuzzy scorer and exact/affix matchers.
//!
//! Uses a fast zero-allocation subsequence pre-filter to reject non-matching candidates
//! in O(n), followed by affine-gap dynamic programming over the narrowed `[first..=last]`
//! window with thread-local scratch buffer reuse.

use std::cell::RefCell;

pub const SCORE_MATCH: i32 = 16;
pub const BONUS_CONSECUTIVE: i32 = 4;
pub const PENALTY_GAP_START: i32 = -3;
pub const PENALTY_GAP_EXTENSION: i32 = -1;
pub const BONUS_EXACT_CASE: i32 = 1;

pub const BONUS_BOUNDARY_START: i32 = 10;
pub const BONUS_BOUNDARY_WHITESPACE: i32 = 8;
pub const BONUS_BOUNDARY_DELIMITER: i32 = 7;
pub const BONUS_CAMEL_CASE: i32 = 6;
pub const BONUS_DIGIT_TRANSITION: i32 = 4;

const NEG_INF: i32 = i32::MIN / 4;
/// Upper bound on `pattern_len * window_len` for full DP matrix evaluation.
/// Very large haystacks (e.g. megabyte clipboard text) fall back to greedy window matching
/// so memory stays bounded.
const MAX_DP_CELLS: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Whitespace,
    Delimiter,
    Lower,
    Upper,
    Digit,
    Other,
}

#[inline]
fn classify_char(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Whitespace
    } else if matches!(ch, '/' | '\\' | ':' | ';' | '-' | '_' | '.' | '|') {
        CharClass::Delimiter
    } else if ch.is_lowercase() {
        CharClass::Lower
    } else if ch.is_uppercase() {
        CharClass::Upper
    } else if ch.is_ascii_digit() || ch.is_numeric() {
        CharClass::Digit
    } else {
        CharClass::Other
    }
}

#[inline]
fn boundary_bonus(prev: Option<CharClass>, curr: CharClass) -> i32 {
    match prev {
        None => BONUS_BOUNDARY_START,
        Some(CharClass::Whitespace) if curr != CharClass::Whitespace => BONUS_BOUNDARY_WHITESPACE,
        Some(CharClass::Delimiter)
            if curr != CharClass::Delimiter && curr != CharClass::Whitespace =>
        {
            BONUS_BOUNDARY_DELIMITER
        }
        Some(CharClass::Lower) if curr == CharClass::Upper => BONUS_CAMEL_CASE,
        Some(p)
            if curr == CharClass::Digit
                && !matches!(
                    p,
                    CharClass::Digit | CharClass::Whitespace | CharClass::Delimiter
                ) =>
        {
            BONUS_DIGIT_TRANSITION
        }
        _ => 0,
    }
}

/// Folds a single Unicode scalar to lowercase while preserving 1-to-1 character indexing.
#[inline]
pub(crate) fn fold_char(ch: char) -> char {
    if ch.is_ascii() {
        ch.to_ascii_lowercase()
    } else {
        ch.to_lowercase().next().unwrap_or(ch)
    }
}

#[inline]
fn chars_match(h: char, p_orig: char, p_folded: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        h == p_orig
    } else {
        fold_char(h) == p_folded
    }
}

/// Reusable per-thread buffers to eliminate heap allocations during hot-path matching.
#[derive(Debug, Default)]
struct DpScratch {
    hay_window: Vec<char>,
    bonuses: Vec<i32>,
    m_table: Vec<i32>,
    p_table: Vec<i32>,
    streak_table: Vec<i32>,
    from_match: Vec<u8>,
    gap_from_match: Vec<u8>,
}

impl DpScratch {
    fn prepare_tables(&mut self, rows: usize, cols: usize) {
        let total = rows * cols;
        self.m_table.clear();
        self.m_table.resize(total, NEG_INF);
        self.p_table.clear();
        self.p_table.resize(total, NEG_INF);
        self.streak_table.clear();
        self.streak_table.resize(total, 0);
        self.from_match.clear();
        self.from_match.resize(total, 0);
        self.gap_from_match.clear();
        self.gap_from_match.resize(total, 0);
    }
}

thread_local! {
    static DP_SCRATCH: RefCell<DpScratch> = RefCell::new(DpScratch::default());
}

/// Pre-compiled fuzzy pattern ready for repeated matching across many haystacks.
#[derive(Debug, Clone)]
pub struct SkimV2Matcher {
    pattern_orig: Vec<char>,
    pattern_folded: Vec<char>,
    case_sensitive: bool,
}

impl SkimV2Matcher {
    /// Compiles a fuzzy pattern with explicit case sensitivity.
    #[must_use]
    pub fn new(pattern: &str, case_sensitive: bool) -> Self {
        let pattern_orig: Vec<char> = pattern.chars().collect();
        let pattern_folded: Vec<char> = pattern_orig.iter().map(|&c| fold_char(c)).collect();
        Self {
            pattern_orig,
            pattern_folded,
            case_sensitive,
        }
    }

    /// Evaluates the fuzzy match against `haystack`, returning `(score, matched_char_indices)`.
    #[must_use]
    pub fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)> {
        let m = self.pattern_orig.len();
        if m == 0 {
            return Some((0, Vec::new()));
        }

        // Step 1: Fast O(n) zero-allocation subsequence pre-filter that finds the exact
        // outer bounds `[first_match..=last_match]` containing all possible optimal alignments.
        let (first_match, last_match, prev_before_first) =
            self.find_subsequence_bounds(haystack)?;
        let window_len = last_match - first_match + 1;

        if m == 1 {
            return self.match_single_char(haystack, first_match, last_match, prev_before_first);
        }

        if m.saturating_mul(window_len) > MAX_DP_CELLS {
            return self.match_greedy_fallback(haystack, first_match);
        }

        DP_SCRATCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            self.run_smith_waterman(
                haystack,
                first_match,
                last_match,
                prev_before_first,
                &mut scratch,
            )
        })
    }

    /// Scans `haystack` once to verify all pattern chars appear in order and locate:
    /// - `first_match`: earliest char index matching `pattern[0]`
    /// - `last_match`: latest char index matching `pattern[m - 1]` at or after the first full match
    /// - `prev_before_first`: the character immediately preceding `first_match`, if any
    fn find_subsequence_bounds(&self, haystack: &str) -> Option<(usize, usize, Option<char>)> {
        let m = self.pattern_orig.len();
        let mut p_idx = 0usize;
        let mut first_match = 0usize;
        let mut last_match = 0usize;
        let mut prev_char: Option<char> = None;
        let mut prev_before_first: Option<char> = None;

        let last_p_orig = self.pattern_orig[m - 1];
        let last_p_folded = self.pattern_folded[m - 1];

        for (h_idx, ch) in haystack.chars().enumerate() {
            if p_idx < m {
                if chars_match(
                    ch,
                    self.pattern_orig[p_idx],
                    self.pattern_folded[p_idx],
                    self.case_sensitive,
                ) {
                    if p_idx == 0 {
                        first_match = h_idx;
                        prev_before_first = prev_char;
                    }
                    p_idx += 1;
                    if p_idx == m {
                        last_match = h_idx;
                    }
                }
            } else if chars_match(ch, last_p_orig, last_p_folded, self.case_sensitive) {
                last_match = h_idx;
            }
            prev_char = Some(ch);
        }

        if p_idx == m {
            Some((first_match, last_match, prev_before_first))
        } else {
            None
        }
    }

    fn match_single_char(
        &self,
        haystack: &str,
        first_match: usize,
        last_match: usize,
        prev_before_first: Option<char>,
    ) -> Option<(i64, Vec<usize>)> {
        let p_orig = self.pattern_orig[0];
        let p_folded = self.pattern_folded[0];
        let mut prev_class = prev_before_first.map(classify_char);

        let mut best_score = i32::MIN;
        let mut best_idx = first_match;

        for (offset, ch) in haystack
            .chars()
            .skip(first_match)
            .take(last_match - first_match + 1)
            .enumerate()
        {
            let curr_class = classify_char(ch);
            let h_idx = first_match + offset;
            if chars_match(ch, p_orig, p_folded, self.case_sensitive) {
                let b = boundary_bonus(prev_class, curr_class);
                let exact_case = if ch == p_orig { BONUS_EXACT_CASE } else { 0 };
                let leading_penalty = -(h_idx.min(3) as i32);
                let score = SCORE_MATCH + b * 2 + exact_case + leading_penalty;
                if score > best_score {
                    best_score = score;
                    best_idx = h_idx;
                }
            }
            prev_class = Some(curr_class);
        }

        Some((i64::from(best_score), vec![best_idx]))
    }

    fn run_smith_waterman(
        &self,
        haystack: &str,
        first_match: usize,
        last_match: usize,
        prev_before_first: Option<char>,
        scratch: &mut DpScratch,
    ) -> Option<(i64, Vec<usize>)> {
        let m = self.pattern_orig.len();
        let w = last_match - first_match + 1;

        // Populate narrowed character window and positional boundary bonuses `B[j]`.
        scratch.hay_window.clear();
        scratch.bonuses.clear();
        scratch.hay_window.reserve(w);
        scratch.bonuses.reserve(w);

        let mut prev_class = prev_before_first.map(classify_char);
        for ch in haystack.chars().skip(first_match).take(w) {
            let curr_class = classify_char(ch);
            scratch.hay_window.push(ch);
            scratch.bonuses.push(boundary_bonus(prev_class, curr_class));
            prev_class = Some(curr_class);
        }

        scratch.prepare_tables(m, w);

        // Row 0: matching pattern[0].
        let p0_orig = self.pattern_orig[0];
        let p0_folded = self.pattern_folded[0];
        for j in 0..w {
            let ch = scratch.hay_window[j];
            if chars_match(ch, p0_orig, p0_folded, self.case_sensitive) {
                let b = scratch.bonuses[j];
                let exact_case = if ch == p0_orig { BONUS_EXACT_CASE } else { 0 };
                let leading_penalty = -((first_match + j).min(3) as i32);
                scratch.m_table[j] = SCORE_MATCH + b * 2 + exact_case + leading_penalty;
                scratch.streak_table[j] = b.max(BONUS_CONSECUTIVE);
            }
            if j > 0 {
                let open_gap = scratch.m_table[j - 1] + PENALTY_GAP_START + PENALTY_GAP_EXTENSION;
                let ext_gap = scratch.p_table[j - 1] + PENALTY_GAP_EXTENSION;
                if open_gap >= ext_gap {
                    scratch.p_table[j] = open_gap;
                    scratch.gap_from_match[j] = 1;
                } else {
                    scratch.p_table[j] = ext_gap;
                    scratch.gap_from_match[j] = 0;
                }
            }
        }

        // Rows 1..m: affine gap Smith-Waterman recurrence.
        for i in 1..m {
            let p_orig = self.pattern_orig[i];
            let p_folded = self.pattern_folded[i];
            let prev_row = (i - 1) * w;
            let curr_row = i * w;

            for j in i..w {
                let ch = scratch.hay_window[j];
                if chars_match(ch, p_orig, p_folded, self.case_sensitive) {
                    let b = scratch.bonuses[j];
                    let exact_case = if ch == p_orig { BONUS_EXACT_CASE } else { 0 };

                    let prev_m = scratch.m_table[prev_row + j - 1];
                    let prev_p = scratch.p_table[prev_row + j - 1];

                    let (score_from_match, streak_from_match) = if prev_m > NEG_INF / 2 {
                        let cons_bonus = scratch.streak_table[prev_row + j - 1]
                            .max(b)
                            .max(BONUS_CONSECUTIVE);
                        (prev_m + SCORE_MATCH + cons_bonus + exact_case, cons_bonus)
                    } else {
                        (NEG_INF, 0)
                    };

                    let (score_from_gap, streak_from_gap) = if prev_p > NEG_INF / 2 {
                        (
                            prev_p + SCORE_MATCH + b + exact_case,
                            b.max(BONUS_CONSECUTIVE),
                        )
                    } else {
                        (NEG_INF, 0)
                    };

                    if score_from_match >= score_from_gap && score_from_match > NEG_INF / 2 {
                        scratch.m_table[curr_row + j] = score_from_match;
                        scratch.streak_table[curr_row + j] = streak_from_match;
                        scratch.from_match[curr_row + j] = 1;
                    } else if score_from_gap > NEG_INF / 2 {
                        scratch.m_table[curr_row + j] = score_from_gap;
                        scratch.streak_table[curr_row + j] = streak_from_gap;
                        scratch.from_match[curr_row + j] = 0;
                    }
                }

                let open_gap =
                    scratch.m_table[curr_row + j - 1] + PENALTY_GAP_START + PENALTY_GAP_EXTENSION;
                let ext_gap = scratch.p_table[curr_row + j - 1] + PENALTY_GAP_EXTENSION;
                if open_gap >= ext_gap {
                    scratch.p_table[curr_row + j] = open_gap;
                    scratch.gap_from_match[curr_row + j] = 1;
                } else {
                    scratch.p_table[curr_row + j] = ext_gap;
                    scratch.gap_from_match[curr_row + j] = 0;
                }
            }
        }

        // Step 4: Locate best terminal alignment on row `m - 1` and backtrack exact indices.
        let last_row = (m - 1) * w;
        let mut best_score = NEG_INF;
        let mut best_j = 0usize;
        for j in (m - 1)..w {
            let s = scratch.m_table[last_row + j];
            if s > best_score {
                best_score = s;
                best_j = j;
            }
        }

        if best_score <= NEG_INF / 2 {
            return None;
        }

        let mut indices = Vec::with_capacity(m);
        let mut i = m - 1;
        let mut j = best_j;
        let mut in_match = true;

        loop {
            if in_match {
                indices.push(first_match + j);
                if i == 0 {
                    break;
                }
                let took_match = scratch.from_match[i * w + j] != 0;
                i -= 1;
                j -= 1;
                in_match = took_match;
            } else {
                let opened_from_match = scratch.gap_from_match[i * w + j] != 0;
                if j == 0 {
                    break;
                }
                j -= 1;
                in_match = opened_from_match;
            }
        }

        indices.reverse();
        Some((i64::from(best_score), indices))
    }

    fn match_greedy_fallback(
        &self,
        haystack: &str,
        first_match: usize,
    ) -> Option<(i64, Vec<usize>)> {
        let m = self.pattern_orig.len();
        let mut indices = Vec::with_capacity(m);
        let mut p_idx = 0usize;
        let mut score: i64 = 0;
        let mut prev_matched: Option<usize> = None;

        for (offset, ch) in haystack.chars().skip(first_match).enumerate() {
            if chars_match(
                ch,
                self.pattern_orig[p_idx],
                self.pattern_folded[p_idx],
                self.case_sensitive,
            ) {
                let h_idx = first_match + offset;
                indices.push(h_idx);
                score += i64::from(SCORE_MATCH);
                if let Some(prev) = prev_matched {
                    if h_idx == prev + 1 {
                        score += i64::from(BONUS_CONSECUTIVE);
                    } else {
                        let gap = (h_idx - prev - 1) as i64;
                        score +=
                            i64::from(PENALTY_GAP_START) + gap * i64::from(PENALTY_GAP_EXTENSION);
                    }
                }
                prev_matched = Some(h_idx);
                p_idx += 1;
                if p_idx == m {
                    return Some((score, indices));
                }
            }
        }

        None
    }
}

/// Scores a contiguous matched range `[start..start + pattern.len()]` consistently with DP scores.
pub(crate) fn score_contiguous_span(
    hay_chars: &[char],
    start: usize,
    pattern_orig: &[char],
) -> (i64, Vec<usize>) {
    let m = pattern_orig.len();
    if m == 0 {
        return (0, Vec::new());
    }

    let mut score: i32 = 0;
    let mut indices = Vec::with_capacity(m);
    let mut prev_class = if start > 0 {
        Some(classify_char(hay_chars[start - 1]))
    } else {
        None
    };

    let mut streak_bonus = BONUS_CONSECUTIVE;
    for (offset, &p_ch) in pattern_orig.iter().enumerate() {
        let idx = start + offset;
        let h_ch = hay_chars[idx];
        let curr_class = classify_char(h_ch);
        let b = boundary_bonus(prev_class, curr_class);
        let exact_case = if h_ch == p_ch { BONUS_EXACT_CASE } else { 0 };

        if offset == 0 {
            let leading_penalty = -(start.min(3) as i32);
            score += SCORE_MATCH + b * 2 + exact_case + leading_penalty;
            streak_bonus = b.max(BONUS_CONSECUTIVE);
        } else {
            streak_bonus = streak_bonus.max(b).max(BONUS_CONSECUTIVE);
            score += SCORE_MATCH + streak_bonus + exact_case;
        }

        indices.push(idx);
        prev_class = Some(curr_class);
    }

    (i64::from(score), indices)
}

/// Evaluates an exact contiguous substring match (`'term`), picking the highest-scoring occurrence.
#[must_use]
pub fn match_exact_substring(
    haystack: &str,
    pattern_orig: &[char],
    pattern_folded: &[char],
    case_sensitive: bool,
) -> Option<(i64, Vec<usize>)> {
    let m = pattern_orig.len();
    if m == 0 {
        return Some((0, Vec::new()));
    }

    let hay_chars: Vec<char> = haystack.chars().collect();
    if hay_chars.len() < m {
        return None;
    }

    let mut best: Option<(i64, Vec<usize>)> = None;
    for start in 0..=(hay_chars.len() - m) {
        let matched = (0..m).all(|k| {
            chars_match(
                hay_chars[start + k],
                pattern_orig[k],
                pattern_folded[k],
                case_sensitive,
            )
        });
        if matched {
            let candidate = score_contiguous_span(&hay_chars, start, pattern_orig);
            if best.as_ref().is_none_or(|b| candidate.0 > b.0) {
                best = Some(candidate);
            }
        }
    }

    best
}

/// Evaluates a strict prefix match (`^term`).
#[must_use]
pub fn match_prefix(
    haystack: &str,
    pattern_orig: &[char],
    pattern_folded: &[char],
    case_sensitive: bool,
) -> Option<(i64, Vec<usize>)> {
    let m = pattern_orig.len();
    if m == 0 {
        return Some((0, Vec::new()));
    }

    let hay_chars: Vec<char> = haystack.chars().take(m).collect();
    if hay_chars.len() < m {
        return None;
    }

    let matched = (0..m).all(|k| {
        chars_match(
            hay_chars[k],
            pattern_orig[k],
            pattern_folded[k],
            case_sensitive,
        )
    });
    if matched {
        Some(score_contiguous_span(&hay_chars, 0, pattern_orig))
    } else {
        None
    }
}

/// Evaluates a strict suffix match (`term$`).
#[must_use]
pub fn match_suffix(
    haystack: &str,
    pattern_orig: &[char],
    pattern_folded: &[char],
    case_sensitive: bool,
) -> Option<(i64, Vec<usize>)> {
    let m = pattern_orig.len();
    if m == 0 {
        return Some((0, Vec::new()));
    }

    let hay_chars: Vec<char> = haystack.chars().collect();
    if hay_chars.len() < m {
        return None;
    }

    let start = hay_chars.len() - m;
    let matched = (0..m).all(|k| {
        chars_match(
            hay_chars[start + k],
            pattern_orig[k],
            pattern_folded[k],
            case_sensitive,
        )
    });
    if matched {
        Some(score_contiguous_span(&hay_chars, start, pattern_orig))
    } else {
        None
    }
}

/// Evaluates a strict full-string match (`^term$`).
#[must_use]
pub fn match_exact_full(
    haystack: &str,
    pattern_orig: &[char],
    pattern_folded: &[char],
    case_sensitive: bool,
) -> Option<(i64, Vec<usize>)> {
    let hay_chars: Vec<char> = haystack.chars().collect();
    if hay_chars.len() != pattern_orig.len() {
        return None;
    }
    if hay_chars.is_empty() {
        return Some((0, Vec::new()));
    }

    let matched = (0..hay_chars.len()).all(|k| {
        chars_match(
            hay_chars[k],
            pattern_orig[k],
            pattern_folded[k],
            case_sensitive,
        )
    });
    if matched {
        Some(score_contiguous_span(&hay_chars, 0, pattern_orig))
    } else {
        None
    }
}
