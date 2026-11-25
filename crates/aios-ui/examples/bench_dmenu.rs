//! Release-mode benchmark for `aios dmenu` streaming batches, live `DmenuState` fuzzy
//! filtering (10k & 50k items), and O(1) pre-indexed `IconResolver` lookups.
//!
//! Run via:
//! `cargo run --release -p aios-ui --example bench_dmenu`

use std::sync::Arc;
use std::time::{Duration, Instant};

use aios_builtin::IconResolver;
use aios_daemon::{DmenuCaseMode, DmenuRequest};
use aios_fuzzy::{CaseMatching, FrecencyMap, SearchConfig, SearchSession};
use aios_ui::DmenuState;

fn generate_items(n: usize) -> Vec<String> {
    let prefixes = [
        "/usr/share/icons/Papirus-Dark/48x48/apps",
        "/usr/share/icons/Papirus-Dark/24x24/actions",
        "/home/user/projects/aios/crates/aios-ui/src",
        "/home/user/documents/notes/wayland",
    ];
    let names = [
        "camera-photo",
        "accessories-screenshot",
        "edit-cut",
        "preferences-system-windows",
        "utilities-terminal",
        "window-close",
        "document-save",
        "firefox-developer",
        "folder-documents",
        "audio-volume-high",
    ];
    (0..n)
        .map(|i| {
            format!(
                "{}/{}-{:05}.svg",
                prefixes[i % prefixes.len()],
                names[i % names.len()],
                i
            )
        })
        .collect()
}

fn main() {
    println!("=== AIOS dmenu & IconResolver Release Benchmark ===");

    // 1. Pre-indexed IconResolver build + 5,000 lookups
    let sample_icon_names = [
        "camera-photo",
        "accessories-screenshot",
        "edit-cut",
        "preferences-system-windows",
        "utilities-terminal",
        "firefox",
        "folder",
        "clock",
        "window-close",
        "nonexistent-icon-xyz",
    ];
    let t0 = Instant::now();
    let mut resolver = IconResolver::new();
    let first = resolver.resolve("camera-photo");
    let index_build_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let mut hits = 0usize;
    for i in 0..5_000 {
        if resolver
            .resolve(sample_icon_names[i % sample_icon_names.len()])
            .is_some()
        {
            hits += 1;
        }
    }
    let lookup_5k_us = t1.elapsed().as_secs_f64() * 1_000_000.0;
    println!(
        "[IconResolver] Cold index build + 1st resolve: {:.2} ms (camera-photo resolved={})",
        index_build_ms,
        first.is_some()
    );
    println!(
        "[IconResolver] 5,000 cached O(1) lookups: {:.2} ms ({:.0} ns/lookup, hits={hits}/5000)",
        lookup_5k_us / 1000.0,
        lookup_5k_us * 1000.0 / 5_000.0
    );

    // 2. Streaming append_batch (10,000 items in 256-line batches) while user already typed "cam"
    let items_10k = generate_items(10_000);
    let mut state_10k = DmenuState::from_request(&DmenuRequest {
        prompt: Some("Find 10k".to_string()),
        streaming: true,
        case_mode: DmenuCaseMode::Smart,
        ..Default::default()
    });
    state_10k.insert_text("cam");
    let t2 = Instant::now();
    for chunk in items_10k.chunks(256) {
        state_10k.append_batch(chunk.to_vec(), Vec::new());
    }
    state_10k.finish_loading();
    let stream_10k_ms = t2.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[DmenuState 10k] Streaming append_batch (40 batches of 256) with live query 'cam': {:.2} ms (matches={})",
        stream_10k_ms,
        state_10k.filtered_entries().len()
    );

    // 3. Keystroke re-filter on 10,000 items (e.g. typing "scr")
    state_10k.handle_escape();
    let t3 = Instant::now();
    state_10k.insert_text("scr");
    let refilter_10k_ms = t3.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[DmenuState 10k] Single keystroke filter ('scr'): {:.2} ms (matches={})",
        refilter_10k_ms,
        state_10k.filtered_entries().len()
    );

    // 4. Arrow navigation (1,000 move_vertical calls on 10,000 cached items)
    let t4 = Instant::now();
    for _ in 0..1_000 {
        state_10k.move_vertical(1);
    }
    let nav_1k_us = t4.elapsed().as_secs_f64() * 1_000_000.0;
    println!(
        "[DmenuState 10k] 1,000 arrow moves (cached): {:.2} us ({:.1} ns/move)",
        nav_1k_us,
        nav_1k_us * 1000.0 / 1_000.0
    );

    // 5. Keystroke re-filter on 50,000 items (multi-core search_parallel)
    let items_50k = generate_items(50_000);
    let mut state_50k = DmenuState::from_request(&DmenuRequest {
        prompt: Some("Find 50k".to_string()),
        items: items_50k,
        case_mode: DmenuCaseMode::Smart,
        ..Default::default()
    });
    let t5 = Instant::now();
    state_50k.insert_text("cam");
    let refilter_50k_ms = t5.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[DmenuState 50k] Single keystroke filter ('cam', parallel): {:.2} ms (top_k={}, total_matched={})",
        refilter_50k_ms,
        state_50k.filtered_entries().len(),
        state_50k.total_matched
    );

    // 6. Keystroke re-filter on 100,000 items (multi-core search_parallel)
    let items_100k = generate_items(100_000);
    let mut state_100k = DmenuState::from_request(&DmenuRequest {
        prompt: Some("Find 100k".to_string()),
        items: items_100k.clone(),
        case_mode: DmenuCaseMode::Smart,
        ..Default::default()
    });
    let t6 = Instant::now();
    state_100k.insert_text("cam");
    let refilter_100k_ms = t6.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[DmenuState 100k] Single keystroke filter ('cam', parallel): {:.2} ms (top_k={}, total_matched={})",
        refilter_100k_ms,
        state_100k.filtered_entries().len(),
        state_100k.total_matched
    );

    // 7. Async SearchSession rapid typing burst ("c" -> "ca" -> "cam" -> "came" -> "camer" -> "camera")
    // on 100,000 items measuring UI-thread blocking time per keystroke (latest-wins cancellation).
    let session = SearchSession::<String>::new(SearchConfig {
        limit: 500,
        inline_budget: Duration::from_micros(1_200),
        inline_max_items: 512,
        case_matching: CaseMatching::Smart,
        ..SearchConfig::default()
    });
    let corpus: Arc<[String]> = Arc::from(items_100k.into_boxed_slice());
    let frecency = Arc::new(FrecencyMap::new());
    let burst = ["c", "ca", "cam", "came", "camer", "camera"];
    let mut keystroke_ms = Vec::with_capacity(burst.len());
    let mut last_handle = None;
    for q in burst {
        let tk = Instant::now();
        let h = session.search(Arc::clone(&corpus), q, Arc::clone(&frecency), 0);
        keystroke_ms.push(tk.elapsed().as_secs_f64() * 1000.0);
        last_handle = Some(h);
    }
    let avg_ui_ms: f64 = keystroke_ms.iter().sum::<f64>() / keystroke_ms.len() as f64;
    let max_ui_ms: f64 = keystroke_ms.iter().copied().fold(0.0_f64, f64::max);
    let mut final_matched = 0usize;
    let t_finish = Instant::now();
    if let Some(h) = last_handle {
        final_matched = h.initial.total_matched;
        if let Some(rx) = h.updates {
            while let Ok(snap) = rx.recv() {
                final_matched = snap.total_matched;
                if snap.is_complete {
                    break;
                }
            }
        }
    }
    let settle_ms = t_finish.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[DmenuWindow 100k] 6-char rapid typing burst UI latency: avg={:.2} ms, max={:.2} ms, final settle={:.2} ms (matched={})",
        avg_ui_ms, max_ui_ms, settle_ms, final_matched
    );
}
