//! Clean-room fuzzy matching and ranking engine for AIOS.
//!
//! Full Smith-Waterman DP scoring, query syntax parsing, and frecency blending
//! are scheduled for Phase 1 (see `docs/FUZZY_SPEC.md`).

/// A searchable entry indexed by AIOS Root Search or a plugin view.
pub trait SearchItem: Send + Sync {
    fn id(&self) -> &str;
    fn primary_text(&self) -> &str;
    fn secondary_text(&self) -> Option<&str> {
        None
    }
    fn keywords(&self) -> &[String] {
        &[]
    }
    fn alias(&self) -> Option<&str> {
        None
    }
}

/// Match score and character highlight indices returned by a match engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    pub score: i64,
    pub primary_indices: Vec<usize>,
    pub secondary_indices: Vec<usize>,
}

/// Controls case sensitivity during query evaluation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaseMatching {
    #[default]
    Smart,
    Ignore,
    Respect,
}

/// Evaluates a single compiled query against candidate text.
pub trait MatchEngine: Send + Sync {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)>;
}

/// Compiles raw user queries into executable match engines.
pub trait MatchEngineFactory: Send + Sync {
    fn create_engine(&self, query: &str, case: CaseMatching) -> Box<dyn MatchEngine>;
}
