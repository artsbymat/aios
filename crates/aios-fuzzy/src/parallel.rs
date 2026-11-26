//! Adaptive parallel search engine modeled on `instantMENU`'s parallel search/filter/render
//! pipeline.
//!
//! Architecture:
//! 1. **Inline Fast Path**: Small corpora or queries that finish within `inline_budget`
//!    (default `1.5ms`) complete synchronously on the caller thread with zero thread-dispatch
//!    overhead and single-frame 0-flicker latency.
//! 2. **Parallel Background Handoff (`rayon`)**: When items remain after the inline budget
//!    expires, the caller immediately receives the partial [`SearchSnapshot`] so GPUI can render,
//!    while a background coordinator processes the remainder in parallel chunks across CPU cores.
//! 3. **Progressive Top-K Snapshots**: Macro-batches merge bounded Top-K min-heaps and push
//!    updated [`SearchSnapshot`]s over a channel so rendering and searching run concurrently.
//! 4. **Latest-Wins Replacement & Atomic Cancellation**: Each keystroke bumps an `AtomicU64`
//!    generation and overwrites any pending query slot; workers check cancellation every
//!    `cancel_check_interval` items (default `256`).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::corpus::PackedTextSnapshot;
use crate::frecency::{FrecencyConfig, FrecencyRecord, score_item};
use crate::query::QueryParser;
use crate::{CaseMatching, MatchEngine, MatchResult, SearchItem};

/// Lookup map from `SearchItem::id()` to its historical [`FrecencyRecord`].
pub type FrecencyMap = HashMap<String, FrecencyRecord>;

/// Lightweight lock-free token polled by worker chunks to abort superseded searches.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    current: Arc<AtomicU64>,
    expected: u64,
}

impl CancellationToken {
    /// Creates a token tied to `current` that remains active while `current == expected`.
    #[must_use]
    pub fn new(current: Arc<AtomicU64>, expected: u64) -> Self {
        Self { current, expected }
    }

    /// Returns `true` if a newer query generation has superseded this search.
    #[inline]
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.current.load(AtomicOrdering::Relaxed) != self.expected
    }

    /// Returns the generation identifier associated with this token.
    #[inline]
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.expected
    }
}

/// Configuration for adaptive inline vs. parallel background search execution.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Maximum number of top-ranked items retained per snapshot (default: `200`).
    pub limit: usize,
    /// Maximum time spent inline on the caller thread before handing off to background workers.
    pub inline_budget: Duration,
    /// Small corpora at or below this count always complete inline without spawning worker handoffs.
    pub inline_max_items: usize,
    /// Number of items scored per parallel macro-batch before emitting a progressive snapshot.
    pub macro_batch_size: usize,
    /// Sub-chunk granularity passed to Rayon work-stealing workers.
    pub rayon_chunk_size: usize,
    /// Poll frequency for checking [`CancellationToken::is_cancelled`] inside each chunk.
    pub cancel_check_interval: usize,
    /// Case sensitivity mode for query evaluation.
    pub case_matching: CaseMatching,
    /// Field weighting and frecency decay parameters.
    pub frecency: FrecencyConfig,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            limit: 200,
            inline_budget: Duration::from_micros(1_500),
            inline_max_items: 1_024,
            macro_batch_size: 8_192,
            rayon_chunk_size: 512,
            cancel_check_interval: 256,
            case_matching: CaseMatching::Smart,
            frecency: FrecencyConfig::default(),
        }
    }
}

/// A matched entry referencing its index into the immutable corpus slice `Arc<[T]>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedItem {
    pub item_index: usize,
    pub result: MatchResult,
}

/// Immutable snapshot of ranked matches delivered to the UI render loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSnapshot {
    /// Monotonic query generation. UI ignores snapshots older than its active generation.
    pub generation: u64,
    /// Top-K ranked items sorted best-first.
    pub matches: Vec<RankedItem>,
    /// Total number of items matched so far (may exceed `matches.len()` when capped at `limit`).
    pub total_matched: usize,
    /// Number of corpus items evaluated so far.
    pub processed_items: usize,
    /// Total items in the corpus snapshot.
    pub total_items: usize,
    /// `true` once the entire corpus has been scored; `false` while background batches are running.
    pub is_complete: bool,
}

/// Result returned immediately from [`SearchSession::search`].
#[derive(Debug)]
pub struct SearchHandle {
    /// Immediate snapshot computed on the inline fast path (already complete for small lists).
    pub initial: SearchSnapshot,
    /// Progressive update stream if work was handed off to background parallel workers.
    pub updates: Option<mpsc::Receiver<SearchSnapshot>>,
}

/// Internal key capturing deterministic tie-break metadata so heaps need no reference to `&[T]`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SortKey {
    score: i64,
    primary_char_len: usize,
    first_match_index: usize,
    primary_text: String,
    id: String,
    item_index: usize,
}

impl SortKey {
    fn from_item<T: SearchItem>(item: &T, item_index: usize, result: &MatchResult) -> Self {
        Self {
            score: result.score,
            primary_char_len: item.primary_text().chars().count(),
            first_match_index: result
                .primary_indices
                .first()
                .copied()
                .unwrap_or(usize::MAX),
            primary_text: item.primary_text().to_owned(),
            id: item.id().to_owned(),
            item_index,
        }
    }

    /// Zero-allocation comparison of a borrowed candidate against an existing `SortKey`.
    #[inline]
    fn cmp_candidate_best_first<T: SearchItem>(
        item: &T,
        item_index: usize,
        result: &MatchResult,
        worst: &Self,
    ) -> Ordering {
        worst
            .score
            .cmp(&result.score)
            .then_with(|| {
                let len = item.primary_text().chars().count();
                len.cmp(&worst.primary_char_len)
            })
            .then_with(|| {
                let first = result
                    .primary_indices
                    .first()
                    .copied()
                    .unwrap_or(usize::MAX);
                first.cmp(&worst.first_match_index)
            })
            .then_with(|| item.primary_text().cmp(worst.primary_text.as_str()))
            .then_with(|| item.id().cmp(worst.id.as_str()))
            .then_with(|| item_index.cmp(&worst.item_index))
    }

    /// Best-first ordering (smaller `Ordering::Less` means better rank).
    fn cmp_best_first(&self, other: &Self) -> Ordering {
        other
            .score
            .cmp(&self.score)
            .then_with(|| self.primary_char_len.cmp(&other.primary_char_len))
            .then_with(|| self.first_match_index.cmp(&other.first_match_index))
            .then_with(|| self.primary_text.cmp(&other.primary_text))
            .then_with(|| self.id.cmp(&other.id))
            .then_with(|| self.item_index.cmp(&other.item_index))
    }
}

/// Wrapper for `BinaryHeap` where the **worst** element sits at the top (bounded Top-K min-heap).
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorstFirstEntry {
    key: SortKey,
    ranked: RankedItem,
}

impl Ord for WorstFirstEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // `BinaryHeap` is a max-heap, so returning `Greater` for worse ranks places the
        // worst item at `peek()` for O(1) threshold checks and O(log K) eviction.
        self.key.cmp_best_first(&other.key)
    }
}

impl PartialOrd for WorstFirstEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Bounded Top-K collector maintaining the best `limit` items in O(N log K) time and O(K) memory.
#[derive(Debug, Clone)]
struct TopKCollector {
    limit: usize,
    heap: BinaryHeap<WorstFirstEntry>,
}

impl TopKCollector {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            heap: BinaryHeap::with_capacity(limit.min(512)),
        }
    }

    fn push_entry(&mut self, entry: WorstFirstEntry) {
        if self.limit == 0 {
            return;
        }
        if self.heap.len() < self.limit {
            self.heap.push(entry);
        } else if let Some(worst) = self.heap.peek()
            && entry.key.cmp_best_first(&worst.key) == Ordering::Less
        {
            self.heap.pop();
            self.heap.push(entry);
        }
    }

    fn push<T: SearchItem>(&mut self, item: &T, item_index: usize, result: MatchResult) {
        if self.limit == 0 {
            return;
        }
        // Fast path: if heap is full and candidate does not strictly beat the worst entry
        // (checked zero-alloc against borrowed `&str`), skip heap `SortKey` allocation completely.
        if self.heap.len() >= self.limit
            && let Some(worst) = self.heap.peek()
            && SortKey::cmp_candidate_best_first(item, item_index, &result, &worst.key)
                != Ordering::Less
        {
            return;
        }
        let key = SortKey::from_item(item, item_index, &result);
        self.push_entry(WorstFirstEntry {
            key,
            ranked: RankedItem { item_index, result },
        });
    }

    fn merge_from(&mut self, other: Self) {
        for entry in other.heap {
            self.push_entry(entry);
        }
    }

    fn snapshot_sorted(&self) -> Vec<RankedItem> {
        let mut entries: Vec<&WorstFirstEntry> = self.heap.iter().collect();
        entries.sort_unstable_by(|a, b| a.key.cmp_best_first(&b.key));
        entries.into_iter().map(|e| e.ranked.clone()).collect()
    }

    fn into_sorted(self) -> Vec<RankedItem> {
        let mut entries: Vec<WorstFirstEntry> = self.heap.into_vec();
        entries.sort_unstable_by(|a, b| a.key.cmp_best_first(&b.key));
        entries.into_iter().map(|e| e.ranked).collect()
    }
}

/// Shared read-only parameters for scoring a slice of items in parallel.
struct EvalContext<'a> {
    query: &'a str,
    engine: &'a dyn MatchEngine,
    frecency: &'a FrecencyMap,
    now_secs: i64,
    config: &'a SearchConfig,
    cancel: Option<&'a CancellationToken>,
}

/// Evaluates `items[range_start..range_end]` in parallel using Rayon chunks with cancellation.
/// Returns `None` if cancelled mid-flight.
fn score_slice_parallel<T: SearchItem>(
    items: &[T],
    range: std::ops::Range<usize>,
    ctx: &EvalContext<'_>,
) -> Option<(TopKCollector, usize)> {
    let range_start = range.start;
    let slice = &items[range];
    if slice.is_empty() {
        return Some((TopKCollector::new(ctx.config.limit), 0));
    }

    let chunk_size = ctx.config.rayon_chunk_size.max(64);
    let check_every = ctx.config.cancel_check_interval.max(32);

    let reduced = slice
        .par_chunks(chunk_size)
        .enumerate()
        .map(|(chunk_idx, chunk)| {
            if ctx.cancel.is_some_and(CancellationToken::is_cancelled) {
                return None;
            }
            let base_idx = range_start + chunk_idx * chunk_size;
            let mut local = TopKCollector::new(ctx.config.limit);
            let mut matched_count = 0usize;

            let has_frecency = !ctx.frecency.is_empty();
            for (offset, item) in chunk.iter().enumerate() {
                if offset.is_multiple_of(check_every)
                    && ctx.cancel.is_some_and(CancellationToken::is_cancelled)
                {
                    return None;
                }
                let rec = if has_frecency {
                    ctx.frecency.get(item.id())
                } else {
                    None
                };
                if let Some(res) = score_item(
                    item,
                    ctx.query,
                    ctx.engine,
                    rec,
                    ctx.now_secs,
                    &ctx.config.frecency,
                ) {
                    matched_count += 1;
                    local.push(item, base_idx + offset, res);
                }
            }

            Some((local, matched_count))
        })
        .reduce(
            || Some((TopKCollector::new(ctx.config.limit), 0usize)),
            |acc, next| match (acc, next) {
                (Some((mut left_k, left_n)), Some((right_k, right_n))) => {
                    left_k.merge_from(right_k);
                    Some((left_k, left_n + right_n))
                }
                _ => None,
            },
        );

    if ctx.cancel.is_some_and(CancellationToken::is_cancelled) {
        return None;
    }

    reduced
}

/// Runs a full synchronous parallel search over `items` using Rayon + bounded Top-K merge.
///
/// Returns `None` if `cancel` is triggered before completion.
#[must_use]
pub fn search_parallel<T: SearchItem>(
    items: &[T],
    query: &str,
    frecency: &FrecencyMap,
    now_secs: i64,
    config: &SearchConfig,
    cancel: Option<&CancellationToken>,
) -> Option<SearchSnapshot> {
    let engine = QueryParser::parse(query, config.case_matching);
    let generation = cancel.map_or(0, CancellationToken::generation);
    let ctx = EvalContext {
        query,
        engine: engine.as_ref(),
        frecency,
        now_secs,
        config,
        cancel,
    };
    let (collector, total_matched) = score_slice_parallel(items, 0..items.len(), &ctx)?;

    Some(SearchSnapshot {
        generation,
        matches: collector.into_sorted(),
        total_matched,
        processed_items: items.len(),
        total_items: items.len(),
        is_complete: true,
    })
}

struct PendingJob<T: SearchItem + 'static> {
    generation: u64,
    corpus: Arc<[T]>,
    start_index: usize,
    query: String,
    engine: Arc<dyn MatchEngine>,
    frecency: Arc<FrecencyMap>,
    now_secs: i64,
    config: SearchConfig,
    running_collector: TopKCollector,
    running_matched: usize,
    tx: mpsc::Sender<SearchSnapshot>,
}

struct SharedState<T: SearchItem + 'static> {
    latest_job: Option<PendingJob<T>>,
    shutdown: bool,
}

/// Persistent adaptive search coordinator implementing `instantMENU`-style parallel execution:
/// - Inline time-budgeted fast path for instant single-frame small-corpus matching
/// - Background parallel macro-batching with progressive [`SearchSnapshot`] emission
/// - Latest-wins query replacement with lock-free atomic cancellation
pub struct SearchSession<T: SearchItem + 'static> {
    config: SearchConfig,
    generation: Arc<AtomicU64>,
    shared: Arc<(Mutex<SharedState<T>>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl<T: SearchItem + 'static> std::fmt::Debug for SearchSession<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchSession")
            .field("config", &self.config)
            .field("generation", &self.generation.load(AtomicOrdering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl<T: SearchItem + 'static> SearchSession<T> {
    /// Creates a new search session with a dedicated background coordinator thread.
    #[must_use]
    pub fn new(config: SearchConfig) -> Self {
        let generation = Arc::new(AtomicU64::new(0));
        let shared = Arc::new((
            Mutex::new(SharedState::<T> {
                latest_job: None,
                shutdown: false,
            }),
            Condvar::new(),
        ));

        let gen_clone = Arc::clone(&generation);
        let shared_clone = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("aios-fuzzy-coordinator".into())
            .spawn(move || coordinator_loop(gen_clone, shared_clone))
            .expect("failed to spawn aios-fuzzy coordinator thread");

        Self {
            config,
            generation,
            shared,
            worker: Some(worker),
        }
    }

    /// Returns the currently active configuration.
    #[must_use]
    pub fn config(&self) -> &SearchConfig {
        &self.config
    }

    /// Cancels any currently active background search immediately.
    pub fn cancel_active(&self) {
        self.generation.fetch_add(1, AtomicOrdering::AcqRel);
        if let Ok(mut guard) = self.shared.0.lock() {
            guard.latest_job = None;
        }
    }

    /// Submits a new query against an immutable `corpus` snapshot.
    ///
    /// Small lists or fast queries finish inline within `config.inline_budget` and return
    /// `SearchHandle { initial: SearchSnapshot { is_complete: true, .. }, updates: None }`.
    /// Larger lists return an immediate partial snapshot and stream progressive parallel
    /// updates on `updates`.
    #[must_use]
    pub fn search(
        &self,
        corpus: Arc<[T]>,
        query: impl Into<String>,
        frecency: Arc<FrecencyMap>,
        now_secs: i64,
    ) -> SearchHandle {
        let query = query.into();
        let generation = self.generation.fetch_add(1, AtomicOrdering::AcqRel) + 1;
        let engine: Arc<dyn MatchEngine> =
            Arc::from(QueryParser::parse(&query, self.config.case_matching));
        let total_items = corpus.len();

        let mut collector = TopKCollector::new(self.config.limit);
        let mut matched_count = 0usize;
        let mut processed = 0usize;

        // Stage 1: Inline fast path on caller thread within strict time budget.
        let inline_start = Instant::now();
        let force_inline_all = total_items <= self.config.inline_max_items;
        let has_frecency = !frecency.is_empty();
        const TIME_CHECK_STRIDE: usize = 64;

        while processed < total_items {
            let item = &corpus[processed];
            let rec = if has_frecency {
                frecency.get(item.id())
            } else {
                None
            };
            if let Some(res) = score_item(
                item,
                &query,
                engine.as_ref(),
                rec,
                now_secs,
                &self.config.frecency,
            ) {
                matched_count += 1;
                collector.push(item, processed, res);
            }
            processed += 1;

            if !force_inline_all
                && processed.is_multiple_of(TIME_CHECK_STRIDE)
                && inline_start.elapsed() >= self.config.inline_budget
            {
                break;
            }
        }

        if processed >= total_items {
            let snapshot = SearchSnapshot {
                generation,
                matches: collector.into_sorted(),
                total_matched: matched_count,
                processed_items: total_items,
                total_items,
                is_complete: true,
            };
            return SearchHandle {
                initial: snapshot,
                updates: None,
            };
        }

        // Stage 2: Budget exceeded; emit immediate partial snapshot and hand off remainder.
        let initial = SearchSnapshot {
            generation,
            matches: collector.snapshot_sorted(),
            total_matched: matched_count,
            processed_items: processed,
            total_items,
            is_complete: false,
        };

        let (tx, rx) = mpsc::channel();
        let job = PendingJob {
            generation,
            corpus,
            start_index: processed,
            query,
            engine,
            frecency,
            now_secs,
            config: self.config.clone(),
            running_collector: collector,
            running_matched: matched_count,
            tx,
        };

        // Latest-wins replacement: overwrite any unstarted older query without queueing.
        if let Ok(mut guard) = self.shared.0.lock() {
            guard.latest_job = Some(job);
            self.shared.1.notify_one();
        }

        SearchHandle {
            initial,
            updates: Some(rx),
        }
    }
}

impl<T: SearchItem + 'static> Drop for SearchSession<T> {
    fn drop(&mut self) {
        self.generation.fetch_add(1, AtomicOrdering::AcqRel);
        if let Ok(mut guard) = self.shared.0.lock() {
            guard.shutdown = true;
            guard.latest_job = None;
            self.shared.1.notify_all();
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

fn coordinator_loop<T: SearchItem + 'static>(
    generation_counter: Arc<AtomicU64>,
    shared: Arc<(Mutex<SharedState<T>>, Condvar)>,
) {
    loop {
        let mut job = {
            let Ok(mut guard) = shared.0.lock() else {
                return;
            };
            while guard.latest_job.is_none() && !guard.shutdown {
                guard = match shared.1.wait(guard) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if guard.shutdown {
                return;
            }
            match guard.latest_job.take() {
                Some(j) => j,
                None => continue,
            }
        };

        let token = CancellationToken::new(Arc::clone(&generation_counter), job.generation);
        let total_items = job.corpus.len();
        let batch_size = job.config.macro_batch_size.max(512);
        let mut cursor = job.start_index;

        while cursor < total_items {
            if token.is_cancelled() {
                break;
            }
            let batch_end = (cursor + batch_size).min(total_items);
            let ctx = EvalContext {
                query: &job.query,
                engine: job.engine.as_ref(),
                frecency: &job.frecency,
                now_secs: job.now_secs,
                config: &job.config,
                cancel: Some(&token),
            };
            let Some((batch_collector, batch_matched)) =
                score_slice_parallel(&job.corpus, cursor..batch_end, &ctx)
            else {
                break;
            };

            job.running_collector.merge_from(batch_collector);
            job.running_matched += batch_matched;
            cursor = batch_end;

            let is_complete = cursor >= total_items;
            let snapshot = SearchSnapshot {
                generation: job.generation,
                matches: job.running_collector.snapshot_sorted(),
                total_matched: job.running_matched,
                processed_items: cursor,
                total_items,
                is_complete,
            };

            if job.tx.send(snapshot).is_err() {
                // UI dropped the receiver; stop background scoring immediately.
                break;
            }
        }
    }
}

struct StrSearchItem<'a>(&'a str);

impl SearchItem for StrSearchItem<'_> {
    #[inline]
    fn id(&self) -> &str {
        self.0
    }

    #[inline]
    fn primary_text(&self) -> &str {
        self.0
    }
}

fn score_packed_range_parallel(
    corpus: &PackedTextSnapshot,
    range: std::ops::Range<usize>,
    ctx: &EvalContext<'_>,
) -> Option<(TopKCollector, usize)> {
    let range_len = range.end.saturating_sub(range.start);
    if range_len == 0 {
        return Some((TopKCollector::new(ctx.config.limit), 0));
    }

    let chunk_size = ctx.config.rayon_chunk_size.max(64);
    let check_every = ctx.config.cancel_check_interval.max(32);
    let chunk_count = range_len.div_ceil(chunk_size);
    let range_start = range.start;
    let range_end = range.end;

    let reduced = (0..chunk_count)
        .into_par_iter()
        .map(|chunk_idx| {
            if ctx.cancel.is_some_and(CancellationToken::is_cancelled) {
                return None;
            }
            let base_idx = range_start + chunk_idx * chunk_size;
            let end_idx = (base_idx + chunk_size).min(range_end);
            let mut local = TopKCollector::new(ctx.config.limit);
            let mut matched_count = 0usize;
            let has_frecency = !ctx.frecency.is_empty();

            for (offset, idx) in (base_idx..end_idx).enumerate() {
                if offset.is_multiple_of(check_every)
                    && ctx.cancel.is_some_and(CancellationToken::is_cancelled)
                {
                    return None;
                }
                let Some(text) = corpus.get(idx) else {
                    continue;
                };
                let item = StrSearchItem(text);
                let rec = if has_frecency {
                    ctx.frecency.get(text)
                } else {
                    None
                };
                if let Some(res) = score_item(
                    &item,
                    ctx.query,
                    ctx.engine,
                    rec,
                    ctx.now_secs,
                    &ctx.config.frecency,
                ) {
                    matched_count += 1;
                    local.push(&item, idx, res);
                }
            }

            Some((local, matched_count))
        })
        .reduce(
            || Some((TopKCollector::new(ctx.config.limit), 0usize)),
            |acc, next| match (acc, next) {
                (Some((mut left_k, left_n)), Some((right_k, right_n))) => {
                    left_k.merge_from(right_k);
                    Some((left_k, left_n + right_n))
                }
                _ => None,
            },
        );

    if ctx.cancel.is_some_and(CancellationToken::is_cancelled) {
        return None;
    }

    reduced
}

/// Runs a full synchronous parallel search directly over a [`PackedTextSnapshot`] with zero
/// per-item string allocation.
#[must_use]
pub fn search_packed_parallel(
    corpus: &PackedTextSnapshot,
    query: &str,
    frecency: &FrecencyMap,
    now_secs: i64,
    config: &SearchConfig,
    cancel: Option<&CancellationToken>,
) -> Option<SearchSnapshot> {
    let engine = QueryParser::parse(query, config.case_matching);
    let generation = cancel.map_or(0, CancellationToken::generation);
    let ctx = EvalContext {
        query,
        engine: engine.as_ref(),
        frecency,
        now_secs,
        config,
        cancel,
    };
    let (collector, total_matched) = score_packed_range_parallel(corpus, 0..corpus.len(), &ctx)?;

    Some(SearchSnapshot {
        generation,
        matches: collector.into_sorted(),
        total_matched,
        processed_items: corpus.len(),
        total_items: corpus.len(),
        is_complete: true,
    })
}

struct PendingPackedJob {
    generation: u64,
    corpus: PackedTextSnapshot,
    start_index: usize,
    query: String,
    engine: Arc<dyn MatchEngine>,
    frecency: Arc<FrecencyMap>,
    now_secs: i64,
    config: SearchConfig,
    running_collector: TopKCollector,
    running_matched: usize,
    tx: mpsc::Sender<SearchSnapshot>,
}

struct SharedPackedState {
    latest_job: Option<PendingPackedJob>,
    shutdown: bool,
}

/// Adaptive parallel search session operating directly on zero-copy [`PackedTextSnapshot`] slabs.
pub struct PackedSearchSession {
    config: SearchConfig,
    generation: Arc<AtomicU64>,
    shared: Arc<(Mutex<SharedPackedState>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PackedSearchSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackedSearchSession")
            .field("config", &self.config)
            .field("generation", &self.generation.load(AtomicOrdering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl PackedSearchSession {
    #[must_use]
    pub fn new(config: SearchConfig) -> Self {
        let generation = Arc::new(AtomicU64::new(0));
        let shared = Arc::new((
            Mutex::new(SharedPackedState {
                latest_job: None,
                shutdown: false,
            }),
            Condvar::new(),
        ));

        let gen_clone = Arc::clone(&generation);
        let shared_clone = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("aios-fuzzy-packed-coord".into())
            .spawn(move || packed_coordinator_loop(gen_clone, shared_clone))
            .expect("failed to spawn aios-fuzzy packed coordinator thread");

        Self {
            config,
            generation,
            shared,
            worker: Some(worker),
        }
    }

    pub fn cancel_active(&self) {
        self.generation.fetch_add(1, AtomicOrdering::AcqRel);
        if let Ok(mut guard) = self.shared.0.lock() {
            guard.latest_job = None;
        }
    }

    #[must_use]
    pub fn search(
        &self,
        corpus: PackedTextSnapshot,
        query: impl Into<String>,
        frecency: Arc<FrecencyMap>,
        now_secs: i64,
    ) -> SearchHandle {
        let query = query.into();
        let generation = self.generation.fetch_add(1, AtomicOrdering::AcqRel) + 1;
        let engine: Arc<dyn MatchEngine> =
            Arc::from(QueryParser::parse(&query, self.config.case_matching));
        let total_items = corpus.len();

        let mut collector = TopKCollector::new(self.config.limit);
        let mut matched_count = 0usize;
        let mut processed = 0usize;

        let inline_start = Instant::now();
        let force_inline_all = total_items <= self.config.inline_max_items;
        let has_frecency = !frecency.is_empty();
        const TIME_CHECK_STRIDE: usize = 64;

        while processed < total_items {
            if let Some(text) = corpus.get(processed) {
                let item = StrSearchItem(text);
                let rec = if has_frecency {
                    frecency.get(text)
                } else {
                    None
                };
                if let Some(res) = score_item(
                    &item,
                    &query,
                    engine.as_ref(),
                    rec,
                    now_secs,
                    &self.config.frecency,
                ) {
                    matched_count += 1;
                    collector.push(&item, processed, res);
                }
            }
            processed += 1;

            if !force_inline_all
                && processed.is_multiple_of(TIME_CHECK_STRIDE)
                && inline_start.elapsed() >= self.config.inline_budget
            {
                break;
            }
        }

        if processed >= total_items {
            let snapshot = SearchSnapshot {
                generation,
                matches: collector.into_sorted(),
                total_matched: matched_count,
                processed_items: total_items,
                total_items,
                is_complete: true,
            };
            return SearchHandle {
                initial: snapshot,
                updates: None,
            };
        }

        let initial = SearchSnapshot {
            generation,
            matches: collector.snapshot_sorted(),
            total_matched: matched_count,
            processed_items: processed,
            total_items,
            is_complete: false,
        };

        let (tx, rx) = mpsc::channel();
        let job = PendingPackedJob {
            generation,
            corpus,
            start_index: processed,
            query,
            engine,
            frecency,
            now_secs,
            config: self.config.clone(),
            running_collector: collector,
            running_matched: matched_count,
            tx,
        };

        if let Ok(mut guard) = self.shared.0.lock() {
            guard.latest_job = Some(job);
            self.shared.1.notify_one();
        }

        SearchHandle {
            initial,
            updates: Some(rx),
        }
    }
}

impl Drop for PackedSearchSession {
    fn drop(&mut self) {
        self.generation.fetch_add(1, AtomicOrdering::AcqRel);
        if let Ok(mut guard) = self.shared.0.lock() {
            guard.shutdown = true;
            guard.latest_job = None;
            self.shared.1.notify_all();
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

fn packed_coordinator_loop(
    generation_counter: Arc<AtomicU64>,
    shared: Arc<(Mutex<SharedPackedState>, Condvar)>,
) {
    loop {
        let mut job = {
            let Ok(mut guard) = shared.0.lock() else {
                return;
            };
            while guard.latest_job.is_none() && !guard.shutdown {
                guard = match shared.1.wait(guard) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if guard.shutdown {
                return;
            }
            match guard.latest_job.take() {
                Some(j) => j,
                None => continue,
            }
        };

        let token = CancellationToken::new(Arc::clone(&generation_counter), job.generation);
        let total_items = job.corpus.len();
        let batch_size = job.config.macro_batch_size.max(512);
        let mut cursor = job.start_index;

        while cursor < total_items {
            if token.is_cancelled() {
                break;
            }
            let batch_end = (cursor + batch_size).min(total_items);
            let ctx = EvalContext {
                query: &job.query,
                engine: job.engine.as_ref(),
                frecency: &job.frecency,
                now_secs: job.now_secs,
                config: &job.config,
                cancel: Some(&token),
            };
            let Some((batch_collector, batch_matched)) =
                score_packed_range_parallel(&job.corpus, cursor..batch_end, &ctx)
            else {
                break;
            };

            job.running_collector.merge_from(batch_collector);
            job.running_matched += batch_matched;
            cursor = batch_end;

            let is_complete = cursor >= total_items;
            let snapshot = SearchSnapshot {
                generation: job.generation,
                matches: job.running_collector.snapshot_sorted(),
                total_matched: job.running_matched,
                processed_items: cursor,
                total_items,
                is_complete,
            };

            if job.tx.send(snapshot).is_err() {
                break;
            }
        }
    }
}
