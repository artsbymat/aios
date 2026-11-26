//! End-to-end integration tests for Phase 4 Hybrid Plugin Runtime:
//! - Real WebAssembly Component Model (`wasmtime` + `wit/aios-plugin.wit`) plugins:
//!   1. `com.test.list-detail` (`List + Split Detail` + `LocalStorage` + `Toast` + capability-gated clipboard)
//!   2. `com.test.grid-form` (`Grid` + `Form` multi-command plugin)
//!   3. Infinite-loop WASM plugin terminated cleanly by fuel/epoch budget (`ErrorView` isolation)
//!   4. Trapping (`unreachable`) WASM plugin isolated cleanly into `ErrorView`
//! - Tier-3 Script Commands (`examples/scripts/system-uptime.sh`, `disk-usage.sh`, + confirmation gate)
//! - Settings & Extension Manager flows (install, preferences, capabilities, uninstall)
//! - `PluginDevWatcher` hot-reload detection

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aios_core::{AiosConfig, AiosPaths, Database, view::ViewNode};
use aios_plugin_host::{
    DevReloadEvent, HostSideEffect, PluginDevWatcher, PluginHost, WasmExecutionLimits,
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

    let toast_title = b"List Detail Loaded";
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

const TEST_LIST_DETAIL_MANIFEST: &str = r#"
[plugin]
id = "com.test.list-detail"
name = "List Detail Fixture"
version = "0.1.0"
description = "Test fixture exercising List+Detail, LocalStorage, and clipboard capability gating."
author = "AIOS"
min_aios_version = "0.1.0"
entry = "plugin.wasm"

[capabilities]
clipboard_write = true

[[preferences]]
name = "default_folder"
title = "Default Folder"
kind = "textfield"
default = "inbox"

[[commands]]
id = "browse"
title = "Browse Items"
subtitle = "List Detail Fixture"
mode = "view"
default_view = "list"
"#;

#[test]
fn list_detail_wasm_plugin_and_capability_gate_work_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, db, mut host) = test_host(&tmp);

    // Stage synthetic List+Detail plugin fixture in temp dir.
    let bundle_dir = tmp.path().join("stage-list-detail");
    fs::create_dir_all(&bundle_dir).expect("mkdir stage");
    fs::write(
        bundle_dir.join("aios-plugin.toml"),
        TEST_LIST_DETAIL_MANIFEST,
    )
    .expect("write list-detail manifest");

    let list_view = ListViewBuilder::new("List Detail Fixture (WASM)")
        .showing_detail(true)
        .section(
            Some("Saved Items"),
            Some("1 item"),
            vec![
                ListItemBuilder::new("item-1", "Wayland Layer-Shell Checklist")
                    .subtitle("inbox")
                    .icon("LD")
                    .detail(
                        "# Wayland Layer-Shell Checklist\n\n- Hyprland blur rule",
                        vec![MetadataEntry::Label {
                            title: "Folder".to_string(),
                            text: "inbox".to_string(),
                        }],
                    )
                    .actions(
                        ActionPanelBuilder::new()
                            .action(ActionItem::new("item.copy", "Copy Item").shortcut("Enter"))
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
            title: "Copied Item".to_string(),
        },
    }
    .to_json()
    .expect("serialize hud");

    let wasm_bytes = build_wat_component(&init_json, &event_json, true, true);
    fs::write(bundle_dir.join("plugin.wasm"), wasm_bytes).expect("write plugin.wasm");

    // Install into XDG plugins directory and verify Root Search projection.
    let installed = host
        .install_local_plugin(&bundle_dir)
        .expect("install list-detail fixture");
    assert_eq!(installed.manifest.id, "com.test.list-detail");
    assert!(
        host.root_search_entries()
            .iter()
            .any(|e| e.id == "plugin:com.test.list-detail:browse")
    );

    // 1. Run `browse` -> pushes List+Detail frame, persists LocalStorage, and emits Toast side-effect!
    let res = host.run_wasm_command("com.test.list-detail", "browse", "[]", "dark");
    assert!(
        matches!(
            res.response,
            PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::List(l) if l.is_showing_detail)
        ),
        "unexpected browse response: {:#?}",
        res.response
    );
    assert!(res.side_effects.iter().any(|s| matches!(
        s,
        HostSideEffect::ShowToast(t) if t.title == "List Detail Loaded"
    )));
    let stored = db
        .lock()
        .expect("lock db")
        .plugin_storage_get("com.test.list-detail", "last_opened")
        .expect("read storage");
    assert_eq!(stored, Some(b"v1-note".to_vec()));

    // 2. Trigger clipboard action BEFORE approving `clipboard_write` -> denied cleanly into ErrorView!
    let event = PluginEvent::ActionTriggered {
        command_id: "browse".to_string(),
        action_id: "item.copy".to_string(),
        item_id: Some("item-1".to_string()),
        inline_args: Vec::new(),
    };
    let denied = host.send_wasm_event("com.test.list-detail", "browse", &event, "dark");
    assert!(matches!(
        denied.response,
        PluginResponse::Render { ref view } if matches!(view.as_ref(), ViewNode::Error(e) if e.detail.as_deref().unwrap_or("").contains("not been approved"))
    ));

    // 3. Approve `clipboard_write` via Settings & Extension Manager action -> now succeeds!
    let grant_res = host
        .dispatch_host_action(
            "settings.cap-set:com.test.list-detail:clipboard_write:grant",
            &[],
            None,
            "dark",
        )
        .expect("cap grant action");
    assert!(matches!(grant_res.response, PluginResponse::Render { .. }));

    let allowed = host.send_wasm_event("com.test.list-detail", "browse", &event, "dark");
    assert!(matches!(allowed.response, PluginResponse::ShowHud { .. }));
    assert!(allowed.side_effects.iter().any(|s| matches!(
        s,
        HostSideEffect::ClipboardWrite(txt) if txt == "Wayland Layer-Shell Checklist"
    )));
}

#[test]
fn grid_and_form_wasm_plugin_runs_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, _db, mut host) = test_host(&tmp);

    let bundle_dir = tmp.path().join("stage-grid-form");
    fs::create_dir_all(&bundle_dir).expect("mkdir stage");
    fs::write(
        bundle_dir.join("aios-plugin.toml"),
        r#"
[plugin]
id = "com.test.grid-form"
name = "Grid Form Test"
version = "0.1.0"
author = "AIOS Test"
description = "Synthetic Grid + Form WASM fixture"
min_aios_version = "0.1.0"
entry = "plugin.wasm"

[capabilities]
clipboard_write = false
clipboard_read = false
http_hosts = []
fs_read = []
secrets = false

[[commands]]
id = "swatches"
title = "Browse Swatches"
mode = "view"
default_view = "grid"
"#,
    )
    .expect("write grid-form manifest");

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
    let event_json = PluginResponse::push(Some("grid.form".to_string()), form_view)
        .to_json()
        .expect("serialize form push");

    fs::write(
        bundle_dir.join("plugin.wasm"),
        build_wat_component(&init_json, &event_json, false, false),
    )
    .expect("write plugin.wasm");

    host.install_local_plugin(&bundle_dir)
        .expect("install grid-form");

    let res = host.run_wasm_command("com.test.grid-form", "swatches", "[]", "light");
    assert!(matches!(
        res.response,
        PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::Grid(g) if g.columns == 4)
    ));

    let push_form = host.send_wasm_event(
        "com.test.grid-form",
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
fn settings_extension_manager_and_dev_watcher_work_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, db, mut host) = test_host(&tmp);

    // Prepare a local dev plugin directory.
    let dev_dir = tmp.path().join("my-dev-plugin");
    fs::create_dir_all(&dev_dir).expect("mkdir dev");
    fs::write(dev_dir.join("aios-plugin.toml"), TEST_LIST_DETAIL_MANIFEST)
        .expect("write dev manifest");
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
            "settings.plugin-prefs-save:com.test.list-detail",
            &[],
            Some(&pref_vals),
            "dark",
        )
        .expect("save prefs");
    assert!(matches!(pref_res.response, PluginResponse::Pop));
    assert_eq!(
        db.lock()
            .expect("lock db")
            .get_plugin_preference("com.test.list-detail", "default_folder")
            .expect("read pref"),
        Some("projects".to_string())
    );

    // Settings home view lists the installed plugin.
    let home = host.build_settings_view();
    assert!(matches!(home, ViewNode::List(l) if l.sections.len() >= 2));

    // Uninstall prompts ConfirmAlert, then removes bundle & purges SQLite state.
    let prompt = host
        .dispatch_host_action(
            "settings.plugin-uninstall-prompt:com.test.list-detail",
            &[],
            None,
            "dark",
        )
        .expect("uninstall prompt");
    assert!(matches!(prompt.response, PluginResponse::Confirm { .. }));

    let confirmed = host
        .dispatch_host_action(
            "settings.plugin-uninstall-confirm:com.test.list-detail",
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
            .get_plugin_preference("com.test.list-detail", "default_folder")
            .expect("pref purged"),
        None
    );
}

#[test]
fn inline_calculator_copy_action_copies_and_closes_main_window() {
    let tmp = TempDir::new().expect("temp dir");
    let (_paths, _db, mut host) = test_host(&tmp);

    let inline = host.evaluate_inline_query("10*30", "dark");
    assert_eq!(inline.len(), 1);
    assert_eq!(inline[0].copy_value, "300");

    let dispatched = host
        .dispatch_host_action(&inline[0].copy_action_id, &[], None, "dark")
        .expect("inline copy action handled");
    assert!(matches!(
        dispatched.response,
        PluginResponse::CopyToClipboard { ref text, .. } if text == "300"
    ));
    assert!(
        dispatched
            .side_effects
            .iter()
            .any(|e| matches!(e, aios_plugin_host::HostSideEffect::CloseMainWindow)),
        "expected CloseMainWindow side effect for Root Search inline copy, got {:?}",
        dispatched.side_effects
    );

    let (icon, badge) = host
        .plugin_icon_for("com.aios.calculator")
        .expect("calculator plugin icon");
    assert_eq!(
        icon.as_deref(),
        Some("asset://icons/plugins/calculator.svg")
    );
    assert_eq!(badge, "=");
}

fn ensure_calculator_wasm_bundle() -> PathBuf {
    use std::process::Command;
    use std::sync::Once;

    static BUILD_ONCE: Once = Once::new();
    let plugin_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/com.aios.calculator");
    BUILD_ONCE.call_once(|| {
        let wasm_out = plugin_dir.join("plugin.wasm");
        if wasm_out.is_file() {
            return;
        }
        let status = Command::new("cargo")
            .args([
                "build",
                "--manifest-path",
                plugin_dir.join("Cargo.toml").to_str().expect("utf8 path"),
                "--target",
                "wasm32-wasip2",
                "--release",
            ])
            .status()
            .expect("invoke cargo build for com.aios.calculator");
        assert!(status.success(), "cargo build wasm32-wasip2 failed");
        let built = plugin_dir.join("target/wasm32-wasip2/release/aios_plugin_calculator.wasm");
        let tmp_wasm = plugin_dir.join(".plugin.wasm.tmp");
        fs::copy(&built, &tmp_wasm).expect("copy compiled calculator wasm");
        fs::rename(&tmp_wasm, &wasm_out).expect("install compiled calculator wasm");
    });
    plugin_dir
}

#[test]
fn bundled_external_wasm_calculator_shadows_builtin_runs_inline_and_toggles_cleanly() {
    let wasm_bundle_src = ensure_calculator_wasm_bundle();
    assert!(
        wasm_bundle_src.join("plugin.wasm").is_file(),
        "expected compiled wasm32-wasip2 bundle at {}",
        wasm_bundle_src.display()
    );

    let tmp = TempDir::new().expect("temp dir");
    let bundled_root = tmp.path().join("bundled");
    let bundled_calc_dir = bundled_root.join("com.aios.calculator");
    aios_plugin_host::install_plugin_from_dir(&wasm_bundle_src, &bundled_root)
        .expect("stage bundled calculator");
    assert!(bundled_calc_dir.join("plugin.wasm").is_file());

    let (_paths, _db, mut host) = test_host(&tmp);
    host = host.with_bundled_dirs(vec![bundled_root.clone()]);

    // 1. Two-layer discovery finds external WASM Calculator as `Bundled`.
    let calc = host
        .find_plugin("com.aios.calculator")
        .expect("bundled external calculator loaded");
    assert_eq!(calc.origin, aios_plugin_host::PluginOrigin::Bundled);
    assert!(calc.manifest.inline_trigger.is_configured());
    assert!(!calc.manifest.inline_trigger.matches_fast_gate("firefox"));
    assert!(calc.manifest.inline_trigger.matches_fast_gate("10*30"));
    assert!(calc.manifest.inline_trigger.matches_fast_gate("=pi"));

    // Probe init_command first so any Wasmtime instantiation error prints clearly in test output.
    let probe = host.run_wasm_command("com.aios.calculator", "calculator", "[]", "dark");
    if let PluginResponse::Push { view, .. } = &probe.response
        && let ViewNode::Error(err) = view.as_ref()
    {
        panic!(
            "WASM calculator failed to execute: {} | {:?}",
            err.title, err.detail
        );
    }

    // 2. Live inline evaluation executes inside Wasmtime sandbox & resolves icon.svg relative to bundle.
    assert!(host.evaluate_inline_query("firefox", "dark").is_empty());
    let inline = host.evaluate_inline_query("1500 ml to l", "dark");
    assert_eq!(inline.len(), 1);
    assert_eq!(inline[0].copy_value, "1.5");
    assert_eq!(inline[0].title, "1.5 L");
    assert!(
        inline[0]
            .icon_path
            .as_deref()
            .is_some_and(|p| p.ends_with("com.aios.calculator/icon.svg")),
        "expected resolved bundle icon.svg, got {:?}",
        inline[0].icon_path
    );

    // 3. Inline copy action executes inside WASM, writes SQLite history, and closes window.
    let copy_res = host
        .dispatch_host_action(&inline[0].copy_action_id, &[], None, "dark")
        .expect("inline action dispatched");
    assert!(matches!(
        copy_res.response,
        PluginResponse::CopyToClipboard { ref text, .. } if text == "1.5"
    ));
    assert!(
        copy_res
            .side_effects
            .iter()
            .any(|e| matches!(e, aios_plugin_host::HostSideEffect::CloseMainWindow))
    );

    // 4. Launching Calculator command resolves category SVG relative paths (`icons/calc-unit.svg`).
    let open = host.run_wasm_command(
        "com.aios.calculator",
        "calculator",
        r#"["24 px to rem"]"#,
        "dark",
    );
    let PluginResponse::Push { view, .. } = open.response else {
        panic!("expected Push ListView from WASM calculator");
    };
    let ViewNode::List(list) = *view else {
        panic!("expected ListView");
    };
    assert_eq!(list.sections[0].items[0].title, "1.5 rem");
    assert!(
        list.sections[0].items[0]
            .icon_path
            .as_deref()
            .is_some_and(|p| p.ends_with("icons/calc-unit.svg")),
        "expected resolved category icon, got {:?}",
        list.sections[0].items[0].icon_path
    );

    // 5. Disabling the bundled plugin hides it from Root Search & inline queries without deleting files.
    host.set_plugin_enabled("com.aios.calculator", false)
        .expect("disable calculator");
    assert!(host.evaluate_inline_query("10*30", "dark").is_empty());
    assert!(
        !host
            .root_search_entries()
            .iter()
            .any(|e| e.id.starts_with("plugin:com.aios.calculator:"))
    );
    host.set_plugin_enabled("com.aios.calculator", true)
        .expect("re-enable calculator");
    assert_eq!(host.evaluate_inline_query("10*30", "dark").len(), 1);
}

#[test]
fn plugin_archive_pack_sha256_registry_and_store_install_work_end_to_end() {
    let wasm_bundle_src = ensure_calculator_wasm_bundle();
    let tmp = TempDir::new().expect("temp dir");

    // 1. Pack `.aios-plugin` tarball and verify SHA-256 round-trip.
    let archive_path = tmp.path().join("com.aios.calculator-v0.1.0.aios-plugin");
    let (packed_meta, sha256) = aios_plugin_host::pack_plugin_dir(&wasm_bundle_src, &archive_path)
        .expect("pack .aios-plugin archive");
    assert_eq!(packed_meta.manifest.id, "com.aios.calculator");
    assert_eq!(sha256.len(), 64);

    // Tampered SHA-256 MUST be rejected before extraction.
    let archive_bytes = fs::read(&archive_path).expect("read archive");
    let bad_sha = "0".repeat(64);
    assert!(
        aios_plugin_host::install_plugin_archive_bytes(
            &archive_bytes,
            Some(&bad_sha),
            &tmp.path().join("rejected"),
        )
        .is_err()
    );

    // 2. Build a local file:// registry index.json from `registry/plugins/*.toml`.
    let reg_dir = tmp.path().join("registry");
    fs::create_dir_all(reg_dir.join("plugins")).expect("mkdir registry/plugins");
    let entry_toml = format!(
        r#"
id = "com.aios.calculator"
name = "Calculator & Converter"
author = "AIOS"
description = "External WASM calculator"
repository = "https://github.com/artsbymat/aios"
category = "productivity"
tags = ["math", "units"]
capabilities = ["clipboard_write"]

[latest]
version = "0.1.0"
min_aios_version = "0.1.0"
download_url = "file://{}"
sha256 = "{sha256}"
"#,
        archive_path.display()
    );
    fs::write(reg_dir.join("plugins/com.aios.calculator.toml"), entry_toml)
        .expect("write registry entry");

    let index =
        aios_plugin_host::build_registry_index_from_dir(&reg_dir).expect("build registry index");
    assert_eq!(index.plugins.len(), 1);
    let index_path = reg_dir.join("index.json");
    fs::write(&index_path, serde_json::to_string_pretty(&index).unwrap())
        .expect("write index.json");

    // 3. Configure PluginHost with the local file:// registry URL and install via Store/Registry.
    let paths = AiosPaths::from_roots(tmp.path(), None, None, None, None);
    paths.ensure_dirs().expect("ensure dirs");
    let db = Arc::new(Mutex::new(
        Database::open(&paths.database_file()).expect("open db"),
    ));
    let mut cfg = AiosConfig::default();
    cfg.plugins.registry_urls = vec![format!("file://{}", index_path.display())];
    let mut host = PluginHost::new(cfg, paths, Arc::clone(&db), WasmExecutionLimits::default())
        .expect("init host");

    let fetched =
        aios_plugin_host::fetch_registry_index(&host.config().plugins.registry_urls, &db, 6, true)
            .expect("fetch file:// index");
    assert_eq!(fetched.plugins.len(), 1);

    let store_before = host.build_store_view();
    let ViewNode::List(store_list) = store_before else {
        panic!("expected ListView for Plugin Store");
    };
    assert_eq!(
        store_list.sections[0].items[0].id,
        "store:com.aios.calculator"
    );

    // Install from registry with SHA-256 verification and verify user-installed layer shadows bundled.
    let installed = host
        .install_registry_plugin("com.aios.calculator")
        .expect("install from registry");
    assert_eq!(
        installed.origin,
        aios_plugin_host::PluginOrigin::UserInstalled
    );
    assert!(installed.root_dir.join("icons/calc-unit.svg").is_file());
    assert_eq!(
        host.evaluate_inline_query("24 * 12", "dark")[0].copy_value,
        "288"
    );
}
