use aios_core::view::ToastStyle;
use aios_ui::components::char_indices_to_byte_ranges;
use aios_ui::demo::{build_root_showcase_frame, dispatch_demo_action, open_deep_link};
use aios_ui::{ActiveOverlay, HeaderFocus, LauncherState, ShellEffect};

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
}
