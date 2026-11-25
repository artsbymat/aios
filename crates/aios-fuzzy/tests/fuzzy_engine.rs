use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use aios_fuzzy::{
    CancellationToken, CaseMatching, FrecencyConfig, FrecencyMap, FrecencyRecord,
    MatchEngineFactory, QueryEngineFactory, QueryParser, SearchConfig, SearchItem, SearchSession,
    SkimV2Matcher, compute_frecency_boost, decay_weight, score_item, search_parallel,
};

#[derive(Debug, Clone)]
struct TestItem {
    id: String,
    primary: String,
    secondary: Option<String>,
    keywords: Vec<String>,
    alias: Option<String>,
}

impl TestItem {
    fn new(id: &str, primary: &str) -> Self {
        Self {
            id: id.to_owned(),
            primary: primary.to_owned(),
            secondary: None,
            keywords: Vec::new(),
            alias: None,
        }
    }

    fn with_secondary(mut self, s: &str) -> Self {
        self.secondary = Some(s.to_owned());
        self
    }

    fn with_keywords(mut self, kw: &[&str]) -> Self {
        self.keywords = kw.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    fn with_alias(mut self, alias: &str) -> Self {
        self.alias = Some(alias.to_owned());
        self
    }
}

impl SearchItem for TestItem {
    fn id(&self) -> &str {
        &self.id
    }
    fn primary_text(&self) -> &str {
        &self.primary
    }
    fn secondary_text(&self) -> Option<&str> {
        self.secondary.as_deref()
    }
    fn keywords(&self) -> &[String] {
        &self.keywords
    }
    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

fn extract_matched_chars(haystack: &str, indices: &[usize]) -> String {
    let chars: Vec<char> = haystack.chars().collect();
    indices.iter().map(|&i| chars[i]).collect()
}

#[test]
fn word_boundary_and_camelcase_ranking() {
    let vsc = SkimV2Matcher::new("vsc", false);
    let (score_vsc, idx_vsc) = vsc.match_text("Visual Studio Code").expect("should match");
    assert_eq!(idx_vsc, vec![0, 7, 14]);
    assert_eq!(extract_matched_chars("Visual Studio Code", &idx_vsc), "VSC");

    let (score_scattered, _) = vsc
        .match_text("avxxxsxxxc")
        .expect("scattered should match");
    assert!(
        score_vsc > score_scattered,
        "word starts ({score_vsc}) must outrank scattered interior ({score_scattered})"
    );

    let hp = SkimV2Matcher::new("hp", false);
    let (score_camel, idx_camel) = hp.match_text("HyprPaper").expect("should match");
    assert_eq!(idx_camel, vec![0, 4]);
    let (score_mid, _) = hp.match_text("ahxp").expect("should match");
    assert!(score_camel > score_mid);

    let fch = SkimV2Matcher::new("fch", false);
    let (score_fc, idx_fc) = fch.match_text("Firefox Cache").expect("should match");
    assert_eq!(idx_fc[0], 0);
    assert_eq!(idx_fc.len(), 3);
    let (score_bad, _) = fch.match_text("afxxxxcxxxxh").expect("should match");
    assert!(score_fc > score_bad);

    let fc = SkimV2Matcher::new("fc", false);
    let (_, idx_word_starts) = fc.match_text("Firefox Cache").expect("should match");
    assert_eq!(idx_word_starts, vec![0, 8]);
}

#[test]
fn contiguous_substring_and_later_boundary_win() {
    let matcher = SkimV2Matcher::new("abc", false);

    // Later path boundary `/abc` must beat earlier interior `xabc`.
    let (_, idx) = matcher
        .match_text("xabc /abc")
        .expect("should match later boundary");
    assert_eq!(idx, vec![6, 7, 8]);

    // Contiguous run beats scattered characters.
    let fire = SkimV2Matcher::new("fire", false);
    let (contig_score, contig_idx) = fire.match_text("Firefox").expect("should match");
    let (spread_score, _) = fire.match_text("f_i_r_e_x").expect("should match");
    assert_eq!(contig_idx, vec![0, 1, 2, 3]);
    assert!(contig_score > spread_score);
}

#[test]
fn digit_transition_and_delimiter_bonuses() {
    let matcher = SkimV2Matcher::new("g4", false);
    let (score_digit, idx) = matcher.match_text("gtk4").expect("should match");
    assert_eq!(idx, vec![0, 3]);
    let (score_plain, _) = matcher.match_text("g124").expect("should match");
    assert!(score_digit > score_plain);

    let path_matcher = SkimV2Matcher::new("wm-l", false);
    let (_, idx_wm) = path_matcher
        .match_text("Window Management - Left Half")
        .expect("should match");
    assert_eq!(
        extract_matched_chars("Window Management - Left Half", &idx_wm),
        "WM-L"
    );
}

#[test]
fn unicode_char_indices_are_accurate() {
    let matcher = SkimV2Matcher::new("café", false);
    let (_, idx) = matcher.match_text("🚀 Café Mocha").expect("should match");
    assert_eq!(idx, vec![2, 3, 4, 5]);
    assert_eq!(extract_matched_chars("🚀 Café Mocha", &idx), "Café");

    let cjk = SkimV2Matcher::new("東駅", false);
    let (_, idx_cjk) = cjk.match_text("東京駅").expect("should match");
    assert_eq!(idx_cjk, vec![0, 2]);
    assert_eq!(extract_matched_chars("東京駅", &idx_cjk), "東駅");
}

#[test]
fn query_parser_operators_and_case_modes() {
    let factory = QueryEngineFactory;

    // Exact substring `'term`
    let exact = factory.create_engine("'fox", CaseMatching::Smart);
    assert!(exact.match_text("Firefox").is_some());
    assert!(exact.match_text("f_o_x").is_none());

    // Prefix `^term`
    let prefix = factory.create_engine("^fire", CaseMatching::Smart);
    assert!(prefix.match_text("Firefox").is_some());
    assert!(prefix.match_text("Waterfire").is_none());

    // Suffix `term$`
    let suffix = factory.create_engine("fox$", CaseMatching::Smart);
    assert!(suffix.match_text("Firefox").is_some());
    assert!(suffix.match_text("Foxhound").is_none());

    // Full exact `^term$`
    let full = factory.create_engine("^firefox$", CaseMatching::Smart);
    assert!(full.match_text("Firefox").is_some());
    assert!(full.match_text("Firefox Developer").is_none());

    // Inverse `!term` and `!'term`
    let inv = factory.create_engine("firefox !dev", CaseMatching::Smart);
    assert!(inv.match_text("Firefox Browser").is_some());
    assert!(inv.match_text("Firefox Developer").is_none());

    // AND + OR precedence: `hypr left | right` => `hypr AND (left OR right)`
    let combo = factory.create_engine("hypr left | right", CaseMatching::Smart);
    assert!(combo.match_text("Hyprland: Snap Left Half").is_some());
    assert!(combo.match_text("Hyprland: Snap Right Half").is_some());
    assert!(combo.match_text("Hyprland: Maximize").is_none());
    assert!(combo.match_text("Niri: Move Left").is_none());

    // Escaped space stays inside one exact token
    let escaped = factory.create_engine("'foo\\ bar", CaseMatching::Smart);
    assert!(escaped.match_text("my foo bar app").is_some());
    assert!(escaped.match_text("my foobar app").is_none());

    // Smart vs Ignore vs Respect case
    let smart_upper = QueryParser::parse("FireFox", CaseMatching::Smart);
    assert!(smart_upper.match_text("FireFox").is_some());
    assert!(smart_upper.match_text("firefox").is_none());

    let ignore_upper = QueryParser::parse("FireFox", CaseMatching::Ignore);
    assert!(ignore_upper.match_text("firefox").is_some());

    let respect_lower = QueryParser::parse("firefox", CaseMatching::Respect);
    assert!(respect_lower.match_text("Firefox").is_none());
    assert!(respect_lower.match_text("firefox").is_some());

    // Edge-case operator-only queries never panic
    for edge in ["", "   ", "'", "^", "$", "!", "^$", "|"] {
        let eng = QueryParser::parse(edge, CaseMatching::Smart);
        let _ = eng.match_text("Firefox");
    }
}

#[test]
fn multi_field_scoring_and_frecency_blending() {
    let config = FrecencyConfig::default();
    let now = 1_750_000_000i64;

    // Decay bucket monotonicity
    assert!((decay_weight(60) - 1.0).abs() < f64::EPSILON);
    assert!((decay_weight(6 * 3600) - 0.8).abs() < f64::EPSILON);
    assert!((decay_weight(3 * 86_400) - 0.6).abs() < f64::EPSILON);
    assert!((decay_weight(14 * 86_400) - 0.35).abs() < f64::EPSILON);
    assert!((decay_weight(60 * 86_400) - 0.15).abs() < f64::EPSILON);

    let recent_rec = FrecencyRecord {
        hits: 15,
        last_used_at_secs: now - 300,
        pinned: false,
        query_prefix_hits: 4,
    };
    let old_rec = FrecencyRecord {
        hits: 15,
        last_used_at_secs: now - 45 * 86_400,
        pinned: false,
        query_prefix_hits: 0,
    };
    assert!(
        compute_frecency_boost(&recent_rec, now, &config)
            > compute_frecency_boost(&old_rec, now, &config)
    );

    // Exact alias override beats plain fuzzy match
    let clipboard = TestItem::new("cb", "Clipboard History").with_alias("cb");
    let codebase = TestItem::new("codebase", "Codebase Explorer");
    let engine = QueryParser::parse("cb", CaseMatching::Smart);

    let cb_res = score_item(&clipboard, "cb", engine.as_ref(), None, now, &config)
        .expect("alias should match");
    let code_res = score_item(&codebase, "cb", engine.as_ref(), None, now, &config)
        .expect("fuzzy should match");
    assert!(cb_res.score > code_res.score + 5_000);

    // Cross-field multi-term match (`primary` + `secondary`)
    let ff = TestItem::new("ff", "Firefox")
        .with_secondary("Web Browser")
        .with_keywords(&["mozilla", "internet"]);
    let cross_engine = QueryParser::parse("firefox browser", CaseMatching::Smart);
    let cross_res = score_item(
        &ff,
        "firefox browser",
        cross_engine.as_ref(),
        None,
        now,
        &config,
    )
    .expect("cross-field query should match");
    assert!(!cross_res.primary_indices.is_empty());
    assert!(!cross_res.secondary_indices.is_empty());
}

#[test]
fn inline_fast_path_and_progressive_parallel_handoff_match_exactly() {
    let now = 1_750_000_000i64;
    let items: Arc<[TestItem]> = (0..12_000)
        .map(|i| {
            if i == 11_999 {
                TestItem::new("target", "Visual Studio Code").with_secondary("Code Editor")
            } else if i % 100 == 0 {
                TestItem::new(
                    &format!("item-{i}"),
                    &format!("Virtual Software Container {i}"),
                )
            } else {
                TestItem::new(&format!("noise-{i}"), &format!("System Utility Entry {i}"))
            }
        })
        .collect::<Vec<_>>()
        .into();

    let frecency = Arc::new(FrecencyMap::new());

    // 1. Small corpus completes inline with zero background updates.
    let small: Arc<[TestItem]> = items[..200].to_vec().into();
    let inline_session = SearchSession::new(SearchConfig::default());
    let inline_handle = inline_session.search(small, "vsc", Arc::clone(&frecency), now);
    assert!(inline_handle.initial.is_complete);
    assert!(inline_handle.updates.is_none());

    // 2. Force immediate handoff + multiple macro-batches to test instantMENU-style progressive streaming.
    let progressive_cfg = SearchConfig {
        limit: 50,
        inline_budget: Duration::ZERO,
        inline_max_items: 0,
        macro_batch_size: 2_048,
        rayon_chunk_size: 256,
        ..SearchConfig::default()
    };
    let session = SearchSession::new(progressive_cfg.clone());
    let handle = session.search(Arc::clone(&items), "vsc", Arc::clone(&frecency), now);

    assert!(
        !handle.initial.is_complete,
        "forced zero-budget search must return an immediate partial snapshot"
    );
    let rx = handle
        .updates
        .expect("partial snapshot must provide progressive receiver");

    let mut snapshots = vec![handle.initial];
    for snap in rx {
        snapshots.push(snap);
    }

    assert!(
        snapshots.len() >= 3,
        "expected multiple progressive snapshots, got {}",
        snapshots.len()
    );
    for pair in snapshots.windows(2) {
        assert_eq!(pair[0].generation, pair[1].generation);
        assert!(pair[1].processed_items > pair[0].processed_items);
    }

    let final_snap = snapshots.last().expect("at least one snapshot");
    assert!(final_snap.is_complete);
    assert_eq!(final_snap.processed_items, items.len());
    assert_eq!(items[final_snap.matches[0].item_index].id, "target");

    // Final progressive result must match synchronous `search_parallel` identically.
    let sync_snap = search_parallel(&items, "vsc", &frecency, now, &progressive_cfg, None)
        .expect("sync search must complete");
    assert_eq!(final_snap.total_matched, sync_snap.total_matched);
    assert_eq!(final_snap.matches, sync_snap.matches);
}

#[test]
fn latest_wins_cancellation_and_token_abort() {
    let now = 1_750_000_000i64;
    let items: Arc<[TestItem]> = (0..15_000)
        .map(|i| TestItem::new(&format!("id-{i}"), &format!("Workspace Window Alpha {i}")))
        .collect::<Vec<_>>()
        .into();
    let frecency = Arc::new(HashMap::new());

    // Direct token cancellation aborts `search_parallel` immediately.
    let counter = Arc::new(AtomicU64::new(2));
    let stale_token = CancellationToken::new(Arc::clone(&counter), 1);
    let aborted = search_parallel(
        &items,
        "alpha",
        &frecency,
        now,
        &SearchConfig::default(),
        Some(&stale_token),
    );
    assert!(aborted.is_none());

    // Rapid latest-wins burst on SearchSession returns exact results for the final query.
    let cfg = SearchConfig {
        inline_budget: Duration::ZERO,
        inline_max_items: 0,
        macro_batch_size: 2_048,
        ..SearchConfig::default()
    };
    let session = SearchSession::new(cfg);

    let _h1 = session.search(Arc::clone(&items), "a", Arc::clone(&frecency), now);
    let _h2 = session.search(Arc::clone(&items), "al", Arc::clone(&frecency), now);
    let _h3 = session.search(Arc::clone(&items), "alp", Arc::clone(&frecency), now);
    let final_handle = session.search(
        Arc::clone(&items),
        "alpha 14999",
        Arc::clone(&frecency),
        now,
    );

    let mut latest = final_handle.initial;
    if let Some(rx) = final_handle.updates {
        for snap in rx {
            latest = snap;
        }
    }

    assert!(latest.is_complete);
    assert_eq!(latest.matches.len(), 1);
    assert_eq!(items[latest.matches[0].item_index].id, "id-14999");
}

#[test]
fn performance_sanity_10k_items() {
    let now = 1_750_000_000i64;
    let items: Vec<TestItem> = (0..10_000)
        .map(|i| {
            TestItem::new(
                &format!("file-{i}"),
                &format!(
                    "/home/user/projects/aios/crates/module_{}/file_{i}.rs",
                    i % 50
                ),
            )
        })
        .collect();
    let frecency = HashMap::new();
    let cfg = SearchConfig::default();

    let start = Instant::now();
    let snap =
        search_parallel(&items, "mod file 99", &frecency, now, &cfg, None).expect("should finish");
    let elapsed = start.elapsed();

    assert!(snap.is_complete);
    assert!(!snap.matches.is_empty());
    // Generous debug-mode bound; release benchmark is verified separately.
    assert!(
        elapsed < Duration::from_secs(2),
        "10k parallel match took unexpectedly long: {elapsed:?}"
    );
}
