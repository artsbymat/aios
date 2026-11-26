# AIOS Fuzzy Search Engine Specification (`aios-fuzzy`)

AIOS implements its own Pure Rust fuzzy search and ranking engine in `crates/aios-fuzzy`. **No `skim` or `fuzzy-matcher` crate is added as a dependency.** Instead, `aios-fuzzy` adopts the proven architectural concepts and scoring mechanics of `skim` (specifically `SkimMatcherV2` + `MatchEngineFactory` query composition) in a clean-room implementation tailored for a GUI command launcher.

---

## 1. Design Goals

1. **Human-Intuitive Ranking**: Typing `fch` matches `Firefox Cache`, `vsc` matches `Visual Studio Code`, and `wm-l` matches `Window Management: Left Half` near the top.
2. **Exact Character Match Indices**: Every match returns sorted character indices (`Vec<usize>`) so GPUI highlights matched glyphs directly in `accent.fg`.
3. **Power-User Query Syntax**: Supports `skim`-style multi-term `AND`/`OR`, exact substring, prefix, suffix, and inverse negation operators natively.
4. **Unicode-First & Smart-Case**: Proper Unicode scalar/char handling (important for international filenames, notes, and clipboard entries) with `Smart`, `Ignore`, and `Respect` case modes.
5. **Sub-Frame Latency & Cancellation**: Zero UI stalls even over 50,000+ file or clipboard items via chunked parallel matching and atomic cancellation per keystroke.

---

## 2. Core Traits & Data Types

Modeled cleanly after `skim`'s `SkimItem` and `MatchEngineFactory` separation, decoupled from any terminal/TUI assumptions:

```rust
pub trait SearchItem: Send + Sync {
    /// Stable identifier used for frecency lookup and selection persistence.
    fn id(&self) -> &str;
    /// Primary searchable text (e.g. command title, application name).
    fn primary_text(&self) -> &str;
    /// Secondary searchable text (e.g. subtitle, alias, package name).
    fn secondary_text(&self) -> Option<&str> { None }
    /// Additional keywords weighted lower than primary_text.
    fn keywords(&self) -> &[String] { &[] }
    /// Optional explicit user alias (exact/prefix match gets top priority).
    fn alias(&self) -> Option<&str> { None }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    pub score: i64,
    pub primary_indices: Vec<usize>,
    pub secondary_indices: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMatching {
    Smart,   // Case-insensitive unless query contains any uppercase character
    Ignore,  // Always case-insensitive
    Respect, // Always case-sensitive
}

pub trait MatchEngine: Send + Sync {
    fn match_text(&self, haystack: &str) -> Option<(i64, Vec<usize>)>;
}

pub trait MatchEngineFactory: Send + Sync {
    fn create_engine(&self, query: &str, case: CaseMatching) -> Box<dyn MatchEngine>;
}
```

---

## 3. Clean-Room `SkimV2`-Style Scoring Algorithm

When matching a pattern `P` (length `m`) against a haystack `H` (length `n`), every character of `P` must appear in `H` in order.

### Step 1: Fast Pre-Filter (Subsequence Scan)

Before allocating or filling DP tables, run a single linear pass over `H` to confirm all characters of `P` exist in order and record the tightest `[first_match ..= last_match]` window.

- If the subsequence does not exist, immediately return `None` in $O(n)$ with zero allocations.
- All subsequent DP computation runs strictly inside the narrowed slice `[first_match ..= last_match]`.

### Step 2: Character Bonus Table `B[j]`

Classify each character `H[j]` into a character class:

- `Whitespace` (` `, `\t`, `\n`)
- `Delimiter` (`/`, `\`, `:`, `;`, `-`, `_`, `.`, `|`)
- `Lower`, `Upper`, `Digit`, `Other`

Assign a positional bonus `B[j]` based on transition from `H[j - 1]` to `H[j]`:

| Condition                                        | Default Bonus                           | Rationale                                                   |
| ------------------------------------------------ | --------------------------------------- | ----------------------------------------------------------- |
| Start of string (`j == 0`)                       | `+10` (plus first-char multiplier `x2`) | Strongly favors prefix matches                              |
| After `Whitespace`                               | `+8`                                    | Strongly favors word starts (`Visual Studio Code` -> `vsc`) |
| After path/code `Delimiter` (`/`, `_`, `-`, `.`) | `+7`                                    | Ideal for file paths and kebab/snake commands               |
| `Lower` -> `Upper` (`camelCase` boundary)        | `+6`                                    | Matches symbol/app names (`HyprPaper` -> `hp`)              |
| Non-digit -> `Digit` transition                  | `+4`                                    | Helps matching versioned tools (`gtk4`, `py3`)              |
| Interior character                               | `+0`                                    | Baseline                                                    |

### Step 3: Affine Gap Smith-Waterman DP

Constants (tunable and covered by ranking regression tests):

- `SCORE_MATCH = 16`
- `BONUS_CONSECUTIVE = 4` (compounding or max of streak boundary bonus so contiguous substrings beat scattered matches)
- `PENALTY_GAP_START = -3`
- `PENALTY_GAP_EXTENSION = -1`
- `BONUS_EXACT_CASE = 1` (tiny tie-breaker when case matches exactly in Smart/Ignore mode)

Two DP matrices over `i in 0..m` (pattern) and `j in 0..n_window` (haystack slice):

- **`M[i][j]`**: Best score matching `P[0..=i]` with `P[i]` aligned to `H[j]`.
  ```text
  M[i][j] = SCORE_MATCH + B[j] + case_bonus(P[i], H[j])
            + max(
                M[i-1][j-1] + consecutive_bonus(i, j),
                P[i-1][j-1]
              )
  ```
- **`P[i][j]`**: Best score matching `P[0..=i]` ending in an unmatched gap at `H[j]`.
  ```text
  P[i][j] = max(
              M[i][j-1] + PENALTY_GAP_START + PENALTY_GAP_EXTENSION,
              P[i][j-1] + PENALTY_GAP_EXTENSION
            )
  ```

### Step 4: Backtracking for Match Indices

Starting from `argmax_j M[m-1][j]`, trace backwards through the DP decision table to recover the exact matched character indices in `H`, offset by `first_match`, sorted ascending for direct GPUI text run highlighting.

---

## 4. Query Syntax & Composite Engines

`QueryParser` tokenizes the raw user search input into an `AndOrEngine` tree modeled on `skim`'s extended search syntax:

| Token Syntax | Engine Type     | Behavior                                                                |
| ------------ | --------------- | ----------------------------------------------------------------------- |
| `foo`        | `FuzzyEngine`   | Fuzzy subsequence match with DP score                                   |
| `'foo`       | `ExactEngine`   | Contiguous substring match (`contains`)                                 |
| `^foo`       | `PrefixEngine`  | Must match at start of text                                             |
| `foo$`       | `SuffixEngine`  | Must match at end of text                                               |
| `!foo`       | `InverseEngine` | Excludes items matching `foo` (also supports `!'foo`, `!^foo`, `!foo$`) |
| `foo bar`    | `AndEngine`     | Both `foo` AND `bar` must match; scores sum and indices merge           |
| `foo \| bar` | `OrEngine`      | Either `foo` OR `bar` matches; highest scoring branch wins              |

Escaped spaces (`\ `) stay inside a single token. Empty queries bypass matching and fall back to default ordering (Pinned -> Recent/Frecency -> Alphabetical).

---

## 5. Multi-Field Weighting & Frecency Blending

In Root Search and `List` views, items have multiple searchable fields plus historical usage. Final rank score combines text match quality with user behavior:

```text
final_score = text_score + alias_boost + frecency_boost + pin_boost
```

1. **Field Priority**:
   - Exact `alias` match: `+10_000` (instant deterministic recall, e.g., typing `cb` always ranks Clipboard History #1)
   - `primary_text` match: `1.0x` base DP score
   - `secondary_text` match: `0.7x` base DP score (used when primary does not match or as additive bonus on multi-term queries)
   - `keywords` match: `0.6x` base DP score
2. **Frecency Formula** (stored in SQLite `frecency` table):
   - Decay buckets by time since `last_used_at`:
     - `< 4 hours`: `1.0`
     - `< 24 hours`: `0.8`
     - `< 7 days`: `0.6`
     - `< 30 days`: `0.35`
     - `>= 30 days`: `0.15`
   - `frecency_boost = (ln(1 + hits) * decay_weight * 80.0) as i64`, capped so a terrible fuzzy match never outranks an exact title match.
   - Query-prefix affinity: extra bonus when the user repeatedly selected `item_id` after typing the same prefix.
3. **Tie-Breaking**:
   - Higher `final_score` -> shorter `primary_text` length -> earlier first match index -> lexicographical order.

---

## 6. Concurrency, Adaptive Parallel Execution & Progressive Results

Inspired by `instantMENU`'s parallel search/filter/render architecture (specifically its non-blocking parallel pipeline, without adopting X11 backends or `dmenu`-style `stdin` streaming), `aios-fuzzy` decouples matching from the GPUI render loop using a 3-stage concurrency model:

### 6.1 Adaptive Inline Fast Path vs. Parallel Background Handoff

- **Inline Time Budget**: When a query arrives, the session scores an initial window inline on the caller thread up to a strict time budget (default `1.5ms` or small corpora `<= 1,024` items). Small lists (Root Search apps, commands, quicklinks) finish synchronously in a single frame with zero thread-dispatch overhead and zero visual flicker.
- **Automatic Parallel Handoff**: If items remain once the inline budget expires (e.g. File Search, large Clipboard History, or bulk plugin datasets), the session immediately returns the initial partial `SearchSnapshot` so the UI renders right away, and dispatches the remaining `Arc<[T]>` range to the background parallel pool (`rayon`).

### 6.2 Progressive Chunked Snapshots (Search, Filter & Render in Parallel)

- **Parallel Chunk Scoring**: Background workers split remaining items into work-stealing chunks and score them concurrently across CPU cores using thread-local DP scratch buffers (`Vec<i32>`), keeping per-item heap allocations at zero on the hot path.
- **Bounded Top-K Min-Heap Merge**: Each chunk retains only its top `K` (default `200`) highest-scoring matches in a binary min-heap instead of sorting the entire corpus. After each macro-batch (e.g. `8,192` items or `~8ms`), results merge into the running global Top-K and emit a progressive `SearchSnapshot` (`generation`, `matches`, `total_matched`, `processed_items`, `total_items`, `is_complete`).
- **Live UI Feedback**: While `is_complete == false`, the GPUI render thread stays responsive at 60-144Hz, displays the best matches discovered so far, activates the 2px header loading track, and indicates ongoing refinement in the match counter (e.g., `42...`).

### 6.3 Latest-Wins Query Replacement, Atomic Cancellation & Packed Corpora

- **Zero Keystroke Queueing**: Typing faster than a large corpus can be scored never queues stale searches. A new query bumps a monotonic `Arc<AtomicU64>` generation counter and replaces any pending query in a single latest-wins slot.
- **Fine-Grained Cancellation**: Parallel workers poll the generation counter every `256` items and abort stale work immediately. Any in-flight snapshot with an older `generation` is dropped on the receiver side.
- **Lock-Free Corpus Reads**: General structured corpora are shared via immutable `Arc<[T]>` snapshots so UI rendering and parallel worker threads never contend on a mutex during matching.
- **Contiguous `mmap`-Backed Text Corpora (`PackedTextCorpus` / `PackedSearchSession`)**: For huge plain-text streams (such as `find / | aios dmenu` with 1M+ entries), `crates/aios-fuzzy/src/corpus.rs` packs UTF-8 bytes from the front and 4-byte native-endian end offsets from the back of unified contiguous slabs (`64 KiB` initial slab for small lists, `33 MiB` bulk slabs strictly above Linux glibc's `32 MiB` `DEFAULT_MMAP_THRESHOLD_MAX` for large lists). `PackedSearchSession` shares immutable `PackedTextSnapshot` slices (`O(slabs)` `Arc` clones, zero per-item `String` allocation, zero deep-clone on keystrokes) and guarantees immediate OS `munmap()` reclamation when the corpus drops.

### 6.4 Benchmark Targets

- Single item match: `< 1 microsecond` average
- 10,000 items parallel match: `< 5ms`
- 100,000 items parallel match: `< 25ms`
- Inline fast path handoff budget: `<= 1.5ms` max UI-thread time per keystroke
