//! Reproducible Pure Rust builder that compiles the reference WASM Component Model
//! plugin (`palette-grid-wasm`) into `examples/plugins/palette-grid-wasm/plugin.wasm`.
//!
//! Run via:
//! `cargo run -p aios-plugin-host --example build_example_plugins`

use std::fs;
use std::path::{Path, PathBuf};

use aios_plugin_sdk::{
    ActionItem, ActionPanel, FilterOption, FormField, FormViewBuilder, GridAspectRatio,
    GridItemBuilder, GridItemContent, GridViewBuilder, PluginResponse,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn wat_escape(bytes: &[u8]) -> String {
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

fn build_palette_grid_wasm() -> Vec<u8> {
    let swatches = [
        ("sw-basalt", "Basalt Canvas", "#131517"),
        ("sw-elevated", "Basalt Elevated", "#1B1E22"),
        ("sw-alabaster", "Warm Alabaster", "#F6F4EF"),
        ("sw-copper", "Signal Copper", "#E87143"),
        ("sw-kiln", "Kiln Copper", "#B54116"),
        ("sw-success", "Emerald Status", "#4CC38A"),
        ("sw-warning", "Amber Status", "#E5B546"),
        ("sw-danger", "Crimson Status", "#F26D6D"),
    ];

    let grid_items = swatches
        .iter()
        .map(|(id, name, hex)| {
            GridItemBuilder::new(
                *id,
                *name,
                GridItemContent::ColorSwatch {
                    hex: (*hex).to_string(),
                },
            )
            .subtitle(*hex)
            .keyword("color")
            .keyword("swatch")
            .actions(ActionPanel::from_actions(vec![
                ActionItem::new("swatch.open-form", "Create Custom Swatch").shortcut("Enter"),
            ]))
            .build()
        })
        .collect();

    let grid_view = GridViewBuilder::new("Palette Swatches (WASM)", 4)
        .placeholder("Filter mineral swatches...")
        .aspect_ratio(GridAspectRatio::Square)
        .section(
            Some("AIOS Mineral & Copper Tokens"),
            Some("Saved locally"),
            grid_items,
        )
        .build();

    let form_view = FormViewBuilder::new("Create Custom Swatch (WASM)")
        .field(FormField::TextField {
            id: "name".to_string(),
            label: "Swatch Name".to_string(),
            placeholder: Some("e.g. Slate Mineral".to_string()),
            value: String::new(),
            required: true,
            error: None,
        })
        .field(FormField::TextField {
            id: "hex".to_string(),
            label: "Hex Color".to_string(),
            placeholder: Some("#363C44".to_string()),
            value: String::new(),
            required: true,
            error: None,
        })
        .field(FormField::Dropdown {
            id: "theme".to_string(),
            label: "Target Theme".to_string(),
            options: vec![
                FilterOption {
                    id: "dark".to_string(),
                    title: "Basalt Dark".to_string(),
                },
                FilterOption {
                    id: "light".to_string(),
                    title: "Alabaster Light".to_string(),
                },
            ],
            selected_id: "dark".to_string(),
            error: None,
        })
        .actions(ActionPanel::from_actions(vec![
            ActionItem::new("swatch.save", "Save Custom Swatch").shortcut("Enter"),
            ActionItem::new("nav.pop", "Cancel").shortcut("Esc"),
        ]))
        .build();

    let grid_full = PluginResponse::render(grid_view)
        .to_json()
        .expect("serialize grid");
    // Split `grid_full` right before the closing `],"empty_view":null}}}` of the items array
    // so persisted custom swatch JSON items can be appended dynamically from SQLite LocalStorage.
    let split_marker = "]}],\"empty_view\":null}}";
    let split_pos = grid_full
        .rfind(split_marker)
        .expect("grid_full must end with items closing marker");
    let grid_prefix = &grid_full[..split_pos];
    let grid_suffix = &grid_full[split_pos..];

    let form_render_json = PluginResponse::render(form_view.clone())
        .to_json()
        .expect("serialize form render");
    let form_push_json = PluginResponse::push(Some("palette.form".to_string()), form_view)
        .to_json()
        .expect("serialize form push");
    let noop_json = PluginResponse::Noop.to_json().expect("serialize noop");

    // JSON template fragments for one saved custom GridItem:
    // `,{"id":"sw-custom","title":"` + <name> + `","subtitle":"` + <hex> + `","content":{"type":"color-swatch","hex":"` + <hex> + `"},"keywords":["custom","color","swatch"],"filter_group":null,"actions":{"sections":[{"title":null,"actions":[{"id":"swatch.open-form","title":"Create Custom Swatch","shortcut":"Enter","destructive":false,"submenu":[]}]}]}}`
    let tpl_a = b",{\"id\":\"sw-custom\",\"title\":\"";
    let tpl_b = b"\",\"subtitle\":\"";
    let tpl_c = b"\",\"content\":{\"type\":\"color-swatch\",\"hex\":\"";
    let tpl_d = b"\"},\"keywords\":[\"custom\",\"color\",\"swatch\"],\"filter_group\":null,\"actions\":{\"sections\":[{\"title\":null,\"actions\":[{\"id\":\"swatch.open-form\",\"title\":\"Create Custom Swatch\",\"shortcut\":\"Enter\",\"destructive\":false,\"submenu\":[]}]}]}}";

    let key_custom = b"custom_swatches_v1";
    let pat_hex = b"\"hex\":\"";
    let pat_name = b"\"name\":\"";
    let toast_saved = b"Saved Custom Swatch";
    let hud_saved = b"Saved Custom Swatch";

    let grid_prefix_esc = wat_escape(grid_prefix.as_bytes());
    let grid_prefix_len = grid_prefix.len();
    let grid_suffix_esc = wat_escape(grid_suffix.as_bytes());
    let grid_suffix_len = grid_suffix.len();
    let form_render_esc = wat_escape(form_render_json.as_bytes());
    let form_render_len = form_render_json.len();
    let form_push_esc = wat_escape(form_push_json.as_bytes());
    let form_push_len = form_push_json.len();
    let noop_esc = wat_escape(noop_json.as_bytes());
    let noop_len = noop_json.len();

    let tpl_a_esc = wat_escape(tpl_a);
    let tpl_a_len = tpl_a.len();
    let tpl_b_esc = wat_escape(tpl_b);
    let tpl_b_len = tpl_b.len();
    let tpl_c_esc = wat_escape(tpl_c);
    let tpl_c_len = tpl_c.len();
    let tpl_d_esc = wat_escape(tpl_d);
    let tpl_d_len = tpl_d.len();

    let key_esc = wat_escape(key_custom);
    let key_len = key_custom.len();
    let pat_hex_esc = wat_escape(pat_hex);
    let pat_hex_len = pat_hex.len();
    let pat_name_esc = wat_escape(pat_name);
    let pat_name_len = pat_name.len();
    let toast_esc = wat_escape(toast_saved);
    let toast_len = toast_saved.len();
    let hud_esc = wat_escape(hud_saved);
    let hud_len = hud_saved.len();

    // Linear memory layout (4 pages = 256 KiB):
    // 1024..   : grid_prefix
    // 16384..  : grid_suffix
    // 20480..  : form_render_json
    // 36864..  : form_push_json
    // 53248..  : tpl_a
    // 53504..  : tpl_b
    // 53760..  : tpl_c
    // 54016..  : tpl_d
    // 55296..  : key_custom
    // 55552..  : pat_hex
    // 55600..  : pat_name
    // 55648..  : toast_saved
    // 55712..  : hud_saved
    // 60000..  : retptr & scratch structs
    // 65536..  : dynamic output buffer (for assembled grid / storage list)
    // 131072.. : dynamic storage append buffer
    // 196608.. : bump allocator for cabi_realloc
    let wat = format!(
        r#"
(component
  (import "aios:plugin/host-api@0.1.0" (instance $host
    (type $toast-style_def (enum "animated" "success" "failure"))
    (export "toast-style" (type $toast-style (eq $toast-style_def)))
    (type $toast_def (record (field "style" $toast-style) (field "title" string) (field "message" (option string))))
    (export "toast" (type $toast (eq $toast_def)))
    (export "show-toast" (func (param "t" $toast)))
    (export "show-hud" (func (param "title" string)))
    (export "storage-get" (func (param "key" string) (result (option (list u8)))))
    (export "storage-set" (func (param "key" string) (param "val" (list u8)) (result (result (error string)))))
  ))

  (core module $mem_mod
    (memory (export "memory") 4)
    (global $bump (mut i32) (i32.const 196608))
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (i32.add (local.get 3) (i32.const 16))))
      (local.get $ptr)
    )
  )
  (core instance $mem_inst (instantiate $mem_mod))
  (alias core export $mem_inst "memory" (core memory $mem))
  (alias core export $mem_inst "cabi_realloc" (core func $realloc))

  (alias export $host "show-toast" (func $f_show_toast))
  (alias export $host "show-hud" (func $f_show_hud))
  (alias export $host "storage-get" (func $f_storage_get))
  (alias export $host "storage-set" (func $f_storage_set))

  (core func $c_show_toast (canon lower (func $f_show_toast) (memory $mem)))
  (core func $c_show_hud (canon lower (func $f_show_hud) (memory $mem)))
  (core func $c_storage_get (canon lower (func $f_storage_get) (memory $mem) (realloc $realloc)))
  (core func $c_storage_set (canon lower (func $f_storage_set) (memory $mem) (realloc $realloc)))

  (core module $impl
    (import "env" "memory" (memory 4))
    (import "env" "show_toast" (func $host_show_toast (param i32 i32 i32 i32 i32 i32)))
    (import "env" "show_hud" (func $host_show_hud (param i32 i32)))
    (import "env" "storage_get" (func $host_storage_get (param i32 i32 i32)))
    (import "env" "storage_set" (func $host_storage_set (param i32 i32 i32 i32 i32)))

    (data (i32.const 1024) "{grid_prefix_esc}")
    (data (i32.const 16384) "{grid_suffix_esc}")
    (data (i32.const 20480) "{form_render_esc}")
    (data (i32.const 36864) "{form_push_esc}")
    (data (i32.const 53248) "{tpl_a_esc}")
    (data (i32.const 53504) "{tpl_b_esc}")
    (data (i32.const 53760) "{tpl_c_esc}")
    (data (i32.const 54016) "{tpl_d_esc}")
    (data (i32.const 55296) "{key_esc}")
    (data (i32.const 55552) "{pat_hex_esc}")
    (data (i32.const 55600) "{pat_name_esc}")
    (data (i32.const 55648) "{toast_esc}")
    (data (i32.const 55712) "{hud_esc}")
    (data (i32.const 55776) "{noop_esc}")

    ;; Copies `len` bytes from `src` to `dst`, returning `dst + len`.
    (func $memcpy (param $dst i32) (param $src i32) (param $len i32) (result i32)
      (local $i i32)
      (block $done
        (loop $lp
          (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
          (i32.store8
            (i32.add (local.get $dst) (local.get $i))
            (i32.load8_u (i32.add (local.get $src) (local.get $i)))
          )
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $lp)
        )
      )
      (i32.add (local.get $dst) (local.get $len))
    )

    ;; Finds first occurrence of pattern (`pat_ptr`, `pat_len`) inside (`hay_ptr`, `hay_len`).
    ;; Returns pointer immediately AFTER the pattern, or 0 if not found.
    (func $find_after (param $hay_ptr i32) (param $hay_len i32) (param $pat_ptr i32) (param $pat_len i32) (result i32)
      (local $i i32)
      (local $j i32)
      (local $matched i32)
      (if (i32.gt_u (local.get $pat_len) (local.get $hay_len))
        (then (return (i32.const 0)))
      )
      (block $outer_done
        (loop $outer
          (br_if $outer_done (i32.gt_u (i32.add (local.get $i) (local.get $pat_len)) (local.get $hay_len)))
          (local.set $j (i32.const 0))
          (local.set $matched (i32.const 1))
          (block $inner_done
            (loop $inner
              (br_if $inner_done (i32.ge_u (local.get $j) (local.get $pat_len)))
              (if (i32.ne
                    (i32.load8_u (i32.add (local.get $hay_ptr) (i32.add (local.get $i) (local.get $j))))
                    (i32.load8_u (i32.add (local.get $pat_ptr) (local.get $j))))
                (then
                  (local.set $matched (i32.const 0))
                  (br $inner_done)
                )
              )
              (local.set $j (i32.add (local.get $j) (i32.const 1)))
              (br $inner)
            )
          )
          (if (local.get $matched)
            (then (return (i32.add (local.get $hay_ptr) (i32.add (local.get $i) (local.get $pat_len)))))
          )
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $outer)
        )
      )
      (i32.const 0)
    )

    ;; Measures length of JSON string value starting at `start` up to unescaped `"` (max `max_len`).
    (func $json_str_len (param $start i32) (param $end_limit i32) (result i32)
      (local $cur i32)
      (local $ch i32)
      (local.set $cur (local.get $start))
      (block $done
        (loop $lp
          (br_if $done (i32.ge_u (local.get $cur) (local.get $end_limit)))
          (local.set $ch (i32.load8_u (local.get $cur)))
          (br_if $done (i32.eq (local.get $ch) (i32.const 34))) ;; '"'
          (if (i32.eq (local.get $ch) (i32.const 92)) ;; '\\'
            (then (local.set $cur (i32.add (local.get $cur) (i32.const 2))))
            (else (local.set $cur (i32.add (local.get $cur) (i32.const 1))))
          )
          (br $lp)
        )
      )
      (i32.sub (local.get $cur) (local.get $start))
    )

    ;; Assembles complete Grid JSON into buffer at 65536 by reading saved custom swatches from storage.
    ;; Writes result(ok(ptr, len)) into retptr at 60000 and returns 60000.
    (func $build_grid_response (result i32)
      (local $cursor i32)
      (local $saved_ptr i32)
      (local $saved_len i32)

      (local.set $cursor (call $memcpy (i32.const 65536) (i32.const 1024) (i32.const {grid_prefix_len})))
      ;; storage-get("custom_swatches_v1") -> option<list<u8>> at 60200: [disc:u8, pad:3, ptr:u32, len:u32]
      (call $host_storage_get (i32.const 55296) (i32.const {key_len}) (i32.const 60200))
      (if (i32.eq (i32.load8_u (i32.const 60200)) (i32.const 1))
        (then
          (local.set $saved_ptr (i32.load (i32.const 60204)))
          (local.set $saved_len (i32.load (i32.const 60208)))
          (local.set $cursor (call $memcpy (local.get $cursor) (local.get $saved_ptr) (local.get $saved_len)))
        )
      )
      (local.set $cursor (call $memcpy (local.get $cursor) (i32.const 16384) (i32.const {grid_suffix_len})))

      (i32.store8 (i32.const 60000) (i32.const 0))
      (i32.store (i32.const 60004) (i32.const 65536))
      (i32.store (i32.const 60008) (i32.sub (local.get $cursor) (i32.const 65536)))
      (i32.const 60000)
    )

    ;; Switch on first byte of command-id: 'n' (110) => "new-swatch" Form, otherwise "swatches" Grid
    (func (export "init-command") (param $cmd_ptr i32) (param $cmd_len i32) (param i32) (param i32) (result i32)
      (if (i32.and (i32.gt_u (local.get $cmd_len) (i32.const 0))
                   (i32.eq (i32.load8_u (local.get $cmd_ptr)) (i32.const 110)))
        (then
          (i32.store8 (i32.const 60000) (i32.const 0))
          (i32.store (i32.const 60004) (i32.const 20480))
          (i32.store (i32.const 60008) (i32.const {form_render_len}))
          (return (i32.const 60000))
        )
      )
      (call $build_grid_response)
    )

    ;; On `form-submitted` (byte 9 == 'f' (102)): extract name & hex, append to LocalStorage,
    ;; emit Success Toast + HUD, and render the updated Grid containing the saved swatch!
    (func (export "handle-event") (param $ev_ptr i32) (param $ev_len i32) (result i32)
      (local $ev_end i32)
      (local $name_ptr i32)
      (local $name_len i32)
      (local $hex_ptr i32)
      (local $hex_len i32)
      (local $cursor i32)
      (local $saved_ptr i32)
      (local $saved_len i32)

      (if (i32.and (i32.gt_u (local.get $ev_len) (i32.const 9))
                   (i32.eq (i32.load8_u (i32.add (local.get $ev_ptr) (i32.const 9))) (i32.const 102)))
        (then
          (local.set $ev_end (i32.add (local.get $ev_ptr) (local.get $ev_len)))
          (local.set $name_ptr (call $find_after (local.get $ev_ptr) (local.get $ev_len) (i32.const 55600) (i32.const {pat_name_len})))
          (local.set $hex_ptr (call $find_after (local.get $ev_ptr) (local.get $ev_len) (i32.const 55552) (i32.const {pat_hex_len})))
          (if (i32.and (i32.ne (local.get $name_ptr) (i32.const 0))
                       (i32.ne (local.get $hex_ptr) (i32.const 0)))
            (then
              (local.set $name_len (call $json_str_len (local.get $name_ptr) (local.get $ev_end)))
              (local.set $hex_len (call $json_str_len (local.get $hex_ptr) (local.get $ev_end)))

              ;; Load existing saved custom swatches into buffer at 131072
              (local.set $cursor (i32.const 131072))
              (call $host_storage_get (i32.const 55296) (i32.const {key_len}) (i32.const 60200))
              (if (i32.eq (i32.load8_u (i32.const 60200)) (i32.const 1))
                (then
                  (local.set $saved_ptr (i32.load (i32.const 60204)))
                  (local.set $saved_len (i32.load (i32.const 60208)))
                  (local.set $cursor (call $memcpy (local.get $cursor) (local.get $saved_ptr) (local.get $saved_len)))
                )
              )

              ;; Append the new GridItem JSON fragment
              (local.set $cursor (call $memcpy (local.get $cursor) (i32.const 53248) (i32.const {tpl_a_len})))
              (local.set $cursor (call $memcpy (local.get $cursor) (local.get $name_ptr) (local.get $name_len)))
              (local.set $cursor (call $memcpy (local.get $cursor) (i32.const 53504) (i32.const {tpl_b_len})))
              (local.set $cursor (call $memcpy (local.get $cursor) (local.get $hex_ptr) (local.get $hex_len)))
              (local.set $cursor (call $memcpy (local.get $cursor) (i32.const 53760) (i32.const {tpl_c_len})))
              (local.set $cursor (call $memcpy (local.get $cursor) (local.get $hex_ptr) (local.get $hex_len)))
              (local.set $cursor (call $memcpy (local.get $cursor) (i32.const 54016) (i32.const {tpl_d_len})))

              ;; Persist back to SQLite LocalStorage
              (call $host_storage_set
                (i32.const 55296)
                (i32.const {key_len})
                (i32.const 131072)
                (i32.sub (local.get $cursor) (i32.const 131072))
                (i32.const 60300)
              )
            )
          )

          (call $host_show_toast (i32.const 1) (i32.const 55648) (i32.const {toast_len}) (i32.const 0) (i32.const 0) (i32.const 0))
          (call $host_show_hud (i32.const 55712) (i32.const {hud_len}))
          (return (call $build_grid_response))
        )
      )

      ;; On `action-triggered` (byte 9 == 'a' (97)): push Create Custom Swatch Form
      (if (i32.and (i32.gt_u (local.get $ev_len) (i32.const 9))
                   (i32.eq (i32.load8_u (i32.add (local.get $ev_ptr) (i32.const 9))) (i32.const 97)))
        (then
          (i32.store8 (i32.const 60000) (i32.const 0))
          (i32.store (i32.const 60004) (i32.const 36864))
          (i32.store (i32.const 60008) (i32.const {form_push_len}))
          (return (i32.const 60000))
        )
      )

      ;; Ignore unrelated events cleanly with Noop
      (i32.store8 (i32.const 60000) (i32.const 0))
      (i32.store (i32.const 60004) (i32.const 55776))
      (i32.store (i32.const 60008) (i32.const {noop_len}))
      (i32.const 60000)
    )

    (func (export "background-refresh") (param i32 i32) (result i32)
      (call $build_grid_response)
    )

    (func (export "dispose"))
  )

  (core instance $impl_inst (instantiate $impl
    (with "env" (instance
      (export "memory" (memory $mem))
      (export "show_toast" (func $c_show_toast))
      (export "show_hud" (func $c_show_hud))
      (export "storage_get" (func $c_storage_get))
      (export "storage_set" (func $c_storage_set))
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
  (func $dispose_lifted (canon lift (core func $impl_inst "dispose")))

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

    wat::parse_str(&wat).expect("valid palette-grid WAT component")
}

fn main() {
    let root = repo_root();

    let palette_out = root.join("examples/plugins/palette-grid-wasm/plugin.wasm");
    let pg_bytes = build_palette_grid_wasm();
    fs::write(&palette_out, &pg_bytes).expect("write palette-grid plugin.wasm");
    println!("Built {} ({} bytes)", palette_out.display(), pg_bytes.len());
}
