//! Main GPUI `WindowKind::LayerShell` launcher window, persistent single-instance daemon
//! event loop, live XDG portal theme updates, and standalone preview runner.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use aios_builtin::{RootActionOutcome, RootSearchOrchestrator};
use aios_core::config::{AiosConfig, ThemeMode};
use aios_core::paths::AiosPaths;
use aios_core::storage::Database;
use aios_core::view::{FilterDropdown, ToastStyle, ViewNode};
use aios_daemon::{DaemonIpcCommand, DaemonIpcResponse, DaemonStatus};
use aios_wayland::{IpcServer, PortalThemeWatcher, detect_compositor};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, LinuxWindowBackground, QuitMode, Render, ScrollHandle, Size, Window,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point,
    prelude::*,
    px,
};

use crate::components::{
    aios_brand_mark, back_arrow_icon, enter_keycap, keycap, popover_shadow,
    render_confirm_alert_card, render_empty_state, render_error_state, render_footer_toast,
    window_shadow,
};
use crate::demo::{build_root_showcase_frame, dispatch_demo_action, open_deep_link};
use crate::hud::show_hud_pill;
use crate::state::{ActiveOverlay, HeaderFocus, LauncherState, ShellEffect};
use crate::theme::AiosTheme;
use crate::views::{
    detail::render_detail_view, form::render_form_view, grid::render_grid_view,
    list::render_list_view,
};

/// Main GPUI entity owning the active launcher `NavStack`, Root Search orchestrator,
/// keyboard focus, scroll handles, and overlays.
pub struct LauncherWindow {
    pub state: LauncherState,
    pub root_search: Rc<RefCell<RootSearchOrchestrator>>,
    pub focus_handle: FocusHandle,
    pub list_scroll: ScrollHandle,
    pub grid_scroll: ScrollHandle,
    pub form_scroll: ScrollHandle,
    pub action_panel_scroll: ScrollHandle,
    sync_scroll: Cell<bool>,
}

impl std::fmt::Debug for LauncherWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LauncherWindow")
            .field("depth", &self.state.depth())
            .finish_non_exhaustive()
    }
}

impl Focusable for LauncherWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl LauncherWindow {
    pub fn new(
        root_search: Rc<RefCell<RootSearchOrchestrator>>,
        initial_uri: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let initial_root_view = root_search.borrow_mut().build_root_view("", "all");
        let mut state = LauncherState::new(crate::state::NavFrame::new(
            "root.search",
            initial_root_view,
        ));
        if let Some(uri) = initial_uri {
            open_deep_link(&mut state, uri);
        }

        Self {
            state,
            root_search,
            focus_handle,
            list_scroll: ScrollHandle::new(),
            grid_scroll: ScrollHandle::new(),
            form_scroll: ScrollHandle::new(),
            action_panel_scroll: ScrollHandle::new(),
            sync_scroll: Cell::new(true),
        }
    }

    /// Re-runs Root Search orchestrator when the active frame is `root.search`.
    pub fn refresh_root_search(&mut self) {
        if self.state.current().id != "root.search" {
            return;
        }
        let query = self.state.current().search_query.clone();
        let active_filter = match &self.state.current().view {
            ViewNode::List(list) => list
                .filter_dropdown
                .as_ref()
                .map(|d| d.selected_id.clone())
                .unwrap_or_else(|| "all".to_string()),
            _ => "all".to_string(),
        };
        let next_view = self
            .root_search
            .borrow_mut()
            .build_root_view(&query, &active_filter);
        let frame = self.state.current_mut();
        frame.view = next_view;
        frame.clamp_selection();
    }

    pub fn request_scroll_sync(&self) {
        self.sync_scroll.set(true);
    }

    #[must_use]
    pub fn take_scroll_sync(&self) -> bool {
        self.sync_scroll.replace(false)
    }

    pub fn open_uri(&mut self, uri: &str, cx: &mut Context<Self>) {
        open_deep_link(&mut self.state, uri);
        self.refresh_root_search();
        self.request_scroll_sync();
        cx.notify();
    }

    pub fn trigger_primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(action) = self
            .state
            .current()
            .active_action_panel()
            .primary_action()
            .cloned()
        {
            self.execute_action(&action.id, window, cx);
        }
    }

    pub fn trigger_secondary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(action) = self
            .state
            .current()
            .active_action_panel()
            .secondary_action()
            .cloned()
        {
            self.execute_action(&action.id, window, cx);
        }
    }

    pub fn execute_action(&mut self, action_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.state.overlay = None;

        // Handle Root Search & Application Launcher actions first.
        let is_root_action = action_id.starts_with("app.")
            || action_id.starts_with("root.")
            || action_id.starts_with("core.")
            || action_id.starts_with("fallback.");

        if is_root_action {
            let active_query = self.state.current().search_query.clone();
            let outcome = self
                .root_search
                .borrow_mut()
                .execute_action(action_id, &active_query);

            match outcome {
                RootActionOutcome::Launched { hud_title } => {
                    show_hud_pill(hud_title, cx);
                    window.remove_window();
                }
                RootActionOutcome::CopyToClipboard { text, hud_title } => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.state
                        .show_toast(ToastStyle::Success, hud_title.clone(), None);
                    show_hud_pill(hud_title, cx);
                }
                RootActionOutcome::PushView { frame_id, view } => {
                    self.state
                        .push(crate::state::NavFrame::new(frame_id, *view));
                    self.request_scroll_sync();
                }
                RootActionOutcome::OpenUri { uri } => {
                    open_deep_link(&mut self.state, &uri);
                    self.refresh_root_search();
                    self.request_scroll_sync();
                }
                RootActionOutcome::OpenWorkbench => {
                    self.state.push(build_root_showcase_frame());
                    self.request_scroll_sync();
                }
                RootActionOutcome::CycleTheme => {
                    self.apply_effect(ShellEffect::CycleTheme, window, cx);
                }
                RootActionOutcome::Refresh { toast_title } => {
                    self.refresh_root_search();
                    self.state
                        .show_toast(ToastStyle::Success, toast_title, None);
                    self.request_scroll_sync();
                }
                RootActionOutcome::Error { title, message } => {
                    self.state
                        .show_toast(ToastStyle::Failure, title, Some(message));
                }
            }
            cx.notify();
            return;
        }

        let effect = dispatch_demo_action(&mut self.state, action_id);
        if action_id == "search.clear" {
            self.refresh_root_search();
        }
        self.request_scroll_sync();
        self.apply_effect(effect, window, cx);
        cx.notify();
    }

    fn apply_effect(&mut self, effect: ShellEffect, window: &mut Window, cx: &mut Context<Self>) {
        match effect {
            ShellEffect::None => {}
            ShellEffect::HideWindow => {
                window.remove_window();
            }
            ShellEffect::CycleTheme => {
                let next_label = {
                    let theme = cx.global_mut::<AiosTheme>();
                    theme.cycle_mode();
                    theme.mode_label()
                };
                self.state
                    .show_toast(ToastStyle::Success, format!("Theme: {next_label}"), None);
            }
            ShellEffect::SetTheme(mode) => {
                let next_label = {
                    let theme = cx.global_mut::<AiosTheme>();
                    theme.set_mode(mode);
                    theme.mode_label()
                };
                self.state
                    .show_toast(ToastStyle::Success, format!("Theme: {next_label}"), None);
            }
            ShellEffect::ShowHud(hud) => {
                show_hud_pill(hud.title, cx);
            }
            ShellEffect::ActionTriggered(id) => {
                self.state
                    .show_toast(ToastStyle::Success, format!("Triggered {id}"), None);
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;

        // Global keyboard shortcuts matching `DESIGN.md`.
        if key == "escape" {
            let effect = self.state.handle_escape();
            self.refresh_root_search();
            self.apply_effect(effect, window, cx);
            cx.notify();
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("k") {
            self.state.toggle_action_panel();
            cx.notify();
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("t") {
            self.apply_effect(ShellEffect::CycleTheme, window, cx);
            cx.notify();
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("f") {
            let can_toggle = self.state.overlay.is_none()
                || matches!(
                    self.state.overlay,
                    Some(ActiveOverlay::FilterDropdown { .. })
                );
            if can_toggle && self.current_filter_dropdown().is_some() {
                self.state.toggle_filter_dropdown();
                cx.notify();
            }
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("p") {
            self.state.move_vertical(-1);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("n") {
            self.state.move_vertical(1);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "up" {
            self.state.move_vertical(-1);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "down" {
            self.state.move_vertical(1);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "left" {
            self.state.move_horizontal(-1);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "right" {
            self.state.move_horizontal(1);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "tab" {
            self.state.handle_tab(mods.shift);
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "backspace" {
            self.state.handle_backspace();
            self.refresh_root_search();
            self.request_scroll_sync();
            cx.notify();
            return;
        }

        if key == "enter" {
            if mods.control {
                self.trigger_secondary_action(window, cx);
                return;
            }
            self.handle_enter(window, cx);
            return;
        }

        if !mods.control
            && !mods.alt
            && !mods.platform
            && let Some(ch) = event.keystroke.key_char.as_deref()
            && !ch.is_empty()
            && !ch.chars().all(char::is_control)
        {
            self.state.insert_text(ch);
            self.refresh_root_search();
            self.request_scroll_sync();
            cx.notify();
        }
    }

    fn handle_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overlay) = self.state.overlay.clone() {
            match overlay {
                ActiveOverlay::ConfirmAlert(alert) => {
                    self.execute_action(&alert.id, window, cx);
                    return;
                }
                ActiveOverlay::FilterDropdown { selected_index } => {
                    self.state.apply_filter_option(selected_index);
                    self.refresh_root_search();
                    self.request_scroll_sync();
                    cx.notify();
                    return;
                }
                ActiveOverlay::ActionPanel {
                    selected_index,
                    submenu_parent,
                    ..
                } => {
                    let items = self.state.visible_action_panel_items();
                    if let Some(item) = items.get(selected_index) {
                        if submenu_parent.is_none() && !item.submenu.is_empty() {
                            self.state.overlay = Some(ActiveOverlay::ActionPanel {
                                filter_query: String::new(),
                                selected_index: 0,
                                submenu_parent: Some(item.id.clone()),
                            });
                            cx.notify();
                        } else {
                            let id = item.id.clone();
                            self.execute_action(&id, window, cx);
                        }
                    }
                    return;
                }
            }
        }

        if self.state.current().header_focus == HeaderFocus::FilterDropdown {
            self.state.toggle_filter_dropdown();
            cx.notify();
            return;
        }

        self.trigger_primary_action(window, cx);
    }

    fn current_filter_dropdown(&self) -> Option<&FilterDropdown> {
        match &self.state.current().view {
            ViewNode::List(l) => l.filter_dropdown.as_ref(),
            ViewNode::Grid(g) => g.filter_dropdown.as_ref(),
            _ => None,
        }
    }

    fn render_header(&self, theme: &AiosTheme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let frame = self.state.current();
        let depth = self.state.depth();

        let left_slot: AnyElement = if depth > 1 {
            div()
                .id("aios-header-back")
                .w(px(28.0))
                .h(px(28.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.metrics.radius_sm)
                .bg(c.bg_subtle)
                .border_1()
                .border_color(c.border_subtle)
                .text_color(c.text_primary)
                .font(theme.mono_font())
                .text_size(px(13.0))
                .cursor_pointer()
                .hover(|s| s.bg(c.bg_hover).border_color(c.border_default))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.state.pop();
                    this.refresh_root_search();
                    this.request_scroll_sync();
                    cx.notify();
                }))
                .child(back_arrow_icon(c.text_primary))
                .into_any_element()
        } else {
            aios_brand_mark(theme).into_any_element()
        };

        let center_slot: AnyElement = match &frame.view {
            ViewNode::List(l) => {
                let placeholder = l.search_placeholder.as_deref().unwrap_or("Search AIOS...");
                self.render_search_input(placeholder, &l.inline_arguments, theme, cx)
            }
            ViewNode::Grid(g) => {
                let placeholder = g.search_placeholder.as_deref().unwrap_or("Filter grid...");
                self.render_search_input(placeholder, &[], theme, cx)
            }
            _ => div()
                .flex_1()
                .min_w(px(0.0))
                .text_color(c.text_primary)
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(frame.title().to_owned())
                .into_any_element(),
        };

        let mut header_row = div()
            .w_full()
            .h(theme.metrics.header_height)
            .px(px(16.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .child(left_slot)
            .child(center_slot);

        if let Some(dropdown) = self.current_filter_dropdown() {
            let active_label = dropdown
                .options
                .iter()
                .find(|o| o.id == dropdown.selected_id)
                .map(|o| o.title.as_str())
                .unwrap_or("Filter");
            let focused = frame.header_focus == HeaderFocus::FilterDropdown
                || matches!(
                    self.state.overlay,
                    Some(ActiveOverlay::FilterDropdown { .. })
                );

            header_row = header_row.child(
                div()
                    .id("aios-filter-dropdown-btn")
                    .relative()
                    .flex_shrink_0()
                    .px(px(8.0))
                    .py(px(5.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(theme.metrics.radius_sm)
                    .bg(c.bg_subtle)
                    .border_1()
                    .border_color(if focused {
                        c.border_focus
                    } else {
                        c.border_default
                    })
                    .cursor_pointer()
                    .when(!focused, |d| d.hover(|s| s.bg(c.bg_hover)))
                    .when(focused, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(5.0))
                                .border_1()
                                .border_color(c.border_focus),
                        )
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.toggle_filter_dropdown();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_color(c.text_primary)
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(active_label.to_owned()),
                    )
                    .child(keycap("Ctrl+F", theme)),
            );
        }

        // Theme mode indicator button (clickable to cycle Dark / Light / System live).
        header_row = header_row.child(
            div()
                .id("aios-theme-toggle-btn")
                .flex_shrink_0()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(theme.metrics.radius_xs)
                .bg(c.bg_subtle)
                .border_1()
                .border_color(c.border_subtle)
                .text_color(c.text_secondary)
                .font_family(theme.font_mono)
                .text_size(px(11.0))
                .cursor_pointer()
                .hover(|s| s.bg(c.bg_hover).border_color(c.border_default))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.apply_effect(ShellEffect::CycleTheme, window, cx);
                    cx.notify();
                }))
                .child(theme.mode_label()),
        );

        let is_loading = frame.is_loading();
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(header_row)
            // 1px bottom divider, upgraded to 2px Signal Copper track when loading (zero layout shift).
            .child(
                div()
                    .w_full()
                    .h(if is_loading { px(2.0) } else { px(1.0) })
                    .bg(c.border_subtle)
                    .when(is_loading, |d| {
                        d.child(div().w_1_2().h_full().bg(c.accent_solid))
                    }),
            )
            .into_any_element()
    }

    fn render_search_input(
        &self,
        placeholder: &str,
        inline_args: &[aios_core::view::InlineArgumentSpec],
        theme: &AiosTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let frame = self.state.current();
        let search_focused =
            frame.header_focus == HeaderFocus::Search && self.state.overlay.is_none();

        let mut bar = div()
            .id("aios-search-input")
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .cursor_text()
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.current_mut().header_focus = HeaderFocus::Search;
                cx.notify();
            }));

        if frame.search_query.is_empty() {
            bar = bar.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .when(search_focused, |d| {
                        d.child(div().w(px(2.0)).h(px(18.0)).mr(px(2.0)).bg(c.accent_solid))
                    })
                    .child(
                        div()
                            .text_color(c.text_muted)
                            .text_size(px(16.0))
                            .line_height(px(24.0))
                            .child(placeholder.to_owned()),
                    ),
            );
        } else {
            bar = bar.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .text_color(c.text_primary)
                    .text_size(px(16.0))
                    .line_height(px(24.0))
                    .child(frame.search_query.clone())
                    .when(search_focused, |d| {
                        d.child(div().w(px(2.0)).h(px(18.0)).ml(px(1.0)).bg(c.accent_solid))
                    }),
            );
        }

        // Inline command argument pills (`Tab` moves focus into pills per DESIGN.md).
        for (idx, spec) in inline_args.iter().enumerate() {
            let val = frame
                .inline_arg_values
                .get(idx)
                .map(String::as_str)
                .unwrap_or("");
            let is_arg_focused = frame.header_focus == HeaderFocus::InlineArg(idx);

            bar = bar.child(
                div()
                    .id(("aios-inline-arg", idx))
                    .relative()
                    .px(px(7.0))
                    .py(px(3.0))
                    .rounded(theme.metrics.radius_xs)
                    .bg(c.bg_subtle)
                    .border_1()
                    .border_color(if is_arg_focused {
                        c.border_focus
                    } else {
                        c.border_default
                    })
                    .cursor_pointer()
                    .when(is_arg_focused, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(3.0))
                                .border_1()
                                .border_color(c.border_focus),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.current_mut().header_focus = HeaderFocus::InlineArg(idx);
                        cx.notify();
                    }))
                    .font_family(theme.font_mono)
                    .text_size(px(11.5))
                    .text_color(if val.is_empty() {
                        c.text_muted
                    } else {
                        c.text_primary
                    })
                    .child(if val.is_empty() {
                        spec.placeholder.clone()
                    } else {
                        val.to_owned()
                    }),
            );
        }

        bar.into_any_element()
    }

    fn render_footer(&self, theme: &AiosTheme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let frame = self.state.current();
        let panel = frame.active_action_panel();
        let primary = panel.primary_action();

        let left_slot: AnyElement = if let Some(toast) = &self.state.toast {
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(render_footer_toast(toast, theme))
                .into_any_element()
        } else {
            let count_meta = match &frame.view {
                ViewNode::List(_) => format!("{} items", frame.filtered_list_entries().len()),
                ViewNode::Grid(_) => format!("{} tiles", frame.filtered_grid_entries().len()),
                ViewNode::Detail(_) => "Detail".to_string(),
                ViewNode::Form(_) => "Form".to_string(),
                ViewNode::Empty(_) => "Empty".to_string(),
                ViewNode::Error(_) => "Error".to_string(),
            };
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .overflow_hidden()
                .child(
                    div()
                        .text_color(c.text_secondary)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(frame.title().to_owned()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(c.text_muted)
                        .font(theme.mono_font())
                        .text_size(px(11.0))
                        .child(count_meta),
                )
                .into_any_element()
        };

        let mut right_slot = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .flex_shrink_0();

        if let Some(action) = primary {
            right_slot = right_slot.child(
                div()
                    .id("aios-footer-primary-btn")
                    .px(px(6.0))
                    .py(px(4.0))
                    .rounded(theme.metrics.radius_xs)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(c.bg_hover))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.trigger_primary_action(window, cx);
                    }))
                    .child(
                        div()
                            .text_color(if action.destructive {
                                c.status_danger
                            } else {
                                c.text_primary
                            })
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .whitespace_nowrap()
                            .child(action.title.clone()),
                    )
                    .child(enter_keycap(theme)),
            );

            right_slot = right_slot
                .child(div().w(px(1.0)).h(px(16.0)).bg(c.border_subtle))
                .child(
                    div()
                        .id("aios-footer-actions-btn")
                        .px(px(6.0))
                        .py(px(4.0))
                        .rounded(theme.metrics.radius_xs)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(c.bg_hover))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.toggle_action_panel();
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_color(c.text_secondary)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child("Actions"),
                        )
                        .child(keycap("Ctrl+K", theme)),
                );
        }

        div()
            .w_full()
            .h(theme.metrics.footer_height)
            .flex_shrink_0()
            .px(px(14.0))
            .border_t_1()
            .border_color(c.border_subtle)
            .bg(c.bg_subtle)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(left_slot)
            .child(right_slot)
            .into_any_element()
    }

    fn render_overlay(&self, theme: &AiosTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let overlay = self.state.overlay.as_ref()?;
        let c = &theme.colors;

        match overlay {
            ActiveOverlay::ActionPanel {
                filter_query,
                selected_index,
                submenu_parent,
            } => {
                let items = self.state.visible_action_panel_items();
                let mut list = div()
                    .id("aios-action-panel-scroll")
                    .w_full()
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.action_panel_scroll)
                    .flex()
                    .flex_col()
                    .p(px(6.0))
                    .gap(px(2.0));

                if self.take_scroll_sync() {
                    let offset = usize::from(submenu_parent.is_some());
                    self.action_panel_scroll
                        .scroll_to_item(selected_index + offset);
                }

                if submenu_parent.is_some() {
                    list = list.child(
                        div()
                            .id("aios-action-submenu-back")
                            .px(px(8.0))
                            .py(px(5.0))
                            .text_color(c.text_muted)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.move_horizontal(-1);
                                cx.notify();
                            }))
                            .child("← Back to Actions"),
                    );
                }

                if items.is_empty() {
                    list = list.child(
                        div()
                            .p(px(12.0))
                            .text_color(c.text_muted)
                            .text_size(px(12.0))
                            .child("No actions match your filter."),
                    );
                } else {
                    for (idx, action) in items.iter().enumerate() {
                        let selected = idx == *selected_index;
                        let action_id = action.id.clone();
                        let has_sub = !action.submenu.is_empty() && submenu_parent.is_none();

                        let mut row = div()
                            .id(("aios-action-row", idx))
                            .w_full()
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(theme.metrics.radius_sm)
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .cursor_pointer()
                            .bg(if selected {
                                c.bg_selected
                            } else {
                                c.bg_elevated
                            })
                            .hover(|s| s.bg(if selected { c.bg_selected } else { c.bg_hover }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if has_sub {
                                    this.state.overlay = Some(ActiveOverlay::ActionPanel {
                                        filter_query: String::new(),
                                        selected_index: 0,
                                        submenu_parent: Some(action_id.clone()),
                                    });
                                    cx.notify();
                                } else {
                                    this.execute_action(&action_id, window, cx);
                                }
                            }))
                            .child(
                                div()
                                    .text_color(if action.destructive {
                                        c.status_danger
                                    } else {
                                        c.text_primary
                                    })
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(action.title.clone()),
                            );

                        if has_sub {
                            row = row.child(keycap("→", theme));
                        } else if let Some(sc) = &action.shortcut {
                            row = row.child(keycap(sc, theme));
                        }

                        list = list.child(row);
                    }
                }

                // Bottom-anchored mini filter input matching `DESIGN.md`
                let filter_bar = div()
                    .w_full()
                    .px(px(10.0))
                    .py(px(8.0))
                    .border_t_1()
                    .border_color(c.border_subtle)
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(if filter_query.is_empty() {
                        div()
                            .text_color(c.text_muted)
                            .text_size(px(12.0))
                            .child("Filter actions...")
                    } else {
                        div()
                            .text_color(c.text_primary)
                            .text_size(px(12.0))
                            .child(filter_query.clone())
                    });

                let popover = div()
                    .w(theme.metrics.action_panel_width)
                    .max_h(theme.metrics.action_panel_max_height)
                    .flex()
                    .flex_col()
                    .rounded(theme.metrics.radius_lg)
                    .bg(c.bg_elevated)
                    .border_1()
                    .border_color(c.border_default)
                    .shadow(popover_shadow(theme))
                    .child(list)
                    .child(filter_bar);

                Some(
                    div()
                        .absolute()
                        .bottom(px(48.0))
                        .right(px(12.0))
                        .child(popover)
                        .into_any_element(),
                )
            }
            ActiveOverlay::FilterDropdown { selected_index } => {
                let dropdown = self.current_filter_dropdown()?;
                let mut menu = div()
                    .w(px(220.0))
                    .p(px(6.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .rounded(theme.metrics.radius_lg)
                    .bg(c.bg_elevated)
                    .border_1()
                    .border_color(c.border_default)
                    .shadow(popover_shadow(theme));

                for (idx, opt) in dropdown.options.iter().enumerate() {
                    let is_sel = idx == *selected_index;
                    let is_active = opt.id == dropdown.selected_id;
                    menu = menu.child(
                        div()
                            .id(("aios-filter-opt", idx))
                            .w_full()
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(theme.metrics.radius_sm)
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .bg(if is_sel { c.bg_selected } else { c.bg_elevated })
                            .hover(|s| s.bg(if is_sel { c.bg_selected } else { c.bg_hover }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.apply_filter_option(idx);
                                this.refresh_root_search();
                                this.request_scroll_sync();
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_color(c.text_primary)
                                    .text_size(px(12.5))
                                    .child(opt.title.clone()),
                            )
                            .when(is_active, |d| {
                                d.child(
                                    div()
                                        .w(px(6.0))
                                        .h(px(6.0))
                                        .rounded(px(9999.0))
                                        .bg(c.accent_solid),
                                )
                            }),
                    );
                }

                Some(
                    div()
                        .absolute()
                        .top(px(56.0))
                        .right(px(92.0))
                        .child(menu)
                        .into_any_element(),
                )
            }
            ActiveOverlay::ConfirmAlert(alert) => {
                let alert_id = alert.id.clone();
                let cancel_btn = div()
                    .id("aios-alert-cancel")
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(theme.metrics.radius_sm)
                    .bg(c.bg_subtle)
                    .border_1()
                    .border_color(c.border_default)
                    .text_color(c.text_primary)
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .hover(|s| s.bg(c.bg_hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.overlay = None;
                        cx.notify();
                    }))
                    .child(alert.dismiss_action.clone())
                    .into_any_element();

                let confirm_btn = div()
                    .id("aios-alert-confirm")
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(theme.metrics.radius_sm)
                    .bg(if alert.destructive {
                        c.status_danger
                    } else {
                        c.accent_solid
                    })
                    .text_color(c.text_on_accent)
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.execute_action(&alert_id, window, cx);
                    }))
                    .child(alert.primary_action.clone())
                    .into_any_element();

                Some(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(c.bg_scrim)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(render_confirm_alert_card(
                            alert,
                            theme,
                            cancel_btn,
                            confirm_btn,
                        ))
                        .into_any_element(),
                )
            }
        }
    }
}

impl Render for LauncherWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<AiosTheme>().clone();
        let c = &theme.colors;

        let viewport: AnyElement = match &self.state.current().view.clone() {
            ViewNode::List(list) => render_list_view(self, list, &theme, cx),
            ViewNode::Grid(grid) => render_grid_view(self, grid, &theme, cx),
            ViewNode::Detail(detail) => render_detail_view(detail, &theme),
            ViewNode::Form(form) => render_form_view(self, form, &theme, cx),
            ViewNode::Empty(empty) => div()
                .id("aios-empty-action")
                .size_full()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.trigger_primary_action(window, cx);
                }))
                .child(render_empty_state(empty, &theme))
                .into_any_element(),
            ViewNode::Error(error) => div()
                .id("aios-error-action")
                .size_full()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.trigger_primary_action(window, cx);
                }))
                .child(render_error_state(error, &theme))
                .into_any_element(),
        };

        let mut shell = div()
            .id("aios-root-shell")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(theme.metrics.radius_window)
            .bg(c.bg_canvas_root)
            .border_1()
            .border_color(c.border_default)
            .font(theme.sans_font())
            .text_color(c.text_primary)
            .shadow(window_shadow(&theme))
            .child(self.render_header(&theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .overflow_hidden()
                    .child(viewport),
            )
            .child(self.render_footer(&theme, cx));

        if let Some(overlay) = self.render_overlay(&theme, cx) {
            shell = shell.child(overlay);
        }

        shell
    }
}

fn open_launcher_window(
    config: &AiosConfig,
    root_search: Rc<RefCell<RootSearchOrchestrator>>,
    initial_uri: Option<&str>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<LauncherWindow>> {
    let width = config.window.width as f32;
    let height = config.window.default_height as f32;
    // Approximate top offset from configured monitor percentage (clamped cleanly for 1080p/1440p).
    let top_margin =
        ((f32::from(config.window.top_offset_percent) / 100.0) * 1080.0).clamp(48.0, 260.0);

    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: Size::new(px(width), px(height)),
    };

    let layer_opts = WindowOptions::new()
        .titlebar(None)
        .window_bounds(Some(WindowBounds::Windowed(bounds)))
        .app_id(Some("aios".to_string()))
        .kind(WindowKind::LayerShell(LayerShellOptions {
            namespace: "aios".to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP,
            margin: Some((px(top_margin), px(0.0), px(0.0), px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        }))
        .linux_window_background(LinuxWindowBackground::Transparent);

    let uri_owned = initial_uri.map(ToOwned::to_owned);
    let uri_fallback = uri_owned.clone();
    let rs_primary = Rc::clone(&root_search);
    let rs_fallback = root_search;

    let handle = cx
        .open_window(layer_opts, move |window, cx| {
            cx.new(|cx| LauncherWindow::new(rs_primary, uri_owned.as_deref(), window, cx))
        })
        .or_else(|err| {
            tracing::warn!(
                error = %err,
                "zwlr_layer_shell_v1 unavailable; opening standard PopUp window fallback"
            );
            let fallback_opts = WindowOptions::new()
                .titlebar(None)
                .window_bounds(Some(WindowBounds::Windowed(bounds)))
                .app_id(Some("aios".to_string()))
                .kind(WindowKind::PopUp)
                .linux_window_background(LinuxWindowBackground::Transparent);
            cx.open_window(fallback_opts, move |window, cx| {
                cx.new(|cx| LauncherWindow::new(rs_fallback, uri_fallback.as_deref(), window, cx))
            })
        })?;

    Ok(handle)
}

fn open_orchestrator(config: &AiosConfig, paths: &AiosPaths) -> RootSearchOrchestrator {
    let db = match Database::open(&paths.database_file()) {
        Ok(db) => Some(db),
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %paths.database_file().display(),
                "SQLite unavailable; using in-memory frecency database for this session"
            );
            Database::open_in_memory().ok()
        }
    };
    RootSearchOrchestrator::new(config.clone(), db)
}

/// Runs the persistent single-instance AIOS daemon with Unix socket IPC + live XDG portal theme watcher.
pub fn run_daemon_app(
    config: AiosConfig,
    paths: AiosPaths,
    show_immediately: bool,
) -> aios_core::Result<()> {
    let (_ipc_server, ipc_rx) = IpcServer::bind(&paths.runtime_socket)?;
    let (_portal_watcher, initial_scheme, portal_rx) = PortalThemeWatcher::spawn();
    let compositor = detect_compositor();
    let orchestrator = open_orchestrator(&config, &paths);

    tracing::info!(
        socket = %paths.runtime_socket.display(),
        ?compositor,
        ?initial_scheme,
        indexed_apps = orchestrator.visible_app_count(),
        "starting AIOS GPUI daemon"
    );

    gpui_platform::application()
        .with_assets(crate::fonts::AiosAssetSource)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            // Keep IPC server & portal watcher alive for the lifetime of the GPUI loop.
            let _keep_alive = (_ipc_server, _portal_watcher);
            crate::fonts::register_embedded_fonts(cx);

            cx.set_global(AiosTheme::new(
                config.theme.mode,
                initial_scheme.prefers_dark(),
                config.theme.translucent_root,
                config.theme.reduced_motion,
            ));

            let root_search = Rc::new(RefCell::new(orchestrator));
            let active_window: Rc<RefCell<Option<WindowHandle<LauncherWindow>>>> =
                Rc::new(RefCell::new(None));

            if show_immediately
                && let Ok(handle) = open_launcher_window(&config, Rc::clone(&root_search), None, cx)
            {
                *active_window.borrow_mut() = Some(handle);
            }

            // Watch live XDG Desktop Portal `color-scheme` changes.
            let win_for_theme = Rc::clone(&active_window);
            cx.spawn(async move |cx| {
                while let Ok(scheme) = portal_rx.recv().await {
                    cx.update(|cx| {
                        cx.global_mut::<AiosTheme>()
                            .set_system_prefers_dark(scheme.prefers_dark());
                        if let Some(handle) = *win_for_theme.borrow() {
                            let _ = handle.update(cx, |_, _, cx| cx.notify());
                        }
                    });
                }
            })
            .detach();

            // Process incoming Unix socket IPC commands (`toggle`, `show`, `hide`, `open`, `status`).
            let cfg_for_ipc = config.clone();
            let win_for_ipc = Rc::clone(&active_window);
            let rs_for_ipc = Rc::clone(&root_search);
            cx.spawn(async move |cx| {
                let _hold = _keep_alive;
                while let Ok(envelope) = ipc_rx.recv().await {
                    let mut reply = DaemonIpcResponse::Ok;
                    cx.update(|cx| {
                        // Drop stale handle if user already closed window via Escape.
                        let existing = *win_for_ipc.borrow();
                        let still_open =
                            existing.is_some_and(|h| h.update(cx, |_, _, _| ()).is_ok());
                        if !still_open {
                            *win_for_ipc.borrow_mut() = None;
                        }

                        match &envelope.command {
                            DaemonIpcCommand::Toggle => {
                                if let Some(handle) = win_for_ipc.borrow_mut().take() {
                                    let _ =
                                        handle.update(cx, |_, window, _| window.remove_window());
                                } else {
                                    match open_launcher_window(
                                        &cfg_for_ipc,
                                        Rc::clone(&rs_for_ipc),
                                        None,
                                        cx,
                                    ) {
                                        Ok(h) => *win_for_ipc.borrow_mut() = Some(h),
                                        Err(err) => {
                                            reply = DaemonIpcResponse::Error {
                                                message: err.to_string(),
                                            };
                                        }
                                    }
                                }
                            }
                            DaemonIpcCommand::Show => {
                                if let Some(handle) = *win_for_ipc.borrow() {
                                    let _ = handle.update(cx, |_, window, _| {
                                        window.activate();
                                    });
                                } else if let Ok(h) = open_launcher_window(
                                    &cfg_for_ipc,
                                    Rc::clone(&rs_for_ipc),
                                    None,
                                    cx,
                                ) {
                                    *win_for_ipc.borrow_mut() = Some(h);
                                }
                            }
                            DaemonIpcCommand::Hide => {
                                if let Some(handle) = win_for_ipc.borrow_mut().take() {
                                    let _ =
                                        handle.update(cx, |_, window, _| window.remove_window());
                                }
                            }
                            DaemonIpcCommand::Open { uri } => {
                                if let Some(handle) = *win_for_ipc.borrow() {
                                    let _ = handle.update(cx, |view, window, cx| {
                                        view.open_uri(uri, cx);
                                        window.activate();
                                    });
                                } else {
                                    match open_launcher_window(
                                        &cfg_for_ipc,
                                        Rc::clone(&rs_for_ipc),
                                        Some(uri),
                                        cx,
                                    ) {
                                        Ok(h) => *win_for_ipc.borrow_mut() = Some(h),
                                        Err(err) => {
                                            reply = DaemonIpcResponse::Error {
                                                message: err.to_string(),
                                            };
                                        }
                                    }
                                }
                            }
                            DaemonIpcCommand::Status => {
                                let visible = win_for_ipc.borrow().is_some();
                                let theme_label = cx.global::<AiosTheme>().mode_label().to_string();
                                reply = DaemonIpcResponse::Status(DaemonStatus {
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    visible,
                                    compositor: format!("{compositor:?}"),
                                    active_theme: theme_label,
                                });
                            }
                            DaemonIpcCommand::Ping => {
                                reply = DaemonIpcResponse::Pong {
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                };
                            }
                        }
                    });
                    envelope.respond(reply);
                }
            })
            .detach();
        });

    Ok(())
}

/// Runs a standalone foreground launcher window (exits when closed with `Esc`).
pub fn run_preview_app(
    mut config: AiosConfig,
    paths: AiosPaths,
    initial_uri: Option<String>,
    theme_override: Option<ThemeMode>,
) {
    if let Some(mode) = theme_override {
        config.theme.mode = mode;
    }
    let (_portal_watcher, initial_scheme, portal_rx) = PortalThemeWatcher::spawn();
    let orchestrator = open_orchestrator(&config, &paths);

    gpui_platform::application()
        .with_assets(crate::fonts::AiosAssetSource)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            let _keep_portal = _portal_watcher;
            crate::fonts::register_embedded_fonts(cx);
            cx.set_global(AiosTheme::new(
                config.theme.mode,
                initial_scheme.prefers_dark(),
                config.theme.translucent_root,
                config.theme.reduced_motion,
            ));

            let root_search = Rc::new(RefCell::new(orchestrator));
            let handle = open_launcher_window(&config, root_search, initial_uri.as_deref(), cx)
                .expect("failed to open AIOS preview window");

            cx.spawn(async move |cx| {
                let _hold = _keep_portal;
                while let Ok(scheme) = portal_rx.recv().await {
                    cx.update(|cx| {
                        cx.global_mut::<AiosTheme>()
                            .set_system_prefers_dark(scheme.prefers_dark());
                        let _ = handle.update(cx, |_, _, cx| cx.notify());
                    });
                }
            })
            .detach();
        });
}
