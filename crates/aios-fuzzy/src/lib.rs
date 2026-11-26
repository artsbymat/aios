//! Clean-room fuzzy matching, query parsing, frecency ranking, and adaptive parallel
//! search engine for AIOS.
//!
//! Inspired by `skim`'s `SkimMatcherV2` scoring and query syntax alongside `instantMENU`'s
//! adaptive inline + progressive parallel execution architecture, implemented purely in Rust
//! with zero `skim` or `fuzzy-matcher` dependencies.

pub mod corpus;
pub mod frecency;
pub mod parallel;
pub mod query;
pub mod scorer;

pub use corpus::{PackedTextCorpus, PackedTextSnapshot};
pub use frecency::{
    FrecencyConfig, FrecencyRecord, compute_frecency_boost, decay_weight, score_item,
};
pub use parallel::{
    CancellationToken, FrecencyMap, PackedSearchSession, RankedItem, SearchConfig, SearchHandle,
    SearchSession, SearchSnapshot, search_packed_parallel, search_parallel,
};
pub use query::{
    AndEngine, ExactEngine, ExactFullEngine, FuzzyEngine, InverseEngine, MatchAllEngine, OrEngine,
    PrefixEngine, QueryEngineFactory, QueryParser, SuffixEngine,
};
pub use scorer::SkimV2Matcher;

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

impl SearchItem for String {
    #[inline]
    fn id(&self) -> &str {
        self.as_str()
    }

    #[inline]
    fn primary_text(&self) -> &str {
        self.as_str()
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
