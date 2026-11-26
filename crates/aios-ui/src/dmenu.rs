//! Standalone and daemon-backed Wayland `dmenu` / `rofi -dmenu` compatible fuzzy picker.
//!
//! Reads newline-delimited choices, filters with `aios-fuzzy`, renders on AIOS's native GPUI
//! `WindowKind::LayerShell` surface with full Dark/Light/System parity, and returns either
//! `DmenuOutcome::Selected { value, index }` or `DmenuOutcome::Cancelled`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aios_builtin::IconResolver;
use aios_core::config::{AiosConfig, ThemeMode};
use aios_core::view::EmptyView;
use aios_daemon::{DmenuCaseMode, DmenuRequest, DmenuStreamMessage};
use aios_fuzzy::{
    CaseMatching, FrecencyConfig, FrecencyMap, MatchResult, QueryParser, SearchConfig, SearchItem,
    SearchSession, SearchSnapshot, score_item, search_parallel,
};
use aios_wayland::PortalThemeWatcher;
use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, FontWeight, KeyDownEvent, LinuxWindowBackground,
    QuitMode, Render, ScrollStrategy, Size, UniformListScrollHandle, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, div,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point,
    prelude::*,
    px, uniform_list,
};

use crate::components::{
    aios_brand_mark, app_icon_or_badge, enter_keycap, highlighted_text, item_badge, keycap,
    loading_progress_bar, render_empty_state, render_loading_state,
};
use crate::theme::AiosTheme;

/// Final outcome emitted when a `dmenu` window finishes or closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmenuOutcome {
    Selected { value: String, index: Option<usize> },
    Cancelled,
}

/// One filtered `dmenu` candidate linked back to its original 0-based stdin line index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmenuFilteredEntry {
    pub original_index: usize,
    pub match_result: MatchResult,
}

const DMENU_PARALLEL_THRESHOLD: usize = 512;
const DMENU_TOP_K_LIMIT: usize = 500;

/// Pure, headless-testable state controller for `aios dmenu` with cached multi-core
/// `aios-fuzzy` parallel filtering and O(batch) incremental streaming support.
#[derive(Debug, Clone)]
pub struct DmenuState {
    pub prompt: String,
    pub items: Vec<String>,
    pub item_icons: Vec<Option<String>>,
    pub header_icon: Option<String>,
    pub show_icons: bool,
    pub is_loading: bool,
    pub query: String,
    pub selected_index: usize,
    pub case_mode: DmenuCaseMode,
    pub allow_custom: bool,
    pub return_index: bool,
    pub total_matched: usize,
    cached_entries: Vec<DmenuFilteredEntry>,
}

struct DmenuItemAdapter<'a> {
    idx: usize,
    text: &'a str,
}

impl SearchItem for DmenuItemAdapter<'_> {
    fn id(&self) -> &str {
        self.text
    }
    fn primary_text(&self) -> &str {
        self.text
    }
    fn secondary_text(&self) -> Option<&str> {
        None
    }
    fn keywords(&self) -> &[String] {
        let _ = self.idx;
        &[]
    }
}

impl DmenuState {
    #[must_use]
    pub fn from_request(req: &DmenuRequest) -> Self {
        let prompt = req
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Filter...")
            .to_owned();
        let mut state = Self {
            prompt,
            items: req.items.clone(),
            item_icons: req.item_icons.clone(),
            header_icon: req.icon.clone(),
            show_icons: req.show_icons || req.item_icons.iter().any(Option::is_some),
            is_loading: req.streaming,
            query: String::new(),
            selected_index: 0,
            case_mode: req.case_mode,
            allow_custom: req.allow_custom,
            return_index: req.return_index,
            total_matched: 0,
            cached_entries: Vec::new(),
        };
        state.recompute_filter();
        state
    }

    /// Appends a newly arrived stdin batch incrementally without freezing active user typing.
    pub fn append_batch(&mut self, new_items: Vec<String>, new_icons: Vec<Option<String>>) {
        if new_items.is_empty() {
            return;
        }
        let start_idx = self.items.len();
        let incoming_has_icons = new_icons.iter().any(Option::is_some);
        if incoming_has_icons {
            self.show_icons = true;
        }
        if incoming_has_icons || !self.item_icons.is_empty() {
            self.item_icons.resize(start_idx, None);
            let mut padded_icons = new_icons;
            padded_icons.resize(new_items.len(), None);
            self.item_icons.extend(padded_icons);
        }
        self.items.extend(new_items);

        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            for original_index in start_idx..self.items.len() {
                self.cached_entries.push(DmenuFilteredEntry {
                    original_index,
                    match_result: MatchResult {
                        score: 0,
                        primary_indices: Vec::new(),
                        secondary_indices: Vec::new(),
                    },
                });
            }
            self.total_matched = self.items.len();
            return;
        }

        let case_mode = match self.case_mode {
            DmenuCaseMode::Smart => CaseMatching::Smart,
            DmenuCaseMode::Insensitive => CaseMatching::Ignore,
            DmenuCaseMode::Sensitive => CaseMatching::Respect,
        };
        let engine = QueryParser::parse(trimmed, case_mode);
        let config = FrecencyConfig::default();

        let mut added = false;
        for (offset, text) in self.items[start_idx..].iter().enumerate() {
            let original_index = start_idx + offset;
            let adapter = DmenuItemAdapter {
                idx: original_index,
                text,
            };
            if let Some(match_result) =
                score_item(&adapter, trimmed, engine.as_ref(), None, 0, &config)
            {
                self.total_matched += 1;
                self.cached_entries.push(DmenuFilteredEntry {
                    original_index,
                    match_result,
                });
                added = true;
            }
        }
        if added {
            self.cached_entries.sort_unstable_by(|a, b| {
                b.match_result
                    .score
                    .cmp(&a.match_result.score)
                    .then_with(|| a.original_index.cmp(&b.original_index))
            });
            if self.cached_entries.len() > DMENU_TOP_K_LIMIT {
                self.cached_entries.truncate(DMENU_TOP_K_LIMIT);
            }
            if self.selected_index >= self.cached_entries.len() {
                self.selected_index = 0;
            }
        }
    }

    pub fn finish_loading(&mut self) {
        self.is_loading = false;
    }

    /// Updates query text without running synchronous scoring (used by `DmenuWindow` when
    /// offloading large-corpus queries to [`SearchSession`] with `<= 1.5ms` UI-thread return).
    pub fn set_query_deferred(&mut self, next_query: String) {
        self.query = next_query;
        self.selected_index = 0;
        if self.query.trim().is_empty() {
            self.recompute_filter();
        }
    }

    /// Applies an inline or progressive [`SearchSnapshot`] produced by [`SearchSession`].
    pub fn apply_search_snapshot(&mut self, snapshot: SearchSnapshot) {
        self.total_matched = snapshot.total_matched;
        self.cached_entries = snapshot
            .matches
            .into_iter()
            .map(|ranked| DmenuFilteredEntry {
                original_index: ranked.item_index,
                match_result: ranked.result,
            })
            .collect();
        if self.selected_index >= self.cached_entries.len() {
            self.selected_index = 0;
        }
    }

    /// Returns the `aios_fuzzy::CaseMatching` corresponding to `self.case_mode`.
    #[must_use]
    pub fn case_matching(&self) -> CaseMatching {
        match self.case_mode {
            DmenuCaseMode::Smart => CaseMatching::Smart,
            DmenuCaseMode::Insensitive => CaseMatching::Ignore,
            DmenuCaseMode::Sensitive => CaseMatching::Respect,
        }
    }

    #[must_use]
    pub fn filtered_entries(&self) -> &[DmenuFilteredEntry] {
        &self.cached_entries
    }

    fn recompute_filter(&mut self) {
        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            self.cached_entries = (0..self.items.len())
                .map(|original_index| DmenuFilteredEntry {
                    original_index,
                    match_result: MatchResult {
                        score: 0,
                        primary_indices: Vec::new(),
                        secondary_indices: Vec::new(),
                    },
                })
                .collect();
            self.total_matched = self.items.len();
            return;
        }

        let case_matching = match self.case_mode {
            DmenuCaseMode::Smart => CaseMatching::Smart,
            DmenuCaseMode::Insensitive => CaseMatching::Ignore,
            DmenuCaseMode::Sensitive => CaseMatching::Respect,
        };

        if self.items.len() > DMENU_PARALLEL_THRESHOLD {
            let empty_frecency = FrecencyMap::new();
            let cfg = SearchConfig {
                limit: DMENU_TOP_K_LIMIT,
                case_matching,
                ..SearchConfig::default()
            };
            if let Some(snapshot) =
                search_parallel(&self.items, trimmed, &empty_frecency, 0, &cfg, None)
            {
                self.total_matched = snapshot.total_matched;
                self.cached_entries = snapshot
                    .matches
                    .into_iter()
                    .map(|ranked| DmenuFilteredEntry {
                        original_index: ranked.item_index,
                        match_result: ranked.result,
                    })
                    .collect();
                return;
            }
        }

        let engine = QueryParser::parse(trimmed, case_matching);
        let config = FrecencyConfig::default();

        let mut entries = Vec::new();
        for (original_index, text) in self.items.iter().enumerate() {
            let adapter = DmenuItemAdapter {
                idx: original_index,
                text,
            };
            if let Some(match_result) =
                score_item(&adapter, trimmed, engine.as_ref(), None, 0, &config)
            {
                entries.push(DmenuFilteredEntry {
                    original_index,
                    match_result,
                });
            }
        }

        entries.sort_unstable_by(|a, b| {
            b.match_result
                .score
                .cmp(&a.match_result.score)
                .then_with(|| a.original_index.cmp(&b.original_index))
        });
        self.total_matched = entries.len();
        self.cached_entries = entries;
    }

    pub fn move_vertical(&mut self, delta: isize) {
        let count = self.cached_entries.len();
        if count == 0 {
            self.selected_index = 0;
            return;
        }
        let len_i = count as isize;
        let next = (((self.selected_index as isize + delta) % len_i) + len_i) as usize % count;
        self.selected_index = next;
    }

    pub fn insert_text(&mut self, text: &str) {
        self.query.push_str(text);
        self.selected_index = 0;
        self.recompute_filter();
    }

    pub fn handle_backspace(&mut self) {
        if !self.query.is_empty() {
            self.query.pop();
            self.selected_index = 0;
            self.recompute_filter();
        }
    }

    /// Clears query if non-empty (returns `false`), or signals cancellation (returns `true`).
    pub fn handle_escape(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
            self.selected_index = 0;
            self.recompute_filter();
            false
        } else {
            true
        }
    }

    /// Confirms the highlighted match, or custom text when `allow_custom` is enabled.
    #[must_use]
    pub fn confirm(&self, prefer_custom: bool) -> Option<DmenuOutcome> {
        if prefer_custom && self.allow_custom && !self.query.is_empty() {
            return Some(DmenuOutcome::Selected {
                value: self.query.clone(),
                index: None,
            });
        }
        if let Some(entry) = self.cached_entries.get(self.selected_index) {
            return Some(DmenuOutcome::Selected {
                value: self.items[entry.original_index].clone(),
                index: Some(entry.original_index),
            });
        }
        if self.allow_custom && !self.query.is_empty() {
            return Some(DmenuOutcome::Selected {
                value: self.query.clone(),
                index: None,
            });
        }
        None
    }
}

type DmenuCallback = Box<dyn FnOnce(DmenuOutcome)>;
type SharedDmenuCallback = Rc<RefCell<Option<DmenuCallback>>>;

/// Interactive GPUI LayerShell view for `aios dmenu`.
pub struct DmenuWindow {
    pub state: DmenuState,
    pub focus_handle: FocusHandle,
    pub list_scroll: UniformListScrollHandle,
    sync_scroll: Cell<bool>,
    icon_resolver: Arc<Mutex<IconResolver>>,
    icon_cache: RefCell<HashMap<String, Option<String>>>,
    warm_icon_paths: RefCell<HashSet<String>>,
    typing_epoch: u64,
    icons_deferred: bool,
    search_session: SearchSession<String>,
    corpus_arc: Option<Arc<[String]>>,
    corpus_len: usize,
    empty_frecency: Arc<FrecencyMap>,
    active_search_gen: u64,
    is_searching: bool,
    responder: Option<DmenuCallback>,
}

impl std::fmt::Debug for DmenuWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DmenuWindow")
            .field("prompt", &self.state.prompt)
            .field("items", &self.state.items.len())
            .finish_non_exhaustive()
    }
}

impl Drop for DmenuWindow {
    fn drop(&mut self) {
        if let Some(cb) = self.responder.take() {
            cb(DmenuOutcome::Cancelled);
        }
    }
}

impl Focusable for DmenuWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DmenuWindow {
    pub fn new(
        request: &DmenuRequest,
        icon_resolver: Arc<Mutex<IconResolver>>,
        on_done: impl FnOnce(DmenuOutcome) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let state = DmenuState::from_request(request);
        let search_cfg = SearchConfig {
            limit: DMENU_TOP_K_LIMIT,
            inline_budget: Duration::from_micros(1_200),
            inline_max_items: DMENU_PARALLEL_THRESHOLD,
            case_matching: state.case_matching(),
            ..SearchConfig::default()
        };
        Self {
            state,
            focus_handle,
            list_scroll: UniformListScrollHandle::new(),
            sync_scroll: Cell::new(true),
            icon_resolver,
            icon_cache: RefCell::new(HashMap::new()),
            warm_icon_paths: RefCell::new(HashSet::new()),
            typing_epoch: 0,
            icons_deferred: false,
            search_session: SearchSession::new(search_cfg),
            corpus_arc: None,
            corpus_len: 0,
            empty_frecency: Arc::new(FrecencyMap::new()),
            active_search_gen: 0,
            is_searching: false,
            responder: Some(Box::new(on_done)),
        }
    }

    /// Schedules a 55ms settle timer so heavy uncached `.svg` rasterization (`usvg`/`tiny_skia`)
    /// never blocks rapid keystrokes, while already-cached icons still render at 0ms.
    fn mark_typing_burst(&mut self, cx: &mut Context<Self>) {
        if !self.state.show_icons {
            return;
        }
        self.typing_epoch = self.typing_epoch.wrapping_add(1);
        let epoch = self.typing_epoch;
        self.icons_deferred = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(55))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.typing_epoch == epoch && view.icons_deferred {
                    view.icons_deferred = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Runs instant inline search for small lists (`<= 512`) or time-budgeted (`<= 1.2ms`)
    /// adaptive parallel `SearchSession` with atomic keystroke cancellation for huge corpora.
    fn trigger_query_search(&mut self, next_query: String, cx: &mut Context<Self>) {
        self.mark_typing_burst(cx);

        let trimmed = next_query.trim();
        if trimmed.is_empty() {
            self.search_session.cancel_active();
            self.active_search_gen = self.active_search_gen.wrapping_add(1);
            self.is_searching = false;
            self.state.set_query_deferred(next_query);
            self.sync_scroll.set(true);
            cx.notify();
            return;
        }

        if self.state.items.len() <= DMENU_PARALLEL_THRESHOLD {
            self.is_searching = false;
            self.state.query = next_query;
            self.state.selected_index = 0;
            self.state.recompute_filter();
            self.sync_scroll.set(true);
            cx.notify();
            return;
        }

        self.state.set_query_deferred(next_query);
        if self.corpus_arc.is_none() || self.corpus_len != self.state.items.len() {
            self.corpus_arc = Some(Arc::from(self.state.items.as_slice()));
            self.corpus_len = self.state.items.len();
        }
        let corpus = Arc::clone(self.corpus_arc.as_ref().expect("corpus cached"));
        let handle = self.search_session.search(
            corpus,
            self.state.query.trim().to_owned(),
            Arc::clone(&self.empty_frecency),
            0,
        );
        let search_gen = handle.initial.generation;
        self.active_search_gen = search_gen;
        let is_complete = handle.initial.is_complete;
        self.is_searching = !is_complete;
        self.state.apply_search_snapshot(handle.initial);
        self.sync_scroll.set(true);
        cx.notify();

        if let Some(rx) = handle.updates {
            let (async_tx, async_rx) = async_channel::bounded::<SearchSnapshot>(8);
            std::thread::Builder::new()
                .name("aios-dmenu-search-rx".into())
                .spawn(move || {
                    while let Ok(snap) = rx.recv() {
                        let done = snap.is_complete;
                        if async_tx.send_blocking(snap).is_err() || done {
                            break;
                        }
                    }
                })
                .ok();

            cx.spawn(async move |this, cx| {
                while let Ok(snap) = async_rx.recv().await {
                    let done = snap.is_complete;
                    let keep_going = this
                        .update(cx, |view, cx| {
                            if view.active_search_gen != search_gen || snap.generation != search_gen
                            {
                                return false;
                            }
                            view.state.apply_search_snapshot(snap);
                            if done {
                                view.is_searching = false;
                            }
                            view.sync_scroll.set(true);
                            cx.notify();
                            !done
                        })
                        .unwrap_or(false);
                    if !keep_going {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    /// Lazily resolves an XDG icon name or file path ONLY for rows currently inside the viewport.
    fn resolve_icon_cached(&self, spec: &str) -> Option<String> {
        let key = spec.trim();
        if key.is_empty() {
            return None;
        }
        if let Some(cached) = self.icon_cache.borrow().get(key) {
            return cached.clone();
        }
        let resolved = if Path::new(key).is_file() {
            Some(key.to_owned())
        } else if let Ok(mut guard) = self.icon_resolver.lock() {
            guard.resolve(key).map(|p| p.to_string_lossy().into_owned())
        } else {
            None
        };
        self.icon_cache
            .borrow_mut()
            .insert(key.to_owned(), resolved.clone());
        resolved
    }

    fn finish(&mut self, outcome: DmenuOutcome, window: &mut Window) {
        if let Some(cb) = self.responder.take() {
            cb(outcome);
        }
        window.remove_window();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;

        if key == "escape" {
            if !self.state.query.is_empty() {
                self.trigger_query_search(String::new(), cx);
            } else {
                self.search_session.cancel_active();
                self.finish(DmenuOutcome::Cancelled, window);
            }
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("c") {
            self.search_session.cancel_active();
            self.finish(DmenuOutcome::Cancelled, window);
            return;
        }

        if mods.control && key.eq_ignore_ascii_case("t") {
            cx.global_mut::<AiosTheme>().cycle_mode();
            cx.notify();
            return;
        }

        if (mods.control && key.eq_ignore_ascii_case("p"))
            || key == "up"
            || (key == "tab" && mods.shift)
        {
            self.state.move_vertical(-1);
            self.sync_scroll.set(true);
            cx.notify();
            return;
        }

        if (mods.control && key.eq_ignore_ascii_case("n"))
            || key == "down"
            || (key == "tab" && !mods.shift)
        {
            self.state.move_vertical(1);
            self.sync_scroll.set(true);
            cx.notify();
            return;
        }

        if key == "backspace" {
            if !self.state.query.is_empty() {
                let mut next = self.state.query.clone();
                next.pop();
                self.trigger_query_search(next, cx);
            }
            return;
        }

        if key == "enter" {
            if let Some(outcome) = self.state.confirm(mods.control) {
                self.search_session.cancel_active();
                self.finish(outcome, window);
            }
            return;
        }

        if !mods.control
            && !mods.alt
            && !mods.platform
            && let Some(ch) = event.keystroke.key_char.as_deref()
            && !ch.is_empty()
            && !ch.chars().all(char::is_control)
        {
            let mut next = self.state.query.clone();
            next.push_str(ch);
            self.trigger_query_search(next, cx);
        }
    }
}

impl Render for DmenuWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<AiosTheme>().clone();
        let c = &theme.colors;
        let total_items = self.state.items.len();
        let matched_items = self.state.filtered_entries().len();
        let total_matched = self.state.total_matched;
        let is_prompt_only = total_items == 0 && self.state.allow_custom && !self.state.is_loading;

        let count_badge = if self.state.is_loading {
            if total_items > 0 {
                format!("{total_matched} / {total_items} · loading")
            } else {
                "loading...".to_string()
            }
        } else if total_items > 0 {
            format!("{total_matched} / {total_items}")
        } else {
            "input".to_string()
        };

        let header_left = match self.state.header_icon.as_deref() {
            Some(spec) => {
                if let Some(resolved_path) = self.resolve_icon_cached(spec) {
                    app_icon_or_badge(Some(&resolved_path), "DM", false, &theme)
                } else if !spec.trim().is_empty()
                    && !Path::new(spec.trim())
                        .components()
                        .any(|c| matches!(c, std::path::Component::RootDir))
                {
                    item_badge(spec.trim(), true, &theme).into_any_element()
                } else {
                    aios_brand_mark(&theme).into_any_element()
                }
            }
            None => aios_brand_mark(&theme).into_any_element(),
        };

        let header = div()
            .w_full()
            .h(theme.metrics.header_height)
            .px(px(16.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .child(header_left)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .overflow_hidden()
                    .child(if self.state.query.is_empty() {
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(1.5))
                                    .h(px(18.0))
                                    .mr(px(2.0))
                                    .bg(c.accent_solid),
                            )
                            .child(
                                div()
                                    .text_color(c.text_muted)
                                    .text_size(px(16.0))
                                    .line_height(px(22.0))
                                    .truncate()
                                    .child(self.state.prompt.clone()),
                            )
                    } else {
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .text_color(c.text_primary)
                            .text_size(px(16.0))
                            .line_height(px(22.0))
                            .whitespace_nowrap()
                            .child(self.state.query.clone())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(1.5))
                                    .h(px(18.0))
                                    .ml(px(1.0))
                                    .bg(c.accent_solid),
                            )
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(7.0))
                    .py(px(3.0))
                    .rounded(theme.metrics.radius_xs)
                    .bg(c.bg_subtle)
                    .border_1()
                    .border_color(c.border_subtle)
                    .text_color(c.text_muted)
                    .font(theme.mono_font())
                    .text_size(px(11.0))
                    .child(count_badge),
            );

        let body = if is_prompt_only {
            div()
                .flex_1()
                .w_full()
                .px(px(16.0))
                .py(px(10.0))
                .flex()
                .items_center()
                .text_color(c.text_secondary)
                .text_size(px(12.0))
                .child("Type a value and press Enter to confirm, or Esc to cancel.")
                .into_any_element()
        } else if matched_items == 0 {
            if self.state.is_loading {
                let msg = if self.state.query.is_empty() {
                    "Streaming items from stdin. You can start typing to filter immediately."
                        .to_string()
                } else {
                    format!(
                        "Searching incoming stream for '{}' ({total_items} scanned so far)...",
                        self.state.query
                    )
                };
                div()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.0))
                    .child(render_loading_state(&msg, &theme))
                    .into_any_element()
            } else {
                let empty = EmptyView {
                    title: if self.state.allow_custom && !self.state.query.is_empty() {
                        format!("Use \"{}\"", self.state.query)
                    } else {
                        "No matching items".to_string()
                    },
                    description: Some(if self.state.allow_custom {
                        "Press Enter to submit your custom input, or Esc to clear.".to_string()
                    } else {
                        format!(
                            "No entries matched '{}'. Press Esc to clear.",
                            self.state.query
                        )
                    }),
                    icon: Some("0".to_string()),
                    actions: Default::default(),
                };
                div()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.0))
                    .child(render_empty_state(&empty, &theme))
                    .into_any_element()
            }
        } else {
            if self.sync_scroll.replace(false) {
                self.list_scroll
                    .scroll_to_item(self.state.selected_index, ScrollStrategy::Nearest);
            }

            let theme_for_rows = theme.clone();
            let list = uniform_list(
                "aios-dmenu-scroll",
                matched_items,
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    let c = &theme_for_rows.colors;
                    let entries = this.state.filtered_entries();
                    let render_row_icons = this.state.show_icons;
                    let mut rows = Vec::with_capacity(range.len());

                    for flat_idx in range {
                        let Some(entry) = entries.get(flat_idx) else {
                            continue;
                        };
                        let is_selected = flat_idx == this.state.selected_index;
                        let orig_idx = entry.original_index;
                        let item_text = &this.state.items[orig_idx];
                        let row_val = item_text.clone();

                        let row_icon_el = if render_row_icons {
                            let spec = this
                                .state
                                .item_icons
                                .get(orig_idx)
                                .and_then(|opt| opt.as_deref());
                            let fallback_short: String = item_text.chars().take(2).collect();
                            let resolved_path = spec.and_then(|s| this.resolve_icon_cached(s));
                            // While user is mid-keystroke burst (`icons_deferred`), render already-warm
                            // GPU-cached icons immediately but defer brand-new `.svg` rasterization for 55ms
                            // so `usvg`/`tiny_skia` never stalls a single typed letter!
                            let effective_icon_path = resolved_path.as_deref().and_then(|path| {
                                let is_svg = path.ends_with(".svg") || path.ends_with(".SVG");
                                if !is_svg {
                                    return Some(path);
                                }
                                if this.icons_deferred {
                                    if this.warm_icon_paths.borrow().contains(path) {
                                        Some(path)
                                    } else {
                                        None
                                    }
                                } else {
                                    this.warm_icon_paths.borrow_mut().insert(path.to_owned());
                                    Some(path)
                                }
                            });
                            let badge = match (&resolved_path, spec) {
                                (None, Some(s))
                                    if !s.trim().is_empty()
                                        && !Path::new(s.trim()).components().any(|c| {
                                            matches!(c, std::path::Component::RootDir)
                                        }) =>
                                {
                                    s.trim()
                                }
                                _ => fallback_short.as_str(),
                            };
                            Some(app_icon_or_badge(
                                effective_icon_path,
                                badge,
                                is_selected,
                                &theme_for_rows,
                            ))
                        } else {
                            None
                        };

                        rows.push(
                            div()
                                .w_full()
                                .h(theme_for_rows.metrics.list_row_height + px(2.0))
                                .pb(px(2.0))
                                .child(
                                    div()
                                        .id(("aios-dmenu-row", flat_idx))
                                        .w_full()
                                        .h(theme_for_rows.metrics.list_row_height)
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(10.0))
                                        .px(px(10.0))
                                        .rounded(theme_for_rows.metrics.radius_sm)
                                        .cursor_pointer()
                                        .bg(if is_selected {
                                            c.bg_selected
                                        } else {
                                            c.bg_canvas
                                        })
                                        .hover(|s| {
                                            s.bg(if is_selected {
                                                c.bg_selected
                                            } else {
                                                c.bg_hover
                                            })
                                        })
                                        .on_click(cx.listener(move |this, _, window, _cx| {
                                            this.finish(
                                                DmenuOutcome::Selected {
                                                    value: row_val.clone(),
                                                    index: Some(orig_idx),
                                                },
                                                window,
                                            );
                                        }))
                                        .child(
                                            div()
                                                .w(px(2.0))
                                                .h(px(22.0))
                                                .flex_shrink_0()
                                                .rounded(theme_for_rows.metrics.radius_xs)
                                                .bg(if is_selected {
                                                    c.accent_solid
                                                } else {
                                                    c.bg_canvas
                                                }),
                                        )
                                        .when_some(row_icon_el, |d, icon_el| d.child(icon_el))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.0))
                                                .text_color(c.text_primary)
                                                .text_size(px(13.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .line_height(px(18.0))
                                                .child(highlighted_text(
                                                    item_text,
                                                    &entry.match_result.primary_indices,
                                                    is_selected,
                                                    &theme_for_rows,
                                                )),
                                        ),
                                ),
                        );
                    }
                    rows
                }),
            )
            .track_scroll(&self.list_scroll)
            .size_full()
            .p(px(8.0));

            div()
                .flex_1()
                .w_full()
                .min_h(px(0.0))
                .child(list)
                .into_any_element()
        };

        let footer = div()
            .w_full()
            .h(theme.metrics.footer_height)
            .flex_shrink_0()
            .px(px(14.0))
            .bg(c.bg_subtle)
            .rounded_b(theme.metrics.radius_window - px(1.0))
            .border_t_1()
            .border_color(c.border_subtle)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .px(px(6.0))
                            .py(px(1.5))
                            .rounded(theme.metrics.radius_xs)
                            .bg(c.bg_subtle)
                            .border_1()
                            .border_color(c.border_subtle)
                            .text_color(c.text_muted)
                            .font(theme.mono_font())
                            .text_size(px(10.0))
                            .child("dmenu"),
                    )
                    .when(self.state.case_mode != DmenuCaseMode::Smart, |d| {
                        let label = match self.state.case_mode {
                            DmenuCaseMode::Sensitive => "Aa sensitive",
                            DmenuCaseMode::Insensitive => "aa insensitive",
                            DmenuCaseMode::Smart => "",
                        };
                        d.child(
                            div()
                                .text_color(c.text_muted)
                                .font(theme.mono_font())
                                .text_size(px(10.0))
                                .child(label),
                        )
                    })
                    .when(self.state.allow_custom, |d| {
                        d.child(
                            div()
                                .text_color(c.text_muted)
                                .text_size(px(11.0))
                                .child("Ctrl+Enter: custom text"),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_color(c.text_secondary)
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Select"),
                            )
                            .child(enter_keycap(&theme)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_color(c.text_muted)
                                    .text_size(px(12.0))
                                    .child("Cancel"),
                            )
                            .child(keycap("Esc", &theme)),
                    ),
            );

        div()
            .id("aios-dmenu-shell")
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .rounded(theme.metrics.radius_window)
            .bg(c.bg_canvas_root)
            .border_1()
            .border_color(c.border_default)
            .overflow_hidden()
            .font(theme.sans_font())
            .text_color(c.text_primary)
            .child(header)
            .child(
                if (self.state.is_loading || self.is_searching) && matched_items > 0 {
                    loading_progress_bar(
                        "aios-dmenu-header-loading",
                        None,
                        2.0,
                        false,
                        &theme,
                    )
                } else {
                    div()
                        .w_full()
                        .h(px(1.0))
                        .bg(c.border_subtle)
                        .into_any_element()
                },
            )
            .child(body)
            .child(footer)
    }
}

/// Computes compact dmenu window height fitting the requested `-l`/`--lines` or item count.
#[must_use]
pub fn compute_dmenu_height(config: &AiosConfig, request: &DmenuRequest) -> f32 {
    if request.items.is_empty() && request.allow_custom && !request.streaming {
        return 132.0;
    }
    let visible_rows = request
        .lines
        .unwrap_or_else(|| {
            if request.streaming {
                8
            } else {
                request.items.len().clamp(1, 10)
            }
        })
        .clamp(1, 14);
    // 52px header + 1px divider + 40px footer + 16px list padding + rows * 42px
    let content_height = 52.0 + 1.0 + 40.0 + 16.0 + (visible_rows as f32) * 42.0;
    let max_h = (config.window.max_height as f32).max(260.0);
    content_height.clamp(160.0, max_h)
}

/// Opens a native Wayland LayerShell `DmenuWindow` inside an already-running GPUI `App` (daemon or standalone),
/// optionally consuming incremental [`DmenuStreamMessage`] batches while the window is already interactive.
pub fn open_dmenu_window(
    config: &AiosConfig,
    request: DmenuRequest,
    stream_rx: Option<async_channel::Receiver<DmenuStreamMessage>>,
    icon_resolver: Arc<Mutex<IconResolver>>,
    on_done: impl FnOnce(DmenuOutcome) + 'static,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<DmenuWindow>> {
    let width = (config.window.width as f32).min(640.0);
    let height = compute_dmenu_height(config, &request);
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

    let shared_cb: SharedDmenuCallback = Rc::new(RefCell::new(Some(Box::new(on_done))));
    let cb_primary = Rc::clone(&shared_cb);
    let cb_fallback = shared_cb;
    let res_primary = Arc::clone(&icon_resolver);
    let res_fallback = icon_resolver;
    let req_primary = request.clone();

    let handle = cx
        .open_window(layer_opts, move |window, cx| {
            cx.new(|cx| {
                let done = move |outcome: DmenuOutcome| {
                    if let Some(cb) = cb_primary.borrow_mut().take() {
                        cb(outcome);
                    }
                };
                DmenuWindow::new(&req_primary, res_primary, done, window, cx)
            })
        })
        .or_else(|err| {
            tracing::warn!(
                error = %err,
                "zwlr_layer_shell_v1 unavailable for dmenu; opening PopUp fallback"
            );
            let fallback_opts = WindowOptions::new()
                .titlebar(None)
                .window_bounds(Some(WindowBounds::Windowed(bounds)))
                .app_id(Some("aios".to_string()))
                .kind(WindowKind::PopUp)
                .linux_window_background(LinuxWindowBackground::Transparent);
            cx.open_window(fallback_opts, move |window, cx| {
                cx.new(|cx| {
                    let done = move |outcome: DmenuOutcome| {
                        if let Some(cb) = cb_fallback.borrow_mut().take() {
                            cb(outcome);
                        }
                    };
                    DmenuWindow::new(&request, res_fallback, done, window, cx)
                })
            })
        })?;

    if let Some(rx) = stream_rx {
        cx.spawn(async move |cx| {
            while let Ok(msg) = rx.recv().await {
                let mut should_stop = false;
                let updated = cx.update(|cx| {
                    handle.update(cx, |view, window, cx| match msg {
                        DmenuStreamMessage::Batch { items, item_icons } => {
                            view.corpus_arc = None;
                            view.state.append_batch(items, item_icons);
                            cx.notify();
                        }
                        DmenuStreamMessage::Eof => {
                            view.corpus_arc = None;
                            view.state.finish_loading();
                            should_stop = true;
                            if view.state.items.is_empty() && !view.state.allow_custom {
                                view.finish(DmenuOutcome::Cancelled, window);
                            } else {
                                cx.notify();
                            }
                        }
                    })
                });
                if should_stop || updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    Ok(handle)
}

/// Parses an optional theme string (`"dark"`, `"light"`, `"system"`).
#[must_use]
pub fn parse_theme_override(raw: Option<&str>) -> Option<ThemeMode> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("dark") => Some(ThemeMode::Dark),
        Some("light") => Some(ThemeMode::Light),
        Some("system") => Some(ThemeMode::System),
        _ => None,
    }
}

/// Runs a standalone foreground `aios dmenu` picker window (fallback when `aios daemon` is not running).
#[must_use]
pub fn run_dmenu_app(config: AiosConfig, request: DmenuRequest) -> DmenuOutcome {
    run_dmenu_app_streaming(config, request, None)
}

/// Runs a standalone foreground `aios dmenu` picker window with optional live stdin streaming.
#[must_use]
pub fn run_dmenu_app_streaming(
    mut config: AiosConfig,
    request: DmenuRequest,
    stream_rx: Option<async_channel::Receiver<DmenuStreamMessage>>,
) -> DmenuOutcome {
    if let Some(mode) = parse_theme_override(request.theme.as_deref()) {
        config.theme.mode = mode;
    }
    let (_portal_watcher, initial_scheme, portal_rx) = PortalThemeWatcher::spawn();
    let result_slot: Arc<Mutex<DmenuOutcome>> = Arc::new(Mutex::new(DmenuOutcome::Cancelled));
    let slot_for_ui = Arc::clone(&result_slot);

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

            let standalone_icons = Arc::new(Mutex::new(IconResolver::new()));
            let handle = open_dmenu_window(
                &config,
                request,
                stream_rx,
                standalone_icons,
                move |outcome| {
                    if let Ok(mut guard) = slot_for_ui.lock() {
                        *guard = outcome;
                    }
                },
                cx,
            )
            .expect("failed to open AIOS dmenu window");

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

    result_slot
        .lock()
        .map(|g| g.clone())
        .unwrap_or(DmenuOutcome::Cancelled)
}
