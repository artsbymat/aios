use aios_core::view::ToastStyle;
use aios_core::{AiosConfig, AiosPaths, Database, view::ViewNode};
use aios_plugin_host::{PluginHost, WasmExecutionLimits};
use aios_plugin_sdk::{PluginEvent, PluginResponse};
use aios_ui::components::char_indices_to_byte_ranges;
use aios_ui::demo::{build_root_showcase_frame, dispatch_demo_action, open_deep_link};
use aios_ui::{ActiveOverlay, HeaderFocus, LauncherState, NavFrame, ShellEffect};

#[test]
fn escape_cascade_closes_overlays_clears_query_pops_stack_then_hides() {
    let mut state = LauncherState::new(build_root_showcase_frame());

    // Push into Grid view
    let _ = dispatch_demo_action(&mut state, "open.grid");
    assert_eq!(state.depth(), 2);

    // Type a search query
    state.insert_text("copper");
    assert_eq!(state.current().search_query, "copper");

    // Open ActionPanel
    state.toggle_action_panel();
    assert!(matches!(
        state.overlay,
        Some(ActiveOverlay::ActionPanel { .. })
    ));

    // 1. Esc closes ActionPanel
    assert_eq!(state.handle_escape(), ShellEffect::None);
    assert!(state.overlay.is_none());

    // 2. Esc clears search query
    assert_eq!(state.handle_escape(), ShellEffect::None);
    assert!(state.current().search_query.is_empty());

    // 3. Esc pops Grid view back to Root
    assert_eq!(state.handle_escape(), ShellEffect::None);
    assert_eq!(state.depth(), 1);

    // 4. Esc at empty Root requests window hide
    assert_eq!(state.handle_escape(), ShellEffect::HideWindow);
}

#[test]
fn host_fuzzy_filtering_highlights_and_alias_ranking() {
    let mut state = LauncherState::new(build_root_showcase_frame());
    state.insert_text("cb");

    let entries = state.current().filtered_list_entries();
    assert!(!entries.is_empty());
    // Exact alias `cb` must rank List + Split Detail View #1.
    let top = &entries[0];
    let aios_core::view::ViewNode::List(list) = &state.current().view else {
        panic!("expected list view");
    };
    let top_item = &list.sections[top.section_index].items[top.item_index];
    assert_eq!(top_item.id, "demo.list-detail");

    // Verify Unicode byte-range mapping for GPUI StyledText highlights
    let ranges = char_indices_to_byte_ranges("🚀 Café Mocha", &[2, 3, 4, 5]);
    assert_eq!(ranges.len(), 1);
    assert_eq!(&"🚀 Café Mocha"[ranges[0].clone()], "Café");
}

#[test]
fn inline_arguments_filter_dropdown_and_action_panel_submenu_keyboard_flow() {
    let mut state = LauncherState::new(build_root_showcase_frame());

    // Tab cycles Search -> InlineArg(0) -> FilterDropdown -> Search
    assert_eq!(state.current().header_focus, HeaderFocus::Search);
    state.handle_tab(false);
    assert_eq!(state.current().header_focus, HeaderFocus::InlineArg(0));
    state.insert_text("wayland");
    assert_eq!(state.current().inline_arg_values[0], "wayland");

    state.handle_tab(false);
    assert_eq!(state.current().header_focus, HeaderFocus::FilterDropdown);
    state.toggle_filter_dropdown();
    assert!(matches!(
        state.overlay,
        Some(ActiveOverlay::FilterDropdown { .. })
    ));

    // Apply "View Renderers" filter (index 1) -> narrows list to the 4 views
    state.apply_filter_option(1);
    assert!(state.overlay.is_none());
    assert_eq!(state.current().filtered_list_entries().len(), 4);

    // Open ActionPanel and drill into Theme submenu
    state.toggle_action_panel();
    let actions = state.visible_action_panel_items();
    assert!(actions.iter().any(|a| !a.submenu.is_empty()));
    let submenu_idx = actions
        .iter()
        .position(|a| !a.submenu.is_empty())
        .expect("submenu action exists");
    for _ in 0..submenu_idx {
        state.move_vertical(1);
    }
    state.move_horizontal(1);
    assert!(matches!(
        state.overlay,
        Some(ActiveOverlay::ActionPanel {
            submenu_parent: Some(_),
            ..
        })
    ));
    let sub_items = state.visible_action_panel_items();
    assert_eq!(sub_items.len(), 3);
}

#[test]
fn form_validation_tab_order_and_submit_flow() {
    let mut state = LauncherState::new(build_root_showcase_frame());
    open_deep_link(&mut state, "aios://demo/form");
    assert_eq!(state.depth(), 2);

    // Submitting empty form produces inline errors and Failure toast
    let effect = dispatch_demo_action(&mut state, "form.submit");
    assert_eq!(effect, ShellEffect::None);
    assert_eq!(
        state.toast.as_ref().map(|t| t.style),
        Some(ToastStyle::Failure)
    );

    // Fill required Name field (index 0), Tab to URL field (index 1), fill it, then submit
    state.insert_text("Docs.rs Rust");
    state.handle_tab(false);
    state.insert_text("https://docs.rs/{query}");

    let effect2 = dispatch_demo_action(&mut state, "form.submit");
    assert!(matches!(effect2, ShellEffect::ShowHud(_)));
    assert_eq!(
        state.toast.as_ref().map(|t| t.style),
        Some(ToastStyle::Success)
    );
}

#[test]
fn dmenu_state_filters_preserves_original_index_supports_custom_and_escape() {
    use aios_daemon::{DmenuCaseMode, DmenuRequest};
    use aios_ui::dmenu::compute_dmenu_height;
    use aios_ui::{DmenuOutcome, DmenuState};

    let req = DmenuRequest {
        prompt: Some("Screenshot".to_string()),
        items: vec![
            "Full".to_string(),
            "Area".to_string(),
            "Window".to_string(),
            "Notes".to_string(),
            "FullDelay".to_string(),
            "DecodeQR".to_string(),
            "ImageToText".to_string(),
        ],
        lines: Some(7),
        case_mode: DmenuCaseMode::Insensitive,
        allow_custom: true,
        return_index: false,
        theme: None,
        icon: Some("SC".to_string()),
        show_icons: true,
        item_icons: vec![Some("FS".to_string()), None],
        streaming: false,
    };

    let mut state = DmenuState::from_request(&req);
    assert_eq!(state.header_icon.as_deref(), Some("SC"));
    assert!(state.show_icons);
    assert_eq!(state.item_icons.len(), 2);
    assert_eq!(state.filtered_entries().len(), 7);

    // Compact height fits 7 lines without exceeding max_height.
    let h = compute_dmenu_height(&AiosConfig::default(), &req);
    assert!((360.0..=500.0).contains(&h), "unexpected dmenu height {h}");

    // Fuzzy query "qr" narrows to DecodeQR while preserving its original 0-based index (5).
    state.insert_text("qr");
    let matches = state.filtered_entries();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].original_index, 5);
    assert_eq!(
        state.confirm(false),
        Some(DmenuOutcome::Selected {
            value: "DecodeQR".to_string(),
            index: Some(5),
        })
    );

    // Ctrl+Enter (`prefer_custom = true`) submits raw typed text when `allow_custom` is enabled.
    assert_eq!(
        state.confirm(true),
        Some(DmenuOutcome::Selected {
            value: "qr".to_string(),
            index: None,
        })
    );

    // First Esc clears query; second Esc signals cancellation.
    assert!(!state.handle_escape());
    assert!(state.query.is_empty());
    assert_eq!(state.filtered_entries().len(), 7);
    assert!(state.handle_escape());

    // Verify all 3 case modes: Sensitive, Insensitive, and Smart.
    let case_items = vec![
        "alpha".to_string(),
        "Alpha".to_string(),
        "ALPHA".to_string(),
    ];

    // 1. Sensitive (`-s`): "alpha" matches ONLY lowercase "alpha"; "ALPHA" matches ONLY "ALPHA".
    let mut sensitive = DmenuState::from_request(&DmenuRequest {
        items: case_items.clone(),
        case_mode: DmenuCaseMode::Sensitive,
        ..Default::default()
    });
    sensitive.insert_text("alpha");
    assert_eq!(sensitive.filtered_entries().len(), 1);
    assert_eq!(sensitive.filtered_entries()[0].original_index, 0);
    sensitive.handle_escape();
    sensitive.insert_text("ALPHA");
    assert_eq!(sensitive.filtered_entries().len(), 1);
    assert_eq!(sensitive.filtered_entries()[0].original_index, 2);

    // 2. Insensitive (`-i`): "ALPHA" matches all 3 entries.
    let mut insensitive = DmenuState::from_request(&DmenuRequest {
        items: case_items.clone(),
        case_mode: DmenuCaseMode::Insensitive,
        ..Default::default()
    });
    insensitive.insert_text("ALPHA");
    assert_eq!(insensitive.filtered_entries().len(), 3);

    // 3. Smart (default): "alpha" matches all 3; "Alpha" matches only "Alpha".
    let mut smart = DmenuState::from_request(&DmenuRequest {
        items: case_items,
        case_mode: DmenuCaseMode::Smart,
        ..Default::default()
    });
    smart.insert_text("alpha");
    assert_eq!(smart.filtered_entries().len(), 3);
    smart.handle_escape();
    smart.insert_text("Alpha");
    assert_eq!(smart.filtered_entries().len(), 1);
    assert_eq!(smart.filtered_entries()[0].original_index, 1);

    // 4. Streaming mode: user types BEFORE items arrive, then batches stream in incrementally.
    let mut streaming = DmenuState::from_request(&DmenuRequest {
        prompt: Some("Find".to_string()),
        streaming: true,
        ..Default::default()
    });
    assert!(streaming.is_loading);
    assert!(streaming.filtered_entries().is_empty());

    // User types "cam" while producer is still running.
    streaming.insert_text("cam");
    assert_eq!(streaming.query, "cam");

    // Batch 1 arrives without matches -> still 0 matches, query preserved.
    streaming.append_batch(
        vec!["edit-cut".to_string(), "folder".to_string()],
        Vec::new(),
    );
    assert_eq!(streaming.items.len(), 2);
    assert!(streaming.filtered_entries().is_empty());

    // Batch 2 arrives containing "camera-photo" (original index 3) -> immediately matched!
    streaming.append_batch(
        vec!["clock".to_string(), "camera-photo".to_string()],
        vec![None, Some("camera-photo".to_string())],
    );
    assert!(streaming.show_icons);
    assert_eq!(streaming.filtered_entries().len(), 1);
    assert_eq!(streaming.filtered_entries()[0].original_index, 3);
    assert_eq!(
        streaming.confirm(false),
        Some(DmenuOutcome::Selected {
            value: "camera-photo".to_string(),
            index: Some(3),
        })
    );
    streaming.finish_loading();
    assert!(!streaming.is_loading);
    assert_eq!(streaming.visible_len(), 1);
    assert_eq!(streaming.entry_at(0).map(|(idx, _)| idx), Some(3));

    // Clearing immediately releases packed items and resets selection/counts.
    streaming.clear();
    assert!(streaming.items.is_empty());
    assert_eq!(streaming.visible_len(), 0);
    assert_eq!(streaming.total_matched, 0);
}

#[test]
fn search_deep_link_prefills_query_decodes_url_and_applies_filter() {
    let mut state = LauncherState::new(build_root_showcase_frame());

    // Push a nested view first so we verify `aios://search?q=...` always pops back to Root.
    open_deep_link(&mut state, "aios://demo/grid");
    assert_eq!(state.depth(), 2);

    open_deep_link(&mut state, "aios://search?q=screenshot");
    assert_eq!(state.depth(), 1);
    assert_eq!(state.current().search_query, "screenshot");
    assert_eq!(state.current().header_focus, HeaderFocus::Search);
    assert!(state.toast.is_none());

    // Percent-encoded spaces, `+`, `query=` alias, and `&filter=...` all work cleanly.
    open_deep_link(
        &mut state,
        "aios://search?query=grid%20view+test&filter=views",
    );
    assert_eq!(state.depth(), 1);
    assert_eq!(state.current().search_query, "grid view test");
    let ViewNode::List(list) = &state.current().view else {
        panic!("expected root list view");
    };
    assert_eq!(
        list.filter_dropdown
            .as_ref()
            .map(|d| d.selected_id.as_str()),
        Some("views")
    );
}

#[test]
fn grid_arrow_navigation_moves_across_columns_and_rows() {
    let mut state = LauncherState::new(build_root_showcase_frame());
    open_deep_link(&mut state, "aios://demo/grid");
    assert_eq!(state.current().selected_index, 0);

    // 4-column grid: Right -> 1, Down -> 5, Left -> 4, Up -> 0
    state.move_horizontal(1);
    assert_eq!(state.current().selected_index, 1);
    state.move_vertical(1);
    assert_eq!(state.current().selected_index, 5);
    state.move_horizontal(-1);
    assert_eq!(state.current().selected_index, 4);
    state.move_vertical(-1);
    assert_eq!(state.current().selected_index, 0);

    // Ctrl+N / Ctrl+P (`move_sequential`) steps right across row 0 (0 -> 1 -> 2 -> 3)
    // and then wraps onto row 1 col 0 (4), and Ctrl+P steps back (4 -> 3).
    for expected in [1, 2, 3, 4] {
        state.move_sequential(1);
        assert_eq!(state.current().selected_index, expected);
    }
    state.move_sequential(-1);
    assert_eq!(state.current().selected_index, 3);
}

#[test]
fn plugin_enter_exit_and_form_keybinds_remain_intact() {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    let tmp = tempfile::TempDir::new().expect("temp dir");
    let paths = AiosPaths::from_roots(
        tmp.path(),
        Some(&tmp.path().join("config")),
        Some(&tmp.path().join("data")),
        Some(&tmp.path().join("cache")),
        Some(&tmp.path().join("run")),
    );
    paths.ensure_dirs().expect("ensure dirs");
    let db = Arc::new(Mutex::new(
        Database::open(&paths.database_file()).expect("open db"),
    ));
    let mut host = PluginHost::new(
        AiosConfig::default(),
        paths,
        db,
        WasmExecutionLimits::default(),
    )
    .expect("plugin host");

    let calc_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .join("plugins/com.aios.calculator");
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

    // Start on Root Search frame (depth 1, no plugin_session).
    let mut state = LauncherState::new(NavFrame::new(
        "root.search",
        build_root_showcase_frame().view,
    ));
    assert_eq!(state.depth(), 1);
    assert!(state.current().plugin_session.is_none());

    // Enter & exit external WASM calculator 3 times and verify Root Search keybinds stay intact.
    for cycle in 0..3 {
        let open_res = host.run_wasm_command("com.aios.calculator", "calculator", "[]", "dark");
        let (frame_id, view) = match open_res.response {
            PluginResponse::Push { frame_id, view } => (frame_id.unwrap_or_default(), *view),
            other => panic!("cycle {cycle}: expected Push(ListView), got {other:?}"),
        };
        let (pid, cid) = open_res
            .active_plugin_session
            .expect("plugin session must be attached");
        state.push(NavFrame::new(frame_id, view).with_plugin_session(pid.clone(), cid.clone()));

        assert_eq!(state.depth(), 2);
        assert!(matches!(state.current().view, ViewNode::List(_)));
        assert!(state.stack[0].plugin_session.is_none());

        state.insert_text("12*8");
        let search_res = host.send_wasm_event(
            &pid,
            &cid,
            &PluginEvent::SearchTextChanged {
                command_id: cid.clone(),
                query: "12*8".to_string(),
            },
            "dark",
        );
        assert!(matches!(search_res.response, PluginResponse::Render { .. }));

        // Esc clears query first, then pops back to Root Search.
        assert_eq!(state.handle_escape(), ShellEffect::None);
        assert!(state.current().search_query.is_empty());
        assert_eq!(state.handle_escape(), ShellEffect::None);
        assert_eq!(state.depth(), 1);
        assert_eq!(state.current().id, "root.search");
        assert!(state.current().plugin_session.is_none());

        // Root Search keybinds still work normally after exiting plugin!
        state.move_vertical(1);
        state.move_vertical(-1);
        state.toggle_filter_dropdown();
        assert!(matches!(
            state.overlay,
            Some(ActiveOverlay::FilterDropdown { .. })
        ));
        assert_eq!(state.handle_escape(), ShellEffect::None);
        assert!(state.overlay.is_none());
    }

    // Verify Form view Tab/Shift+Tab/Backspace/validation with an attached plugin session.
    let form_view = aios_plugin_sdk::FormViewBuilder::new("Create Entry")
        .field(aios_plugin_sdk::FormField::TextField {
            id: "name".to_string(),
            label: "Name".to_string(),
            placeholder: None,
            value: String::new(),
            required: true,
            error: None,
        })
        .field(aios_plugin_sdk::FormField::TextField {
            id: "value".to_string(),
            label: "Value".to_string(),
            placeholder: None,
            value: String::new(),
            required: true,
            error: None,
        })
        .build();
    state.push(
        NavFrame::new("plugin:com.test.form:create", form_view)
            .with_plugin_session("com.test.form", "create"),
    );
    state.insert_text("Cobalt");
    state.handle_backspace();
    state.insert_text("t");
    assert_eq!(state.depth(), 2);
    state.handle_tab(false);
    assert_eq!(state.current().focused_form_field, 1);
    state.insert_text("#2459A8");
    let form_values: BTreeMap<String, String> = state
        .validate_active_form()
        .expect("form should pass validation")
        .into_iter()
        .collect();
    assert_eq!(form_values.get("name").map(String::as_str), Some("Cobalt"));
    assert_eq!(
        form_values.get("value").map(String::as_str),
        Some("#2459A8")
    );
}

#[test]
fn backspace_on_empty_plugin_list_search_does_not_pop_to_root() {
    let mut state = LauncherState::new(build_root_showcase_frame());
    let calc_view = ViewNode::List(aios_core::view::ListView {
        navigation_title: Some("Calculator & Converter".to_string()),
        search_placeholder: Some("Type expression...".to_string()),
        filtering: aios_core::view::FilterMode::Plugin,
        is_loading: false,
        loading_message: None,
        is_showing_detail: true,
        filter_dropdown: None,
        inline_arguments: Vec::new(),
        sections: Vec::new(),
        empty_view: None,
    });
    state.push(
        NavFrame::new("plugin:com.aios.calculator:calculator", calc_view)
            .with_plugin_session("com.aios.calculator", "calculator"),
    );
    assert_eq!(state.depth(), 2);

    state.insert_text("5");
    assert_eq!(state.current().search_query, "5");
    state.handle_backspace();
    assert!(state.current().search_query.is_empty());
    assert_eq!(state.depth(), 2);

    // Extra Backspace presses on empty search input must stay inside the plugin view.
    state.handle_backspace();
    state.handle_backspace();
    assert_eq!(state.depth(), 2);
    assert_eq!(state.current().id, "plugin:com.aios.calculator:calculator");

    // Esc still pops cleanly back to Root.
    assert_eq!(state.handle_escape(), ShellEffect::None);
    assert_eq!(state.depth(), 1);
    assert_eq!(state.current().id, "root.showcase");
}
