//! End-to-end integration tests for Phase 4 Hybrid Plugin Runtime:
//! - Real WebAssembly Component Model (`wasmtime` + `wit/aios-plugin.wit`) plugins:
//!   1. `com.aios.quick-notes` (`List + Split Detail` + `LocalStorage` + `Toast` + capability-gated clipboard)
//!   2. `com.aios.palette-grid` (`Grid` + `Form` multi-command plugin)
//!   3. Infinite-loop WASM plugin terminated cleanly by fuel/epoch budget (`ErrorView` isolation)
//!   4. Trapping (`unreachable`) WASM plugin isolated cleanly into `ErrorView`
//! - Tier-3 Script Commands (`examples/scripts/system-uptime.sh`, `disk-usage.sh`, + confirmation gate)
//! - Settings & Extension Manager flows (install, preferences, capabilities, uninstall)
//! - `PluginDevWatcher` hot-reload detection

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aios_core::{AiosConfig, AiosPaths, Database, view::ViewNode};
use aios_plugin_host::{
    DevReloadEvent, HostSideEffect, PluginDevWatcher, PluginHost, ScriptMode, WasmExecutionLimits,
    discover_script_commands,
};
use aios_plugin_sdk::{
    ActionItem, ActionPanel, ActionPanelBuilder, FormField, FormViewBuilder, GridAspectRatio,
    GridItemBuilder, GridItemContent, GridViewBuilder, HudMessage, ListItemBuilder,
    ListViewBuilder, MetadataEntry, PluginEvent, PluginResponse,
};
use tempfile::TempDir;

/// Builds a real WebAssembly Component binary implementing `aios:plugin@0.1.0` (`aios-extension`)
/// that optionally invokes `host-api::show-toast`, `host-api::storage-set`, and
/// `host-api::clipboard-write-text`, returning deterministic `PluginResponse` JSON payloads.
fn build_wat_component(
    init_json: &str,
    event_json: &str,
    call_clipboard_on_event: bool,
    call_storage_and_toast_on_init: bool,
) -> Vec<u8> {
    let init_esc = wat_escape_bytes(init_json.as_bytes());
    let init_len = init_json.len();
    let event_esc = wat_escape_bytes(event_json.as_bytes());
    let event_len = event_json.len();

    let toast_title = b"Quick Notes Loaded";
    let toast_esc = wat_escape_bytes(toast_title);
    let toast_len = toast_title.len();

    let store_key = b"last_opened";
    let store_key_esc = wat_escape_bytes(store_key);
    let store_key_len = store_key.len();

    let store_val = b"v1-note";
    let store_val_esc = wat_escape_bytes(store_val);
    let store_val_len = store_val.len();

    let clip_text = b"Wayland Layer-Shell Checklist";
    let clip_esc = wat_escape_bytes(clip_text);
    let clip_len = clip_text.len();

    // Offsets in linear memory:
    // 1024.. : init_json
    // 16384..: event_json
    // 32768..: toast_title
    // 33024..: store_key
    // 33280..: store_val
    // 33536..: clip_text
    // 40960..: retptr scratch (for return value and host-call out-params)
    // 49152..: bump allocator for cabi_realloc
    let init_host_calls = if call_storage_and_toast_on_init {
        format!(
            r#"
        ;; show-toast(style=1(success), title_ptr, title_len, msg_disc=0, 0, 0)
        (call $host_show_toast (i32.const 1) (i32.const 32768) (i32.const {toast_len}) (i32.const 0) (i32.const 0) (i32.const 0))
        ;; storage-set(key_ptr, key_len, val_ptr, val_len, retptr=41200)
        (call $host_storage_set (i32.const 33024) (i32.const {store_key_len}) (i32.const 33280) (i32.const {store_val_len}) (i32.const 41200))
"#
        )
    } else {
        String::new()
    };

    let event_body = if call_clipboard_on_event {
        format!(
            r#"
        ;; clipboard-write-text(ptr, len, retptr=41400) -> result<_, string>
        (call $host_clipboard_write (i32.const 33536) (i32.const {clip_len}) (i32.const 41400))
        (if (i32.ne (i32.load8_u (i32.const 41400)) (i32.const 0))
          (then
            ;; Forward host capability error as Err(msg)
            (i32.store8 (i32.const 40960) (i32.const 1))
            (i32.store (i32.const 40964) (i32.load (i32.const 41404)))
            (i32.store (i32.const 40968) (i32.load (i32.const 41408)))
            (return (i32.const 40960))
          )
        )
        (i32.store8 (i32.const 40960) (i32.const 0))
        (i32.store (i32.const 40964) (i32.const 16384))
        (i32.store (i32.const 40968) (i32.const {event_len}))
        (i32.const 40960)
"#
        )
    } else {
        format!(
            r#"
        (i32.store8 (i32.const 40960) (i32.const 0))
        (i32.store (i32.const 40964) (i32.const 16384))
        (i32.store (i32.const 40968) (i32.const {event_len}))
        (i32.const 40960)
"#
        )
    };

    let wat = format!(
        r#"
(component
  (import "aios:plugin/host-api@0.1.0" (instance $host
    (type $toast-style_def (enum "animated" "success" "failure"))
    (export "toast-style" (type $toast-style (eq $toast-style_def)))
    (type $toast_def (record (field "style" $toast-style) (field "title" string) (field "message" (option string))))
    (export "toast" (type $toast (eq $toast_def)))
    (export "show-toast" (func (param "t" $toast)))
    (export "storage-set" (func (param "key" string) (param "val" (list u8)) (result (result (error string)))))
    (export "clipboard-write-text" (func (param "text" string) (result (result (error string)))))
  ))

  (core module $mem_mod
    (memory (export "memory") 2)
    (global $bump (mut i32) (i32.const 49152))
    (func (export "cabi_realloc") (param $old_ptr i32) (param $old_size i32) (param $align i32) (param $new_size i32) (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (i32.add (local.get $new_size) (i32.const 16))))
      (local.get $ptr)
    )
  )
  (core instance $mem_inst (instantiate $mem_mod))
  (alias core export $mem_inst "memory" (core memory $mem))
  (alias core export $mem_inst "cabi_realloc" (core func $realloc))

  (alias export $host "show-toast" (func $f_show_toast))
  (alias export $host "storage-set" (func $f_storage_set))
  (alias export $host "clipboard-write-text" (func $f_clip_write))

  (core func $c_show_toast (canon lower (func $f_show_toast) (memory $mem)))
  (core func $c_storage_set (canon lower (func $f_storage_set) (memory $mem) (realloc $realloc)))
  (core func $c_clip_write (canon lower (func $f_clip_write) (memory $mem) (realloc $realloc)))

  (core module $impl
    (import "env" "memory" (memory 2))
    (import "env" "show_toast" (func $host_show_toast (param i32 i32 i32 i32 i32 i32)))
    (import "env" "storage_set" (func $host_storage_set (param i32 i32 i32 i32 i32)))
    (import "env" "clipboard_write" (func $host_clipboard_write (param i32 i32 i32)))

    (data (i32.const 1024) "{init_esc}")
    (data (i32.const 16384) "{event_esc}")
    (data (i32.const 32768) "{toast_esc}")
    (data (i32.const 33024) "{store_key_esc}")
    (data (i32.const 33280) "{store_val_esc}")
    (data (i32.const 33536) "{clip_esc}")

    (func (export "init-command") (param i32 i32 i32 i32) (result i32)
      {init_host_calls}
      (i32.store8 (i32.const 40960) (i32.const 0))
      (i32.store (i32.const 40964) (i32.const 1024))
      (i32.store (i32.const 40968) (i32.const {init_len}))
      (i32.const 40960)
    )

    (func (export "handle-event") (param i32 i32) (result i32)
      {event_body}
    )

    (func (export "background-refresh") (param i32 i32) (result i32)
      (i32.store8 (i32.const 40960) (i32.const 0))
      (i32.store (i32.const 40964) (i32.const 1024))
      (i32.store (i32.const 40968) (i32.const {init_len}))
      (i32.const 40960)
    )

    (func (export "dispose"))
  )

  (core instance $impl_inst (instantiate $impl
    (with "env" (instance
      (export "memory" (memory $mem))
      (export "show_toast" (func $c_show_toast))
      (export "storage_set" (func $c_storage_set))
      (export "clipboard_write" (func $c_clip_write))
    ))
  ))

  (func $init_lifted (param "command-id" string) (param "args-json" string) (result (result string (error string)))
    (canon lift (core func $impl_inst "init-command") (memory $mem) (realloc $realloc))
  )
  (func $event_lifted (param "event-json" string) (result (result string (error string)))
    (canon lift (core func $impl_inst "handle-event") (memory $mem) (realloc $realloc))
  )
  (func $bg_lifted (param "command-id" string) (result (result string (error string)))
    (canon lift (core func $impl_inst "background-refresh") (memory $mem) (realloc $realloc))
  )
  (func $dispose_lifted
    (canon lift (core func $impl_inst "dispose"))
  )

  (instance $lifecycle_exp
    (export "init-command" (func $init_lifted))
    (export "handle-event" (func $event_lifted))
    (export "background-refresh" (func $bg_lifted))
    (export "dispose" (func $dispose_lifted))
  )
  (export "aios:plugin/lifecycle@0.1.0" (instance $lifecycle_exp))
)
"#
    );

    wat::parse_str(&wat).expect("valid component WAT")
}

/// Builds a misbehaving WASM component that either spins in an infinite loop or traps with `unreachable`.
fn build_faulty_wat_component(infinite_loop: bool) -> Vec<u8> {
    let body = if infinite_loop {
        "(loop $spin (br $spin)) (unreachable)"
    } else {
        "(unreachable)"
    };
    let wat = format!(
        r#"
(component
  (core module $impl
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 1024))
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (i32.const 64)))
      (local.get $ptr)
    )
    (func (export "init-command") (param i32 i32 i32 i32) (result i32)
      {body}
    )
    (func (export "handle-event") (param i32 i32) (result i32) (unreachable))
    (func (export "background-refresh") (param i32 i32) (result i32) (unreachable))
    (func (export "dispose"))
  )
  (core instance $inst (instantiate $impl))
  (alias core export $inst "memory" (core memory $mem))
  (alias core export $inst "cabi_realloc" (core func $realloc))

  (func $init_lifted (param "command-id" string) (param "args-json" string) (result (result string (error string)))
    (canon lift (core func $inst "init-command") (memory $mem) (realloc $realloc))
  )
  (func $event_lifted (param "event-json" string) (result (result string (error string)))
    (canon lift (core func $inst "handle-event") (memory $mem) (realloc $realloc))
  )
  (func $bg_lifted (param "command-id" string) (result (result string (error string)))
    (canon lift (core func $inst "background-refresh") (memory $mem) (realloc $realloc))
  )
  (func $dispose_lifted (canon lift (core func $inst "dispose")))

  (instance $lifecycle_exp
    (export "init-command" (func $init_lifted))
    (export "handle-event" (func $event_lifted))
    (export "background-refresh" (func $bg_lifted))
    (export "dispose" (func $dispose_lifted))
  )
  (export "aios:plugin/lifecycle@0.1.0" (instance $lifecycle_exp))
)
"#
    );
    wat::parse_str(&wat).expect("valid faulty component WAT")
}

fn wat_escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7E => out.push(b as char),
            other => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\{other:02x}");
            }
        }
    }
    out
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn test_host(tmp: &TempDir) -> (AiosPaths, Arc<Mutex<Database>>, PluginHost) {
    let paths = AiosPaths::from_roots(
        tmp.path(),
        Some(&tmp.path().join("config")),
        Some(&tmp.path().join("data")),
        Some(&tmp.path().join("cache")),
        Some(&tmp.path().join("run")),
    );
    paths.ensure_dirs().expect("create XDG dirs");
    let db = Arc::new(Mutex::new(
        Database::open(&paths.database_file()).expect("open sqlite db"),
    ));
    let host = PluginHost::new(
        AiosConfig::default(),
        paths.clone(),
        Arc::clone(&db),
        WasmExecutionLimits {
            max_memory_bytes: 16 * 1024 * 1024,
            fuel_budget: 2_000_000,
            epoch_deadline_ticks: 40,
        },
    )
    .expect("init plugin host");
    (paths, db, host)
}

#[test]
fn quick_notes_list_detail_wasm_plugin_and_capability_gate_work_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, db, mut host) = test_host(&tmp);

    // Stage reference plugin 1 bundle using the real manifest from examples/plugins/quick-notes-wasm.
    let bundle_dir = tmp.path().join("stage-quick-notes");
    fs::create_dir_all(&bundle_dir).expect("mkdir stage");
    fs::copy(
        repo_root().join("examples/plugins/quick-notes-wasm/aios-plugin.toml"),
        bundle_dir.join("aios-plugin.toml"),
    )
    .expect("copy quick-notes manifest");

    let list_view = ListViewBuilder::new("Quick Notes (WASM)")
        .showing_detail(true)
        .section(
            Some("Saved Notes"),
            Some("1 note"),
            vec![
                ListItemBuilder::new("note-1", "Wayland Layer-Shell Checklist")
                    .subtitle("inbox")
                    .icon("NT")
                    .detail(
                        "# Wayland Layer-Shell Checklist\n\n- Hyprland blur rule",
                        vec![MetadataEntry::Label {
                            title: "Folder".to_string(),
                            text: "inbox".to_string(),
                        }],
                    )
                    .actions(
                        ActionPanelBuilder::new()
                            .action(ActionItem::new("note.copy", "Copy Note").shortcut("Enter"))
                            .build(),
                    )
                    .build(),
            ],
        )
        .build();
    let init_json = PluginResponse::render(list_view)
        .to_json()
        .expect("serialize list view");
    let event_json = PluginResponse::ShowHud {
        hud: HudMessage {
            title: "Copied Quick Note".to_string(),
        },
    }
    .to_json()
    .expect("serialize hud");

    let wasm_bytes = build_wat_component(&init_json, &event_json, true, true);
    fs::write(bundle_dir.join("plugin.wasm"), wasm_bytes).expect("write plugin.wasm");

    // Install into XDG plugins directory and verify Root Search projection.
    let installed = host
        .install_local_plugin(&bundle_dir)
        .expect("install quick-notes");
    assert_eq!(installed.manifest.id, "com.aios.quick-notes");
    assert!(
        host.root_search_entries()
            .iter()
            .any(|e| e.id == "plugin:com.aios.quick-notes:browse")
    );

    // 1. Run `browse` -> renders List+Detail, persists LocalStorage, and emits Toast side-effect!
    let res = host.run_wasm_command("com.aios.quick-notes", "browse", "[]", "dark");
    assert!(
        matches!(
            res.response,
            PluginResponse::Render { ref view } if matches!(view.as_ref(), ViewNode::List(l) if l.is_showing_detail)
        ),
        "unexpected browse response: {:#?}",
        res.response
    );
    assert!(res.side_effects.iter().any(|s| matches!(
        s,
        HostSideEffect::ShowToast(t) if t.title == "Quick Notes Loaded"
    )));
    let stored = db
        .lock()
        .expect("lock db")
        .plugin_storage_get("com.aios.quick-notes", "last_opened")
        .expect("read storage");
    assert_eq!(stored, Some(b"v1-note".to_vec()));

    // 2. Trigger clipboard action BEFORE approving `clipboard_write` -> denied cleanly into ErrorView!
    let event = PluginEvent::ActionTriggered {
        command_id: "browse".to_string(),
        action_id: "note.copy".to_string(),
        item_id: Some("note-1".to_string()),
        inline_args: Vec::new(),
    };
    let denied = host.send_wasm_event("com.aios.quick-notes", "browse", &event, "dark");
    assert!(matches!(
        denied.response,
        PluginResponse::Render { ref view } if matches!(view.as_ref(), ViewNode::Error(e) if e.detail.as_deref().unwrap_or("").contains("not been approved"))
    ));

    // 3. Approve `clipboard_write` via Settings & Extension Manager action -> now succeeds!
    let grant_res = host
        .dispatch_host_action(
            "settings.cap-set:com.aios.quick-notes:clipboard_write:grant",
            &[],
            None,
            "dark",
        )
        .expect("cap grant action");
    assert!(matches!(grant_res.response, PluginResponse::Render { .. }));

    let allowed = host.send_wasm_event("com.aios.quick-notes", "browse", &event, "dark");
    assert!(matches!(allowed.response, PluginResponse::ShowHud { .. }));
    assert!(allowed.side_effects.iter().any(|s| matches!(
        s,
        HostSideEffect::ClipboardWrite(txt) if txt == "Wayland Layer-Shell Checklist"
    )));
}

#[test]
fn palette_grid_and_form_wasm_plugin_runs_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, _db, mut host) = test_host(&tmp);

    let bundle_dir = tmp.path().join("stage-palette-grid");
    fs::create_dir_all(&bundle_dir).expect("mkdir stage");
    fs::copy(
        repo_root().join("examples/plugins/palette-grid-wasm/aios-plugin.toml"),
        bundle_dir.join("aios-plugin.toml"),
    )
    .expect("copy palette-grid manifest");

    let grid_view = GridViewBuilder::new("Palette Swatches (WASM)", 4)
        .aspect_ratio(GridAspectRatio::Square)
        .section(
            Some("AIOS Palette"),
            None,
            vec![
                GridItemBuilder::new(
                    "sw-copper",
                    "Signal Copper",
                    GridItemContent::ColorSwatch {
                        hex: "#E87143".to_string(),
                    },
                )
                .subtitle("#E87143")
                .actions(ActionPanel::from_actions(vec![
                    ActionItem::new("open.form", "Create Swatch").shortcut("Enter"),
                ]))
                .build(),
            ],
        )
        .build();

    let form_view = FormViewBuilder::new("Create Custom Swatch (WASM)")
        .field(FormField::TextField {
            id: "name".to_string(),
            label: "Swatch Name".to_string(),
            placeholder: Some("Slate".to_string()),
            value: String::new(),
            required: true,
            error: None,
        })
        .actions(ActionPanel::from_actions(vec![
            ActionItem::new("swatch.save", "Save Swatch").shortcut("Enter"),
        ]))
        .build();

    let init_json = PluginResponse::render(grid_view)
        .to_json()
        .expect("serialize grid");
    let event_json = PluginResponse::push(Some("palette.form".to_string()), form_view)
        .to_json()
        .expect("serialize form push");

    fs::write(
        bundle_dir.join("plugin.wasm"),
        build_wat_component(&init_json, &event_json, false, false),
    )
    .expect("write plugin.wasm");

    host.install_local_plugin(&bundle_dir)
        .expect("install palette-grid");

    let res = host.run_wasm_command("com.aios.palette-grid", "swatches", "[]", "light");
    assert!(matches!(
        res.response,
        PluginResponse::Render { ref view } if matches!(view.as_ref(), ViewNode::Grid(g) if g.columns == 4)
    ));

    let push_form = host.send_wasm_event(
        "com.aios.palette-grid",
        "swatches",
        &PluginEvent::ActionTriggered {
            command_id: "swatches".to_string(),
            action_id: "open.form".to_string(),
            item_id: Some("sw-copper".to_string()),
            inline_args: Vec::new(),
        },
        "light",
    );
    assert!(matches!(
        push_form.response,
        PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::Form(_))
    ));
}

#[test]
fn wasm_infinite_loop_and_trap_are_isolated_into_error_view() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, _db, mut host) = test_host(&tmp);

    // 1. Infinite loop plugin aborted deterministically by Wasmtime fuel budget.
    let loop_dir = tmp.path().join("stage-loop");
    fs::create_dir_all(&loop_dir).expect("mkdir loop");
    fs::write(
        loop_dir.join("aios-plugin.toml"),
        r#"
[plugin]
id = "com.test.infinite-loop"
name = "Infinite Loop Test"
version = "0.1.0"
entry = "plugin.wasm"
[[commands]]
id = "hang"
title = "Hang Forever"
mode = "view"
"#,
    )
    .expect("write manifest");
    fs::write(
        loop_dir.join("plugin.wasm"),
        build_faulty_wat_component(true),
    )
    .expect("write loop wasm");
    host.install_local_plugin(&loop_dir)
        .expect("install loop plugin");

    let loop_res = host.run_wasm_command("com.test.infinite-loop", "hang", "[]", "dark");
    assert!(
        matches!(
            loop_res.response,
            PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::Error(e) if e.detail.as_deref().unwrap_or("").contains("fuel"))
        ),
        "unexpected loop response: {:#?}",
        loop_res.response
    );

    // 2. Trapping plugin (`unreachable`) isolated safely without crashing host.
    let trap_dir = tmp.path().join("stage-trap");
    fs::create_dir_all(&trap_dir).expect("mkdir trap");
    fs::write(
        trap_dir.join("aios-plugin.toml"),
        r#"
[plugin]
id = "com.test.trap"
name = "Trap Test"
version = "0.1.0"
entry = "plugin.wasm"
[[commands]]
id = "boom"
title = "Trigger Trap"
mode = "view"
"#,
    )
    .expect("write trap manifest");
    fs::write(
        trap_dir.join("plugin.wasm"),
        build_faulty_wat_component(false),
    )
    .expect("write trap wasm");
    host.install_local_plugin(&trap_dir)
        .expect("install trap plugin");

    let trap_res = host.run_wasm_command("com.test.trap", "boom", "[]", "dark");
    assert!(matches!(
        trap_res.response,
        PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::Error(e) if e.detail.as_deref().unwrap_or("").contains("WASM trap"))
    ));
}

#[test]
fn reference_script_commands_and_confirmation_flow_run_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (paths, _db, mut host) = test_host(&tmp);

    // Copy both reference scripts from examples/scripts/ into temp XDG scripts dir.
    for file in ["system-uptime.sh", "disk-usage.sh"] {
        let src = repo_root().join("examples/scripts").join(file);
        let dst = paths.scripts_dir().join(file);
        fs::copy(&src, &dst).expect("copy script");
    }
    // Add a destructive script requiring explicit confirmation.
    fs::write(
        paths.scripts_dir().join("danger-clean.sh"),
        "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Clean Temp Cache\n# @aios.mode silent\n# @aios.needsConfirmation true\necho \"Cleaned $1\"\n",
    )
    .expect("write confirm script");

    let discovered = discover_script_commands(&paths.scripts_dir());
    assert_eq!(discovered.len(), 3);
    assert!(
        discovered
            .iter()
            .any(|s| s.id == "script:system-uptime" && s.mode == ScriptMode::Inline)
    );
    assert!(discovered.iter().any(|s| s.id == "script:disk-usage"
        && s.mode == ScriptMode::FullOutput
        && s.arguments.len() == 1));

    host.reload_all();
    // Inline script summary should already be populated in Root Search entries.
    let uptime_entry = host
        .root_search_entries()
        .into_iter()
        .find(|e| e.id == "script:system-uptime")
        .expect("uptime root entry");
    assert!(uptime_entry.inline_summary.is_some());

    // Run `disk-usage` (fullOutput) with argument `/tmp` -> returns DetailView with Copy Output.
    let du_res = host.run_script("script:disk-usage", &["/tmp".to_string()], false);
    assert!(matches!(
        du_res.response,
        PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::Detail(d) if d.markdown.contains("/tmp"))
    ));

    // Run `danger-clean` -> first returns ConfirmAlert, then executes after confirmation.
    let confirm_req = host.run_script("script:danger-clean", &["cache".to_string()], false);
    let alert_id = match confirm_req.response {
        PluginResponse::Confirm { alert } => alert.id,
        other => panic!("expected ConfirmAlert, got {other:?}"),
    };
    let after_confirm = host
        .dispatch_host_action(&alert_id, &[], None, "dark")
        .expect("confirm execution");
    assert!(matches!(
        after_confirm.response,
        PluginResponse::CloseWindow { hud: Some(h) } if h.title == "Cleaned cache"
    ));
}

#[test]
fn settings_extension_manager_and_dev_watcher_work_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, db, mut host) = test_host(&tmp);

    // Prepare a local dev plugin directory.
    let dev_dir = tmp.path().join("my-dev-plugin");
    fs::create_dir_all(&dev_dir).expect("mkdir dev");
    fs::copy(
        repo_root().join("examples/plugins/quick-notes-wasm/aios-plugin.toml"),
        dev_dir.join("aios-plugin.toml"),
    )
    .expect("copy manifest");
    let ok_json = PluginResponse::Noop.to_json().expect("noop json");
    fs::write(
        dev_dir.join("plugin.wasm"),
        build_wat_component(&ok_json, &ok_json, false, false),
    )
    .expect("write wasm");

    // 1. Hot-reload watcher detects updated manifest.
    let mut watcher = PluginDevWatcher::new(&dev_dir, Duration::from_millis(20));
    let initial = watcher.load_initial().expect("initial load");
    assert_eq!(initial.manifest.version, "0.1.0");
    assert!(watcher.poll_change().is_none());

    let updated_manifest = fs::read_to_string(dev_dir.join("aios-plugin.toml"))
        .expect("read manifest")
        .replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    // Ensure size/mtime changes unambiguously
    fs::write(
        dev_dir.join("aios-plugin.toml"),
        format!("{updated_manifest}\n# reloaded\n"),
    )
    .expect("rewrite manifest");
    let ev = watcher.poll_change().expect("expected reload event");
    assert!(matches!(ev, DevReloadEvent::Reloaded(b) if b.manifest.version == "0.2.0"));

    // 2. Install via Settings Form submit action, save preference, then uninstall cleanly.
    let mut form_vals = BTreeMap::new();
    form_vals.insert("source_path".to_string(), dev_dir.display().to_string());
    let install_res = host
        .dispatch_host_action(
            "settings.install-plugin-submit",
            &[],
            Some(&form_vals),
            "dark",
        )
        .expect("install submit");
    assert!(matches!(install_res.response, PluginResponse::Pop));
    assert_eq!(host.plugins().len(), 1);

    let mut pref_vals = BTreeMap::new();
    pref_vals.insert("default_folder".to_string(), "projects".to_string());
    let pref_res = host
        .dispatch_host_action(
            "settings.plugin-prefs-save:com.aios.quick-notes",
            &[],
            Some(&pref_vals),
            "dark",
        )
        .expect("save prefs");
    assert!(matches!(pref_res.response, PluginResponse::Pop));
    assert_eq!(
        db.lock()
            .expect("lock db")
            .get_plugin_preference("com.aios.quick-notes", "default_folder")
            .expect("read pref"),
        Some("projects".to_string())
    );

    // Settings home view lists the installed plugin.
    let home = host.build_settings_view();
    assert!(matches!(home, ViewNode::List(l) if l.sections.len() >= 2));

    // Uninstall prompts ConfirmAlert, then removes bundle & purges SQLite state.
    let prompt = host
        .dispatch_host_action(
            "settings.plugin-uninstall-prompt:com.aios.quick-notes",
            &[],
            None,
            "dark",
        )
        .expect("uninstall prompt");
    assert!(matches!(prompt.response, PluginResponse::Confirm { .. }));

    let confirmed = host
        .dispatch_host_action(
            "settings.plugin-uninstall-confirm:com.aios.quick-notes",
            &[],
            None,
            "dark",
        )
        .expect("uninstall confirm");
    assert!(matches!(confirmed.response, PluginResponse::Render { .. }));
    assert!(host.plugins().is_empty());
    assert_eq!(
        db.lock()
            .expect("lock db")
            .get_plugin_preference("com.aios.quick-notes", "default_folder")
            .expect("pref purged"),
        None
    );
}
