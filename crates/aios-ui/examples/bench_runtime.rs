//! Full-stack release benchmark covering AIOS runtime subsystems:
//! 1. XDG `.desktop` + AppImage scanner + icon resolution (`aios-builtin`)
//! 2. SQLite WAL frecency & KV storage throughput (`aios-core`)
//! 3. Unix domain socket daemon IPC round-trip latency (`aios-wayland`)
//! 4. Wasmtime Component Model cold compile vs warm cached plugin execution (`aios-plugin-host`)
//!
//! Run via:
//! `cargo run --release -p aios-ui --example bench_runtime`

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use aios_core::{AiosConfig, AiosPaths, Database};
use aios_daemon::{DaemonIpcCommand, DaemonIpcResponse, DmenuRequest, DmenuStreamMessage};
use aios_plugin_host::{PluginHost, WasmExecutionLimits};
use aios_plugin_sdk::PluginEvent;
use aios_wayland::{IpcClient, IpcServer};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn main() {
    println!("=== AIOS Core, IPC & WASM Runtime Release Benchmark ===");

    // 0. XDG `.desktop` + AppImage scanner & icon resolver on host OS
    let t_scan = Instant::now();
    let apps = aios_builtin::scan_desktop_applications();
    let scan_ms = t_scan.elapsed().as_secs_f64() * 1000.0;
    let apps_with_icons = apps.iter().filter(|a| a.icon_path.is_some()).count();
    println!(
        "[Desktop Scan] XDG .desktop + AppImage scan & icon resolve: {:.2} ms ({} apps, {} with icons)",
        scan_ms,
        apps.len(),
        apps_with_icons
    );

    // 1. SQLite WAL open, migration, frecency write & read throughput
    let tmp = TempDir::new().expect("temp dir");
    let paths = AiosPaths::from_roots(
        tmp.path(),
        Some(&tmp.path().join("config")),
        Some(&tmp.path().join("data")),
        Some(&tmp.path().join("cache")),
        Some(&tmp.path().join("run")),
    );
    paths.ensure_dirs().expect("ensure dirs");

    let t_db_open = Instant::now();
    let db = Database::open(&paths.database_file()).expect("open sqlite db");
    let db_open_ms = t_db_open.elapsed().as_secs_f64() * 1000.0;

    let t_db_write = Instant::now();
    for i in 0..500 {
        db.record_frecency_hit(&format!("app:item-{i}"), "fi", 1_700_000_000 + i)
            .expect("record hit");
    }
    let db_write_ms = t_db_write.elapsed().as_secs_f64() * 1000.0;

    let t_db_read = Instant::now();
    for _ in 0..1_000 {
        let _ = db.load_frecency_records("fi").expect("load frecency");
    }
    let db_read_ms = t_db_read.elapsed().as_secs_f64() * 1000.0;

    println!(
        "[SQLite WAL] Open + migrations: {:.2} ms | 500 frecency writes: {:.2} ms ({:.1} us/write) | 1,000 query loads: {:.2} ms ({:.1} us/load)",
        db_open_ms,
        db_write_ms,
        db_write_ms * 1000.0 / 500.0,
        db_read_ms,
        db_read_ms * 1000.0 / 1000.0,
    );

    // 2. Unix socket IPC latency (`Ping`, `Toggle`, streaming `Dmenu` 5x256 batch + select)
    let sock = tmp.path().join("bench.sock");
    let (_server, rx) = IpcServer::bind(&sock).expect("bind ipc");
    let client = IpcClient::new(&sock);

    let t_ping = Instant::now();
    for _ in 0..500 {
        let _ = client.send(&DaemonIpcCommand::Ping).expect("ping");
    }
    let ping_us = t_ping.elapsed().as_secs_f64() * 1_000_000.0 / 500.0;

    let handler = thread::spawn(move || {
        for _ in 0..200 {
            let env = rx.recv_blocking().expect("toggle env");
            env.respond(DaemonIpcResponse::Ok);
        }
        // Handle 1 streaming dmenu session receiving 10 batches of 256 items
        let env = rx.recv_blocking().expect("dmenu env");
        let stream_rx = env.dmenu_stream_rx.clone().expect("stream rx");
        let mut received = 0usize;
        while let Ok(msg) = stream_rx.recv_blocking() {
            match msg {
                DmenuStreamMessage::Batch { items, .. } => received += items.len(),
                DmenuStreamMessage::Eof => break,
            }
        }
        env.respond(DaemonIpcResponse::DmenuSelected {
            value: format!("items={received}"),
            index: Some(0),
        });
    });

    let t_toggle = Instant::now();
    for _ in 0..200 {
        let _ = client.send(&DaemonIpcCommand::Toggle).expect("toggle");
    }
    let toggle_us = t_toggle.elapsed().as_secs_f64() * 1_000_000.0 / 200.0;

    let t_stream = Instant::now();
    let session = client
        .start_dmenu_stream(&DmenuRequest {
            prompt: Some("Bench".to_string()),
            streaming: true,
            ..Default::default()
        })
        .expect("start stream");
    let (mut writer, reader) = session.split();
    let sample_batch: Vec<String> = (0..256).map(|i| format!("item-{i:04}")).collect();
    for _ in 0..10 {
        let _ = writer
            .send_stream(&DmenuStreamMessage::Batch {
                items: sample_batch.clone(),
                item_icons: Vec::new(),
            })
            .expect("send batch");
    }
    let _ = writer.send_stream(&DmenuStreamMessage::Eof).expect("eof");
    let _ = reader.wait_response().expect("dmenu reply");
    let stream_ms = t_stream.elapsed().as_secs_f64() * 1000.0;
    handler.join().expect("ipc handler");

    println!(
        "[Wayland IPC] Ping avg: {:.1} us | Toggle round-trip avg: {:.1} us | Streaming dmenu (2,560 items / 10 batches): {:.2} ms",
        ping_us, toggle_us, stream_ms
    );

    // 3. Wasmtime Component Model cold compile vs warm cached calls (`com.aios.calculator`)
    let shared_db = Arc::new(Mutex::new(db));
    let mut host = PluginHost::new(
        AiosConfig::default(),
        paths,
        shared_db,
        WasmExecutionLimits::default(),
    )
    .expect("plugin host");

    let calc_dir = repo_root().join("plugins/com.aios.calculator");
    if !calc_dir.join("plugin.wasm").is_file() {
        let status = std::process::Command::new("cargo")
            .args([
                "build",
                "--manifest-path",
                calc_dir.join("Cargo.toml").to_str().expect("utf8 path"),
                "--target",
                "wasm32-wasip2",
                "--release",
            ])
            .status()
            .expect("build com.aios.calculator wasm");
        assert!(status.success(), "wasm32-wasip2 build failed");
        std::fs::copy(
            calc_dir.join("target/wasm32-wasip2/release/aios_plugin_calculator.wasm"),
            calc_dir.join("plugin.wasm"),
        )
        .expect("copy plugin.wasm");
    }
    host.install_local_plugin(&calc_dir)
        .expect("install com.aios.calculator");

    let t_cold = Instant::now();
    let _ = host.run_wasm_command("com.aios.calculator", "calculator", "[]", "dark");
    let wasm_cold_ms = t_cold.elapsed().as_secs_f64() * 1000.0;

    let t_warm = Instant::now();
    for _ in 0..50 {
        let _ = host.run_wasm_command("com.aios.calculator", "calculator", "[]", "dark");
    }
    let wasm_warm_ms = t_warm.elapsed().as_secs_f64() * 1000.0 / 50.0;

    let ev = PluginEvent::SearchTextChanged {
        command_id: "calculator".to_string(),
        query: "24 * 12".to_string(),
    };
    let t_ev = Instant::now();
    for _ in 0..50 {
        let _ = host.send_wasm_event("com.aios.calculator", "calculator", &ev, "dark");
    }
    let wasm_ev_ms = t_ev.elapsed().as_secs_f64() * 1000.0 / 50.0;

    println!(
        "[Wasmtime WASM] Cold compile + init: {:.2} ms | Warm cached init avg (50x): {:.2} ms | Warm handle-event avg (50x): {:.2} ms",
        wasm_cold_ms, wasm_warm_ms, wasm_ev_ms
    );
}
