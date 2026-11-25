//! Multi-field item scoring and frecency blending.
//!
//! Combines `primary_text`, `secondary_text`, `keywords`, explicit `alias`,
//! pin state, and time-decayed usage history into a unified ranking score.

use crate::{MatchEngine, MatchResult, SearchItem};

/// Historical usage record for a single searchable item (loaded from SQLite `frecency`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrecencyRecord {
    /// Total number of times the item has been selected.
    pub hits: u32,
    /// Unix timestamp in seconds when the item was last selected.
    pub last_used_at_secs: i64,
    /// Whether the user explicitly pinned this item to the top.
    pub pinned: bool,
    /// Number of times this item was chosen for the current query prefix.
    pub query_prefix_hits: u32,
}

/// Tunable weights for multi-field matching and frecency decay.
#[derive(Debug, Clone, PartialEq)]
pub struct FrecencyConfig {
    pub alias_exact_boost: i64,
    pub alias_prefix_boost: i64,
    pub pin_boost: i64,
    pub frecency_scale: f64,
    pub max_frecency_boost: i64,
    pub prefix_affinity_scale: f64,
    pub max_prefix_affinity_boost: i64,
    pub secondary_weight: f64,
    pub keyword_weight: f64,
    pub combined_field_weight: f64,
}

impl Default for FrecencyConfig {
    fn default() -> Self {
        Self {
            alias_exact_boost: 10_000,
            alias_prefix_boost: 4_000,
            pin_boost: 2_000,
            frecency_scale: 80.0,
            max_frecency_boost: 600,
            prefix_affinity_scale: 45.0,
            max_prefix_affinity_boost: 300,
            secondary_weight: 0.7,
            keyword_weight: 0.6,
            combined_field_weight: 0.85,
        }
    }
}

/// Returns the exponential-step decay weight based on age since last use.
#[must_use]
pub fn decay_weight(age_secs: i64) -> f64 {
    const FOUR_HOURS: i64 = 4 * 3_600;
    const ONE_DAY: i64 = 24 * 3_600;
    const SEVEN_DAYS: i64 = 7 * 86_400;
    const THIRTY_DAYS: i64 = 30 * 86_400;

    if age_secs < FOUR_HOURS {
        1.0
    } else if age_secs < ONE_DAY {
        0.8
    } else if age_secs < SEVEN_DAYS {
        0.6
    } else if age_secs < THIRTY_DAYS {
        0.35
    } else {
        0.15
    }
}

/// Computes the bounded frecency + query-prefix affinity boost for a [`FrecencyRecord`].
#[must_use]
pub fn compute_frecency_boost(
    record: &FrecencyRecord,
    now_secs: i64,
    config: &FrecencyConfig,
) -> i64 {
    let base_boost = if record.hits > 0 {
        let age_secs = (now_secs - record.last_used_at_secs).max(0);
        let weight = decay_weight(age_secs);
        let raw = (1.0 + f64::from(record.hits)).ln() * weight * config.frecency_scale;
        (raw as i64).clamp(0, config.max_frecency_boost)
    } else {
        0
    };

    let affinity_boost = if record.query_prefix_hits > 0 {
        let raw = (1.0 + f64::from(record.query_prefix_hits)).ln() * config.prefix_affinity_scale;
        (raw as i64).clamp(0, config.max_prefix_affinity_boost)
    } else {
        0
    };

    base_boost + affinity_boost
}

/// Evaluates `item` across `alias`, `primary_text`, `secondary_text`, and `keywords`,
/// blending text match score with pin and frecency signals.
#[must_use]
pub fn score_item<T: SearchItem + ?Sized>(
    item: &T,
    query: &str,
    engine: &dyn MatchEngine,
    frecency: Option<&FrecencyRecord>,
    now_secs: i64,
    config: &FrecencyConfig,
) -> Option<MatchResult> {
    let pin_boost = if frecency.is_some_and(|r| r.pinned) {
        config.pin_boost
    } else {
        0
    };
    let frecency_boost = frecency.map_or(0, |r| compute_frecency_boost(r, now_secs, config));

    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Some(MatchResult {
            score: pin_boost + frecency_boost,
            primary_indices: Vec::new(),
            secondary_indices: Vec::new(),
        });
    }

    // 1. Explicit alias check (deterministic recall for shortcuts like `cb` -> Clipboard History).
    let alias_boost = item.alias().map_or(0, |alias| {
        if alias.eq_ignore_ascii_case(trimmed_query)
            || alias.to_lowercase() == trimmed_query.to_lowercase()
        {
            config.alias_exact_boost
        } else if trimmed_query.chars().count() >= 2
            && alias
                .to_lowercase()
                .starts_with(&trimmed_query.to_lowercase())
        {
            config.alias_prefix_boost
        } else {
            0
        }
    });

    // 2. Evaluate primary_text, secondary_text, and keywords individually.
    let primary_match = engine.match_text(item.primary_text());
    let secondary_match = item.secondary_text().and_then(|s| engine.match_text(s));
    let best_keyword_score = item
        .keywords()
        .iter()
        .filter_map(|kw| engine.match_text(kw).map(|(score, _)| score))
        .max();

    let mut best_text_score: Option<i64> = None;
    let mut primary_indices = Vec::new();
    let mut secondary_indices = Vec::new();

    if let Some((p_score, p_idx)) = primary_match {
        best_text_score = Some(p_score);
        primary_indices = p_idx;
    }

    if let Some((s_score, s_idx)) = secondary_match {
        let weighted = (s_score as f64 * config.secondary_weight) as i64;
        secondary_indices = s_idx;
        best_text_score = Some(best_text_score.map_or(weighted, |cur| cur.max(weighted)));
    }

    if let Some(kw_score) = best_keyword_score {
        let weighted = (kw_score as f64 * config.keyword_weight) as i64;
        best_text_score = Some(best_text_score.map_or(weighted, |cur| cur.max(weighted)));
    }

    // 3. Cross-field fallback: if a multi-term query spans `primary_text` + `secondary_text`
    //    or `keywords` (e.g. `"firefox browser"`), evaluate against a combined candidate string
    //    and project highlight indices back onto primary and secondary fields.
    if best_text_score.is_none()
        && alias_boost == 0
        && (item.secondary_text().is_some() || !item.keywords().is_empty())
    {
        let primary = item.primary_text();
        let primary_char_len = primary.chars().count();
        let mut combined = String::from(primary);

        let sec_range = if let Some(sec) = item.secondary_text() {
            combined.push(' ');
            let start = primary_char_len + 1;
            let len = sec.chars().count();
            combined.push_str(sec);
            Some((start, start + len))
        } else {
            None
        };

        for kw in item.keywords() {
            combined.push(' ');
            combined.push_str(kw);
        }

        if let Some((combined_score, combined_indices)) = engine.match_text(&combined) {
            let weighted = (combined_score as f64 * config.combined_field_weight) as i64;
            best_text_score = Some(weighted);
            for idx in combined_indices {
                if idx < primary_char_len {
                    primary_indices.push(idx);
                } else if let Some((sec_start, sec_end)) = sec_range
                    && idx >= sec_start
                    && idx < sec_end
                {
                    secondary_indices.push(idx - sec_start);
                }
            }
        }
    }

    if best_text_score.is_none() && alias_boost == 0 {
        return None;
    }

    let final_score = best_text_score
        .unwrap_or(0)
        .saturating_add(alias_boost)
        .saturating_add(frecency_boost)
        .saturating_add(pin_boost);

    Some(MatchResult {
        score: final_score,
        primary_indices,
        secondary_indices,
    })
}
