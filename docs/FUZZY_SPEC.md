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

## 6. Concurrency, Cancellation & Memory Reuse

- **Scratch Buffer Reuse**: Each worker thread keeps a thread-local DP scratch buffer (`Vec<i32>`) so matching 50,000 items allocates zero per-item heap buffers on the hot path.
- **Keystroke Cancellation**: Each search dispatch creates an `Arc<AtomicU64>` generation counter. Workers check the generation counter every 256 items and abort stale searches immediately when the user types the next character.
- **Top-K Bounded Heap**: When matching large lists (e.g., File Search), workers retain only the top `K` (default `200`) highest-scoring items using a binary min-heap instead of sorting the entire corpus.
- **Benchmark Targets**:
  - Single item match: `< 1 microsecond` average
  - 10,000 items parallel match: `< 5ms`
  - 100,000 items parallel match: `< 25ms`
