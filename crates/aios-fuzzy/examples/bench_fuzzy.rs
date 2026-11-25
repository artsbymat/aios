//! Release-mode micro-benchmark verifying `docs/FUZZY_SPEC.md` performance budgets:
//! - Single item match: < 1 microsecond average
//! - 10,000 items parallel match: < 5ms
//! - 100,000 items parallel match: < 25ms

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use aios_fuzzy::{SearchConfig, SearchItem, SearchSession, SkimV2Matcher, search_parallel};

#[derive(Debug)]
struct BenchItem {
    id: String,
    path: String,
}

impl SearchItem for BenchItem {
    fn id(&self) -> &str {
        &self.id
    }
    fn primary_text(&self) -> &str {
        &self.path
    }
}

fn build_corpus(n: usize) -> Vec<BenchItem> {
    let dirs = [
        "crates/aios-fuzzy/src",
        "crates/aios-core/src",
        "crates/aios-ui/src/views",
        "crates/aios-wayland/src/ipc",
        "crates/aios-plugin-host/src/wasm",
        "docs/specs",
    ];
    let names = [
        "matcher.rs",
        "launcher_window.rs",
        "hyprland_adapter.rs",
        "niri_adapter.rs",
        "clipboard_daemon.rs",
        "visual_studio_code.desktop",
        "firefox_browser.desktop",
        "window_management.rs",
    ];

    (0..n)
        .map(|i| BenchItem {
            id: format!("item-{i}"),
            path: format!(
                "/home/user/work/aios/{}/{}_{i}",
                dirs[i % dirs.len()],
                names[i % names.len()]
            ),
        })
        .collect()
}

fn main() {
    let now = 1_750_000_000i64;
    let frecency = HashMap::new();
    let cfg = SearchConfig::default();

    // 1. Single-item hot-path average (mixed matches & non-matches).
    let matcher = SkimV2Matcher::new("vsc", false);
    let samples = [
        "Visual Studio Code",
        "Firefox Web Browser",
        "/usr/share/applications/code.desktop",
        "Hyprland Window Management",
        "Clipboard History Manager",
        "Vector Scene Composer",
        "System Settings Utility",
        "Niri Scrollable Tiling",
    ];
    let iters = 100_000usize;
    let t0 = Instant::now();
    let mut acc = 0i64;
    for i in 0..iters {
        if let Some((score, _)) = matcher.match_text(samples[i % samples.len()]) {
            acc += score;
        }
    }
    let single_ns = t0.elapsed().as_nanos() as f64 / iters as f64;
    println!("Single item match avg : {single_ns:.1} ns (checksum={acc})");

    // 2. 10,000 items parallel match (best of 5 after warmup).
    let corpus_10k = build_corpus(10_000);
    let _ = search_parallel(&corpus_10k, "aios clip", &frecency, now, &cfg, None);
    let mut best_10k = f64::MAX;
    let mut matched_10k = 0;
    for _ in 0..5 {
        let s = Instant::now();
        let snap = search_parallel(&corpus_10k, "aios clip", &frecency, now, &cfg, None)
            .expect("10k completed");
        matched_10k = snap.total_matched;
        best_10k = best_10k.min(s.elapsed().as_secs_f64() * 1000.0);
    }
    println!("10,000 items parallel : {best_10k:.2} ms (matched={matched_10k})");

    // 3. 100,000 items parallel match (best of 5 after warmup).
    let corpus_100k = build_corpus(100_000);
    let _ = search_parallel(&corpus_100k, "hypr adapt 99", &frecency, now, &cfg, None);
    let mut best_100k = f64::MAX;
    let mut matched_100k = 0;
    for _ in 0..5 {
        let s = Instant::now();
        let snap = search_parallel(&corpus_100k, "hypr adapt 99", &frecency, now, &cfg, None)
            .expect("100k completed");
        matched_100k = snap.total_matched;
        best_100k = best_100k.min(s.elapsed().as_secs_f64() * 1000.0);
    }
    println!("100,000 items parallel: {best_100k:.2} ms (matched={matched_100k})");

    // 4. Adaptive SearchSession inline handoff latency on 100,000 items.
    let arc_100k: Arc<[BenchItem]> = corpus_100k.into();
    let arc_frec = Arc::new(frecency);
    let session = SearchSession::new(cfg);
    let s = Instant::now();
    let handle = session.search(arc_100k, "hypr adapt 99", arc_frec, now);
    let ui_block_ms = s.elapsed().as_secs_f64() * 1000.0;
    let mut updates_count = 0usize;
    let mut final_matched = handle.initial.total_matched;
    if let Some(rx) = handle.updates {
        for snap in rx {
            updates_count += 1;
            final_matched = snap.total_matched;
        }
    }
    let total_session_ms = s.elapsed().as_secs_f64() * 1000.0;
    println!(
        "100k adaptive session : UI thread returned in {ui_block_ms:.2} ms, \
         {updates_count} progressive batches, total {total_session_ms:.2} ms (matched={final_matched})"
    );
}
