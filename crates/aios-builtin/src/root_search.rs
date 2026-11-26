//! Root Search Orchestrator combining XDG Applications, core commands, SQLite frecency,
//! user aliases, pinned items, hidden-app filtering, and Fallback Commands via `aios-fuzzy`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aios_core::config::AiosConfig;
use aios_core::storage::Database;
use aios_core::view::{
    Accessory, ActionItem, ActionPanel, ActionSection, DetailView, EmptyView, FilterDropdown,
    FilterMode, FilterOption, ListItem, ListSection, ListView, MetadataEntry, SemanticTone,
    ViewNode,
};
use aios_fuzzy::{
    CaseMatching, FrecencyConfig, FrecencyMap, FrecencyRecord, SearchConfig, SearchItem,
    search_parallel,
};

use crate::desktop::{
    DesktopApp, launch_desktop_app, open_url_detached, scan_desktop_applications,
};

/// Category of an external entry projected into Root Search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalRootKind {
    Plugin,
    ScriptCommand,
}

/// External WASM plugin or Script Command entry projected into Root Search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRootEntry {
    pub id: String,
    pub kind: ExternalRootKind,
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub icon_path: Option<String>,
    pub keywords: Vec<String>,
    pub filter_group: &'static str,
    pub action_id: String,
    pub secondary_action_id: Option<(String, String)>,
    pub tag_label: String,
    pub inline_summary: Option<String>,
}

/// Dynamic inline answer injected at the top of Root Search (e.g. Calculator preview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRootResult {
    pub id: String,
    pub section_title: String,
    pub section_subtitle: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub icon_path: Option<String>,
    pub tag_label: String,
    pub copy_action_id: String,
    pub copy_action_title: String,
    pub open_action_id: String,
    pub open_action_title: String,
    /// When `true` (e.g. explicit `=` prefix), hide other Root Search sections and show only inline answers.
    pub exclusive: bool,
}

/// Kind of entry indexed in AIOS Root Search.
#[derive(Debug, Clone)]
pub enum RootEntryKind {
    Application(Box<DesktopApp>),
    CoreCommand {
        command_id: String,
        subtitle: String,
        badge: String,
        keywords: Vec<String>,
    },
    External(Box<ExternalRootEntry>),
}

/// One searchable entry in the Root Search corpus.
#[derive(Debug, Clone)]
pub struct RootEntry {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub alias: Option<String>,
    pub keywords: Vec<String>,
    pub filter_group: &'static str,
    pub kind: RootEntryKind,
}

impl SearchItem for RootEntry {
    fn id(&self) -> &str {
        &self.id
    }
    fn primary_text(&self) -> &str {
        &self.title
    }
    fn secondary_text(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }
    fn keywords(&self) -> &[String] {
        &self.keywords
    }
    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

/// Result produced when executing a Root Search action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootActionOutcome {
    /// App launched; hide launcher window and display a brief HUD confirmation.
    Launched { hud_title: String },
    /// Copy text to Wayland clipboard and notify user.
    CopyToClipboard { text: String, hud_title: String },
    /// Push a sub-view (such as `.desktop` file detail inspector or Phase 2 workbench).
    PushView {
        frame_id: String,
        view: Box<ViewNode>,
    },
    /// Open an `aios://` deep-link URI (e.g. from a Fallback Command).
    OpenUri { uri: String },
    /// Open the Phase 2 View Workbench demo view.
    OpenWorkbench,
    /// Cycle Dark / Light / System theme mode.
    CycleTheme,
    /// Refresh the root search view with an optional status toast.
    Refresh { toast_title: String },
    /// Show an error toast without closing the launcher.
    Error { title: String, message: String },
}

/// Stateful Root Search Orchestrator backed by `aios-fuzzy` and SQLite `frecency`.
#[derive(Debug)]
pub struct RootSearchOrchestrator {
    config: AiosConfig,
    db: Option<Arc<Mutex<Database>>>,
    apps: Vec<DesktopApp>,
    external_entries: Vec<ExternalRootEntry>,
    entries: Arc<[RootEntry]>,
    hidden_ids: HashSet<String>,
    frecency_cache: FrecencyMap,
    last_query_prefix: String,
}

impl RootSearchOrchestrator {
    /// Initializes Root Search by scanning installed XDG applications and loading SQLite state.
    #[must_use]
    pub fn new(config: AiosConfig, db: Option<Database>) -> Self {
        let apps = scan_desktop_applications();
        Self::from_apps_shared(config, db.map(|d| Arc::new(Mutex::new(d))), apps)
    }

    /// Initializes Root Search using a shared SQLite handle (shared with `PluginHost`).
    #[must_use]
    pub fn new_shared(config: AiosConfig, db: Arc<Mutex<Database>>) -> Self {
        let apps = scan_desktop_applications();
        Self::from_apps_shared(config, Some(db), apps)
    }

    /// Creates an orchestrator from a pre-supplied list of apps (ideal for deterministic tests).
    #[must_use]
    pub fn from_apps(config: AiosConfig, db: Option<Database>, apps: Vec<DesktopApp>) -> Self {
        Self::from_apps_shared(config, db.map(|d| Arc::new(Mutex::new(d))), apps)
    }

    /// Creates an orchestrator from a shared SQLite handle and pre-supplied app list.
    #[must_use]
    pub fn from_apps_shared(
        config: AiosConfig,
        db: Option<Arc<Mutex<Database>>>,
        apps: Vec<DesktopApp>,
    ) -> Self {
        let hidden_ids = db
            .as_ref()
            .and_then(|d| d.lock().ok())
            .and_then(|guard| guard.list_hidden_items().ok())
            .unwrap_or_default();
        let entries = build_corpus(&config, &apps, &[]);
        let mut orchestrator = Self {
            config,
            db,
            apps,
            external_entries: Vec::new(),
            entries: entries.into(),
            hidden_ids,
            frecency_cache: HashMap::new(),
            last_query_prefix: String::new(),
        };
        orchestrator.reload_frecency("");
        orchestrator
    }

    /// Replaces the list of external WASM plugin commands and Script Commands indexed in Root Search.
    pub fn set_external_entries(&mut self, external: Vec<ExternalRootEntry>) {
        self.external_entries = external;
        self.entries = build_corpus(&self.config, &self.apps, &self.external_entries).into();
    }

    /// Re-scans XDG `.desktop` directories and refreshes hidden/frecency state.
    pub fn reload_applications(&mut self) -> usize {
        self.apps = scan_desktop_applications();
        let count = self.apps.len();
        self.entries = build_corpus(&self.config, &self.apps, &self.external_entries).into();
        if let Some(db) = &self.db
            && let Ok(guard) = db.lock()
            && let Ok(hidden) = guard.list_hidden_items()
        {
            self.hidden_ids = hidden;
        }
        self.reload_frecency(&self.last_query_prefix.clone());
        count
    }

    /// Returns total indexed desktop application count (excluding hidden apps).
    #[must_use]
    pub fn visible_app_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| {
                matches!(e.kind, RootEntryKind::Application(_)) && !self.hidden_ids.contains(&e.id)
            })
            .count()
    }

    /// Reloads frecency records from SQLite for the active query prefix.
    pub fn reload_frecency(&mut self, query: &str) {
        self.last_query_prefix = query.trim().to_lowercase();
        let Some(db) = &self.db else {
            return;
        };
        let Ok(guard) = db.lock() else {
            return;
        };
        if let Ok(rows) = guard.load_frecency_records(query) {
            self.frecency_cache = rows
                .into_iter()
                .map(|(id, rec)| {
                    (
                        id,
                        FrecencyRecord {
                            hits: rec.hits,
                            last_used_at_secs: rec.last_used_at,
                            pinned: rec.pinned,
                            query_prefix_hits: rec.query_prefix_hits,
                        },
                    )
                })
                .collect();
        }
    }

    /// Renders the Root Search `ViewNode::List` for `query` and `active_filter` (`"all"`,
    /// `"apps"`, `"commands"`), scored in parallel by `aios-fuzzy` + SQLite frecency.
    #[must_use]
    pub fn build_root_view(&mut self, query: &str, active_filter: &str) -> ViewNode {
        self.build_root_view_with_inline(query, active_filter, &[])
    }

    /// Renders the Root Search `ViewNode::List`, optionally prepending dynamic inline answers
    /// (such as live Calculator expressions) above standard fuzzy matches.
    #[must_use]
    pub fn build_root_view_with_inline(
        &mut self,
        query: &str,
        active_filter: &str,
        inline_results: &[InlineRootResult],
    ) -> ViewNode {
        let trimmed = query.trim();
        let normalized = trimmed.to_lowercase();
        if normalized != self.last_query_prefix {
            self.reload_frecency(trimmed);
        }

        let mut sections: Vec<ListSection> = Vec::new();
        let allow_inline = !trimmed.is_empty()
            && (active_filter.is_empty() || active_filter == "all" || active_filter == "plugins");
        let is_exclusive_inline = allow_inline && inline_results.iter().any(|item| item.exclusive);

        if allow_inline && !inline_results.is_empty() {
            for res in inline_results {
                let item = ListItem {
                    id: res.id.clone(),
                    title: res.title.clone(),
                    subtitle: Some(res.subtitle.clone()),
                    icon: Some(res.badge.clone()),
                    icon_path: res.icon_path.clone(),
                    alias: None,
                    keywords: Vec::new(),
                    filter_group: Some("plugins".to_string()),
                    accessories: vec![Accessory::Tag {
                        label: res.tag_label.clone(),
                        tone: SemanticTone::Accent,
                    }],
                    detail: None,
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new(res.copy_action_id.clone(), res.copy_action_title.clone())
                            .shortcut("Enter"),
                        ActionItem::new(res.open_action_id.clone(), res.open_action_title.clone())
                            .shortcut("Ctrl+Enter"),
                    ]),
                };
                sections.push(ListSection {
                    title: Some(res.section_title.clone()),
                    subtitle: res.section_subtitle.clone(),
                    items: vec![item],
                });
            }
        }

        let now = now_epoch_secs();
        let (visible_entries, snapshot) = if is_exclusive_inline {
            (Vec::new(), None)
        } else {
            let visible_entries: Vec<RootEntry> = self
                .entries
                .iter()
                .filter(|e| !self.hidden_ids.contains(&e.id))
                .filter(|e| {
                    active_filter.is_empty()
                        || active_filter == "all"
                        || e.filter_group == active_filter
                })
                .cloned()
                .collect();

            let search_cfg = SearchConfig {
                limit: if trimmed.is_empty() {
                    visible_entries.len().max(120)
                } else {
                    120
                },
                case_matching: CaseMatching::Smart,
                frecency: FrecencyConfig::default(),
                ..SearchConfig::default()
            };

            let snapshot = search_parallel(
                &visible_entries,
                trimmed,
                &self.frecency_cache,
                now,
                &search_cfg,
                None,
            );
            (visible_entries, snapshot)
        };

        if let Some(snap) = snapshot {
            if trimmed.is_empty() {
                let mut pinned_items = Vec::new();
                let mut frequent_items = Vec::new();
                let mut plugin_items = Vec::new();
                let mut script_items = Vec::new();
                let mut app_items = Vec::new();
                let mut command_items = Vec::new();

                for ranked in &snap.matches {
                    let entry = &visible_entries[ranked.item_index];
                    let frec = self.frecency_cache.get(&entry.id);
                    let is_pinned = frec.is_some_and(|r| r.pinned);
                    let hits = frec.map_or(0, |r| r.hits);
                    let list_item = self.entry_to_list_item(entry, is_pinned, hits);

                    if is_pinned {
                        pinned_items.push(list_item);
                    } else if hits > 0 {
                        frequent_items.push(list_item);
                    } else {
                        match &entry.kind {
                            RootEntryKind::Application(_) => app_items.push(list_item),
                            RootEntryKind::External(ext) => match ext.kind {
                                ExternalRootKind::Plugin => plugin_items.push(list_item),
                                ExternalRootKind::ScriptCommand => script_items.push(list_item),
                            },
                            RootEntryKind::CoreCommand { .. } => command_items.push(list_item),
                        }
                    }
                }

                if !pinned_items.is_empty() {
                    sections.push(ListSection {
                        title: Some("Pinned".to_string()),
                        subtitle: Some(format!("{}", pinned_items.len())),
                        items: pinned_items,
                    });
                }
                if !frequent_items.is_empty() {
                    sections.push(ListSection {
                        title: Some("Recent & Frequent".to_string()),
                        subtitle: Some(format!("{}", frequent_items.len())),
                        items: frequent_items,
                    });
                }
                if !plugin_items.is_empty() {
                    sections.push(ListSection {
                        title: Some("Plugins".to_string()),
                        subtitle: Some(format!("{}", plugin_items.len())),
                        items: plugin_items,
                    });
                }
                if !script_items.is_empty() {
                    sections.push(ListSection {
                        title: Some("Script Commands".to_string()),
                        subtitle: Some(format!("{}", script_items.len())),
                        items: script_items,
                    });
                }
                if !app_items.is_empty() {
                    sections.push(ListSection {
                        title: Some("Applications".to_string()),
                        subtitle: Some(format!("{} installed", app_items.len())),
                        items: app_items,
                    });
                }
                if !command_items.is_empty() {
                    sections.push(ListSection {
                        title: Some("AIOS Core".to_string()),
                        subtitle: None,
                        items: command_items,
                    });
                }
            } else if !snap.matches.is_empty() {
                let items: Vec<ListItem> = snap
                    .matches
                    .iter()
                    .map(|ranked| {
                        let entry = &visible_entries[ranked.item_index];
                        let frec = self.frecency_cache.get(&entry.id);
                        let is_pinned = frec.is_some_and(|r| r.pinned);
                        let hits = frec.map_or(0, |r| r.hits);
                        self.entry_to_list_item(entry, is_pinned, hits)
                    })
                    .collect();

                sections.push(ListSection {
                    title: Some("Results".to_string()),
                    subtitle: Some(format!("{} matched", snap.total_matched)),
                    items,
                });
            }
        }

        // Append Fallback Commands when query is non-empty and direct matches are empty.
        let show_fallbacks = active_filter.is_empty() || active_filter == "all";
        if !trimmed.is_empty()
            && sections.is_empty()
            && show_fallbacks
            && !self.config.fallback_commands.is_empty()
        {
            let fallback_items: Vec<ListItem> = self
                .config
                .fallback_commands
                .iter()
                .map(|fb| ListItem {
                    id: format!("fallback:{}", fb.id),
                    title: format!("{} \"{}\"", fb.title, trimmed),
                    subtitle: Some(fb.template.replace("{query}", trimmed)),
                    icon: Some("FB".to_string()),
                    icon_path: None,
                    alias: None,
                    keywords: Vec::new(),
                    filter_group: None,
                    accessories: vec![Accessory::Tag {
                        label: "Fallback".to_string(),
                        tone: SemanticTone::Neutral,
                    }],
                    detail: None,
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new(
                            format!("fallback.run:{}:{trimmed}", fb.id),
                            format!("Run {}", fb.title),
                        )
                        .shortcut("Enter"),
                    ]),
                })
                .collect();

            sections.push(ListSection {
                title: Some("Fallback Commands".to_string()),
                subtitle: None,
                items: fallback_items,
            });
        }

        let selected_filter = if active_filter.is_empty() {
            "all".to_string()
        } else {
            active_filter.to_owned()
        };

        ViewNode::List(ListView {
            navigation_title: Some("AIOS Root Search".to_string()),
            search_placeholder: Some("Search applications and commands...".to_string()),
            // Orchestrator performs full frecency + fuzzy ranking on every query update.
            filtering: FilterMode::Plugin,
            is_loading: false,
            loading_message: None,
            is_showing_detail: false,
            filter_dropdown: Some(FilterDropdown {
                tooltip: "Filter Root Search".to_string(),
                selected_id: selected_filter,
                options: vec![
                    FilterOption {
                        id: "all".to_string(),
                        title: "All Items".to_string(),
                    },
                    FilterOption {
                        id: "apps".to_string(),
                        title: "Applications".to_string(),
                    },
                    FilterOption {
                        id: "plugins".to_string(),
                        title: "Plugins".to_string(),
                    },
                    FilterOption {
                        id: "scripts".to_string(),
                        title: "Script Commands".to_string(),
                    },
                    FilterOption {
                        id: "commands".to_string(),
                        title: "AIOS Core".to_string(),
                    },
                ],
            }),
            inline_arguments: Vec::new(),
            sections,
            empty_view: Some(EmptyView {
                title: "No applications matched".to_string(),
                description: Some(format!(
                    "No installed application or core command matched '{trimmed}'. Press Esc to clear search."
                )),
                icon: Some("0".to_string()),
                actions: ActionPanel::from_actions(vec![
                    ActionItem::new("search.clear", "Clear Search").shortcut("Enter"),
                    ActionItem::new("root.reload-apps", "Reload Application Index"),
                ]),
            }),
        })
    }

    /// Executes an action emitted from a Root Search item or ActionPanel.
    pub fn execute_action(&mut self, action_id: &str, active_query: &str) -> RootActionOutcome {
        let now = now_epoch_secs();

        if let Some(app_id) = action_id.strip_prefix("app.launch:") {
            return self.launch_by_id(app_id, false, active_query, now);
        }
        if let Some(app_id) = action_id.strip_prefix("app.launch-terminal:") {
            return self.launch_by_id(app_id, true, active_query, now);
        }
        if let Some(app_id) = action_id.strip_prefix("app.copy-exec:")
            && let Some(app) = self.find_app(app_id)
        {
            return RootActionOutcome::CopyToClipboard {
                text: app.exec_raw.clone(),
                hud_title: format!("Copied exec for {}", app.name),
            };
        }
        if let Some(app_id) = action_id.strip_prefix("app.copy-path:")
            && let Some(app) = self.find_app(app_id)
        {
            return RootActionOutcome::CopyToClipboard {
                text: app.desktop_file_path.display().to_string(),
                hud_title: "Copied .desktop Path".to_string(),
            };
        }
        if let Some(app_id) = action_id.strip_prefix("app.show-desktop:")
            && let Some(app) = self.find_app(app_id)
        {
            return RootActionOutcome::PushView {
                frame_id: format!("desktop-detail:{}", app.desktop_id),
                view: Box::new(build_desktop_detail_view(app)),
            };
        }
        if let Some(item_id) = action_id.strip_prefix("app.pin:") {
            if let Some(db) = &self.db
                && let Ok(guard) = db.lock()
            {
                let _ = guard.set_item_pinned(item_id, true, now);
            }
            self.frecency_cache
                .entry(item_id.to_owned())
                .or_default()
                .pinned = true;
            return RootActionOutcome::Refresh {
                toast_title: "Pinned to top of Root Search".to_string(),
            };
        }
        if let Some(item_id) = action_id.strip_prefix("app.unpin:") {
            if let Some(db) = &self.db
                && let Ok(guard) = db.lock()
            {
                let _ = guard.set_item_pinned(item_id, false, now);
            }
            if let Some(rec) = self.frecency_cache.get_mut(item_id) {
                rec.pinned = false;
            }
            return RootActionOutcome::Refresh {
                toast_title: "Unpinned item".to_string(),
            };
        }
        if let Some(item_id) = action_id.strip_prefix("app.hide:") {
            if let Some(db) = &self.db
                && let Ok(guard) = db.lock()
            {
                let _ = guard.set_item_hidden(item_id, true, now);
            }
            self.hidden_ids.insert(item_id.to_owned());
            return RootActionOutcome::Refresh {
                toast_title: "Application hidden from Root Search".to_string(),
            };
        }

        match action_id {
            "root.reload-apps" => {
                let count = self.reload_applications();
                RootActionOutcome::Refresh {
                    toast_title: format!("Indexed {count} desktop applications"),
                }
            }
            "root.unhide-all" => {
                if let Some(db) = &self.db
                    && let Ok(guard) = db.lock()
                {
                    let _ = guard.clear_hidden_items();
                }
                let restored = self.hidden_ids.len();
                self.hidden_ids.clear();
                RootActionOutcome::Refresh {
                    toast_title: format!("Restored {restored} hidden application(s)"),
                }
            }
            "core.cycle-theme" => RootActionOutcome::CycleTheme,
            "core.open-workbench" => RootActionOutcome::OpenWorkbench,
            other => {
                if let Some(rest) = other.strip_prefix("fallback.run:") {
                    let (fb_id, query) = rest.split_once(':').unwrap_or((rest, ""));
                    return self.execute_fallback(fb_id, query);
                }
                RootActionOutcome::Error {
                    title: "Unknown command".to_string(),
                    message: format!("No handler registered for '{other}'."),
                }
            }
        }
    }

    fn launch_by_id(
        &mut self,
        app_id: &str,
        force_terminal: bool,
        active_query: &str,
        now: i64,
    ) -> RootActionOutcome {
        let Some(app) = self.find_app(app_id).cloned() else {
            return RootActionOutcome::Error {
                title: "Application not found".to_string(),
                message: format!("Could not locate '{app_id}' in application index."),
            };
        };

        match launch_desktop_app(&app, force_terminal) {
            Ok(()) => {
                if let Some(db) = &self.db
                    && let Ok(guard) = db.lock()
                {
                    let _ = guard.record_frecency_hit(&app.id, active_query, now);
                }
                let rec = self.frecency_cache.entry(app.id.clone()).or_default();
                rec.hits = rec.hits.saturating_add(1);
                rec.last_used_at_secs = now;
                if !active_query.trim().is_empty() {
                    rec.query_prefix_hits = rec.query_prefix_hits.saturating_add(1);
                }
                RootActionOutcome::Launched {
                    hud_title: format!("Launched {}", app.name),
                }
            }
            Err(err) => RootActionOutcome::Error {
                title: format!("Failed to launch {}", app.name),
                message: err.to_string(),
            },
        }
    }

    fn find_app(&self, app_id: &str) -> Option<&DesktopApp> {
        self.entries.iter().find_map(|entry| {
            if entry.id == app_id
                && let RootEntryKind::Application(app) = &entry.kind
            {
                Some(app.as_ref())
            } else {
                None
            }
        })
    }

    fn execute_fallback(&self, fb_id: &str, query: &str) -> RootActionOutcome {
        let Some(fb) = self.config.fallback_commands.iter().find(|f| f.id == fb_id) else {
            return RootActionOutcome::Error {
                title: "Fallback command not found".to_string(),
                message: format!("No fallback command configured with id '{fb_id}'."),
            };
        };

        let expanded = expand_fallback_template(&fb.template, query);
        if expanded.starts_with("http://")
            || expanded.starts_with("https://")
            || expanded.starts_with("file://")
            || expanded.starts_with("mailto:")
        {
            match open_url_detached(&expanded) {
                Ok(()) => RootActionOutcome::Launched {
                    hud_title: format!("Opened {}", fb.title),
                },
                Err(err) => RootActionOutcome::Error {
                    title: format!("Failed to open {}", fb.title),
                    message: err.to_string(),
                },
            }
        } else if expanded.starts_with("aios://") {
            RootActionOutcome::OpenUri { uri: expanded }
        } else {
            RootActionOutcome::Error {
                title: format!("Unsupported fallback template for {}", fb.title),
                message: format!(
                    "Template '{}' must use http://, https://, file://, or aios://.",
                    fb.template
                ),
            }
        }
    }

    fn entry_to_list_item(&self, entry: &RootEntry, is_pinned: bool, hits: u32) -> ListItem {
        match &entry.kind {
            RootEntryKind::Application(app) => {
                let mut accessories = Vec::new();
                if is_pinned {
                    accessories.push(Accessory::Tag {
                        label: "Pinned".to_string(),
                        tone: SemanticTone::Accent,
                    });
                }
                if let Some(alias) = &entry.alias {
                    accessories.push(Accessory::Text {
                        value: format!("alias: {alias}"),
                    });
                }
                if hits > 0 {
                    accessories.push(Accessory::Text {
                        value: format!("{hits}x"),
                    });
                }
                accessories.push(Accessory::Text {
                    value: app.source.label().to_string(),
                });

                let pin_action = if is_pinned {
                    ActionItem::new(format!("app.unpin:{}", app.id), "Unpin from Top")
                } else {
                    ActionItem::new(format!("app.pin:{}", app.id), "Pin to Top")
                };

                let actions = ActionPanel {
                    sections: vec![
                        ActionSection {
                            title: Some("Launch".to_string()),
                            actions: vec![
                                ActionItem::new(
                                    format!("app.launch:{}", app.id),
                                    format!("Launch {}", app.name),
                                )
                                .shortcut("Enter"),
                                ActionItem::new(
                                    format!("app.launch-terminal:{}", app.id),
                                    "Launch in Terminal",
                                )
                                .shortcut("Ctrl+Enter"),
                            ],
                        },
                        ActionSection {
                            title: Some("Inspect & Organize".to_string()),
                            actions: vec![
                                pin_action,
                                ActionItem::new(
                                    format!("app.show-desktop:{}", app.id),
                                    "Show .desktop Entry",
                                ),
                                ActionItem::new(
                                    format!("app.copy-exec:{}", app.id),
                                    "Copy Exec Command",
                                ),
                                ActionItem::new(
                                    format!("app.copy-path:{}", app.id),
                                    "Copy .desktop Path",
                                ),
                                ActionItem::new(format!("app.hide:{}", app.id), "Hide Application")
                                    .destructive(),
                            ],
                        },
                    ],
                };

                let monogram: String = app
                    .name
                    .split_whitespace()
                    .filter_map(|w| w.chars().next())
                    .take(2)
                    .collect::<String>()
                    .to_uppercase();

                ListItem {
                    id: app.id.clone(),
                    title: app.name.clone(),
                    subtitle: app.generic_name.clone().or_else(|| app.comment.clone()),
                    icon: Some(if monogram.is_empty() {
                        "AP".to_string()
                    } else {
                        monogram
                    }),
                    icon_path: app
                        .icon_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned()),
                    alias: entry.alias.clone(),
                    keywords: app.keywords.clone(),
                    filter_group: Some("apps".to_string()),
                    accessories,
                    detail: None,
                    actions,
                }
            }
            RootEntryKind::CoreCommand {
                command_id,
                subtitle,
                badge,
                keywords,
            } => {
                let mut accessories = vec![Accessory::Tag {
                    label: "Core".to_string(),
                    tone: SemanticTone::Neutral,
                }];
                if let Some(alias) = &entry.alias {
                    accessories.push(Accessory::Text {
                        value: format!("alias: {alias}"),
                    });
                }
                ListItem {
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    subtitle: Some(subtitle.clone()),
                    icon: Some(badge.clone()),
                    icon_path: None,
                    alias: entry.alias.clone(),
                    keywords: keywords.clone(),
                    filter_group: Some("commands".to_string()),
                    accessories,
                    detail: None,
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new(command_id.clone(), format!("Run {}", entry.title))
                            .shortcut("Enter"),
                    ]),
                }
            }
            RootEntryKind::External(ext) => {
                let mut accessories = Vec::new();
                if let Some(summary) = &ext.inline_summary {
                    accessories.push(Accessory::Text {
                        value: summary.clone(),
                    });
                }
                if let Some(alias) = &entry.alias {
                    accessories.push(Accessory::Text {
                        value: format!("alias: {alias}"),
                    });
                }
                let tag_tone = match ext.kind {
                    ExternalRootKind::Plugin => SemanticTone::Accent,
                    ExternalRootKind::ScriptCommand => SemanticTone::Neutral,
                };
                accessories.push(Accessory::Tag {
                    label: ext.tag_label.clone(),
                    tone: tag_tone,
                });

                let primary_label = match ext.kind {
                    ExternalRootKind::Plugin => format!("Open {}", ext.title),
                    ExternalRootKind::ScriptCommand => format!("Run {}", ext.title),
                };
                let mut actions =
                    vec![ActionItem::new(ext.action_id.clone(), primary_label).shortcut("Enter")];
                if let Some((sec_id, sec_title)) = &ext.secondary_action_id {
                    actions.push(
                        ActionItem::new(sec_id.clone(), sec_title.clone()).shortcut("Ctrl+Enter"),
                    );
                }

                ListItem {
                    id: ext.id.clone(),
                    title: ext.title.clone(),
                    subtitle: Some(ext.subtitle.clone()),
                    icon: Some(ext.badge.clone()),
                    icon_path: ext.icon_path.clone(),
                    alias: entry.alias.clone(),
                    keywords: ext.keywords.clone(),
                    filter_group: Some(ext.filter_group.to_string()),
                    accessories,
                    detail: None,
                    actions: ActionPanel::from_actions(actions),
                }
            }
        }
    }
}

fn build_corpus(
    config: &AiosConfig,
    apps: &[DesktopApp],
    external: &[ExternalRootEntry],
) -> Vec<RootEntry> {
    let mut alias_by_target: HashMap<&str, &str> = HashMap::new();
    for (alias, target) in &config.aliases {
        alias_by_target.insert(target.as_str(), alias.as_str());
    }

    let mut entries = Vec::with_capacity(apps.len() + external.len() + 6);

    for app in apps {
        let alias = alias_by_target
            .get(app.id.as_str())
            .or_else(|| alias_by_target.get(app.desktop_id.as_str()))
            .map(|a| (*a).to_owned());

        let subtitle = app.generic_name.clone().or_else(|| app.comment.clone());
        entries.push(RootEntry {
            id: app.id.clone(),
            title: app.name.clone(),
            subtitle,
            alias,
            keywords: app.keywords.clone(),
            filter_group: "apps",
            kind: RootEntryKind::Application(Box::new(app.clone())),
        });
    }

    for ext in external {
        let alias = alias_by_target
            .get(ext.id.as_str())
            .map(|a| (*a).to_owned());
        entries.push(RootEntry {
            id: ext.id.clone(),
            title: ext.title.clone(),
            subtitle: Some(ext.subtitle.clone()),
            alias,
            keywords: ext.keywords.clone(),
            filter_group: ext.filter_group,
            kind: RootEntryKind::External(Box::new(ext.clone())),
        });
    }

    let core_commands = [
        (
            "core:settings",
            "Settings & Plugins",
            "Manage themes, aliases, WASM plugins, preferences, and capabilities",
            "ST",
            "core.open-settings",
            vec![
                "settings",
                "plugins",
                "wasm",
                "extensions",
                "preferences",
                "capabilities",
            ],
        ),
        (
            "core:store",
            "Plugin Store",
            "Discover, install, and update external WASM plugins from registries",
            "PS",
            "core.open-store",
            vec![
                "store",
                "plugins",
                "registry",
                "download",
                "install",
                "extensions",
                "marketplace",
            ],
        ),
        (
            "core:scripts",
            "Script Commands",
            "Browse, run, and reload local scripts in ~/.config/aios/scripts",
            "SH",
            "core.open-scripts",
            vec![
                "scripts",
                "script",
                "commands",
                "shell",
                "bash",
                "automation",
            ],
        ),
        (
            "core:reload-apps",
            "Reload Application Index",
            "Rescan XDG .desktop, Flatpak, and AppImage directories",
            "RL",
            "root.reload-apps",
            vec!["refresh", "rescan", "apps", "desktop"],
        ),
        (
            "core:cycle-theme",
            "Cycle Theme Mode (System / Dark / Light)",
            "Switch Basalt Dark, Warm Alabaster Light, or XDG Portal",
            "TH",
            "core.cycle-theme",
            vec!["theme", "dark", "light", "system", "appearance"],
        ),
        (
            "core:unhide-apps",
            "Restore Hidden Applications",
            "Show all previously hidden desktop entries in Root Search",
            "UH",
            "root.unhide-all",
            vec!["unhide", "restore", "hidden", "apps"],
        ),
        (
            "core:workbench",
            "AIOS View Workbench (Phase 2 Fixtures)",
            "Inspect List+Detail, Grid, Detail+Metadata, Form, and states",
            "WB",
            "core.open-workbench",
            vec!["demo", "workbench", "grid", "form", "detail", "views"],
        ),
    ];

    for (id, title, subtitle, badge, cmd_id, kws) in core_commands {
        let alias = alias_by_target.get(id).map(|a| (*a).to_owned());
        entries.push(RootEntry {
            id: id.to_string(),
            title: title.to_string(),
            subtitle: Some(subtitle.to_string()),
            alias,
            keywords: kws.into_iter().map(ToOwned::to_owned).collect(),
            filter_group: "commands",
            kind: RootEntryKind::CoreCommand {
                command_id: cmd_id.to_string(),
                subtitle: subtitle.to_string(),
                badge: badge.to_string(),
                keywords: Vec::new(),
            },
        });
    }

    entries
}

fn build_desktop_detail_view(app: &DesktopApp) -> ViewNode {
    let raw_contents = fs::read_to_string(&app.desktop_file_path)
        .unwrap_or_else(|err| format!("# Could not read file\n{err}"));
    let truncated: String = raw_contents.lines().take(60).collect::<Vec<_>>().join("\n");

    let markdown = format!(
        "# {}\n\n{}\n\n## Desktop Entry File\n\n```ini\n{}\n```",
        app.name,
        app.comment
            .as_deref()
            .or(app.generic_name.as_deref())
            .unwrap_or("Installed desktop application."),
        truncated
    );

    let mut metadata = vec![
        MetadataEntry::Label {
            title: "Desktop ID".to_string(),
            text: app.desktop_id.clone(),
        },
        MetadataEntry::Label {
            title: "Source".to_string(),
            text: app.source.label().to_string(),
        },
        MetadataEntry::Label {
            title: "Exec".to_string(),
            text: app.exec_raw.clone(),
        },
        MetadataEntry::Label {
            title: "Terminal".to_string(),
            text: if app.terminal { "Yes" } else { "No" }.to_string(),
        },
        MetadataEntry::Separator,
        MetadataEntry::Link {
            title: "File Path".to_string(),
            text: app.desktop_file_path.display().to_string(),
            target: app.desktop_file_path.display().to_string(),
        },
    ];

    if !app.categories.is_empty() {
        metadata.push(MetadataEntry::TagList {
            title: "Categories".to_string(),
            tags: app
                .categories
                .iter()
                .take(6)
                .map(|c| (c.clone(), SemanticTone::Neutral))
                .collect(),
        });
    }

    ViewNode::Detail(DetailView {
        navigation_title: Some(format!("{} (.desktop)", app.name)),
        is_loading: false,
        loading_message: None,
        markdown,
        metadata,
        actions: ActionPanel::from_actions(vec![
            ActionItem::new(
                format!("app.launch:{}", app.id),
                format!("Launch {}", app.name),
            )
            .shortcut("Enter"),
            ActionItem::new(format!("app.copy-path:{}", app.id), "Copy .desktop Path")
                .shortcut("Ctrl+Enter"),
            ActionItem::new(format!("app.copy-exec:{}", app.id), "Copy Exec Command"),
        ]),
    })
}

fn expand_fallback_template(template: &str, query: &str) -> String {
    if template.starts_with("http://") || template.starts_with("https://") {
        template.replace("{query}", &percent_encode_query(query))
    } else {
        template.replace("{query}", query)
    }
}

fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            other => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::AppSource;
    use std::path::PathBuf;

    fn sample_app(id: &str, name: &str, exec: &str) -> DesktopApp {
        DesktopApp {
            id: format!("app:{id}"),
            desktop_id: id.to_string(),
            name: name.to_string(),
            generic_name: Some("Utility".to_string()),
            comment: Some(format!("{name} app")),
            exec_raw: exec.to_string(),
            exec_program: exec.to_string(),
            exec_args: Vec::new(),
            icon_name: None,
            icon_path: None,
            keywords: vec!["tool".to_string()],
            categories: vec!["Utility".to_string()],
            terminal: false,
            working_dir: None,
            desktop_file_path: PathBuf::from(format!("/usr/share/applications/{id}")),
            source: AppSource::System,
        }
    }

    #[test]
    fn frecency_pin_hide_and_alias_ranking_work_end_to_end() {
        let db = Database::open_in_memory().expect("in-memory db");
        let mut cfg = AiosConfig::default();
        cfg.aliases
            .insert("ff".to_string(), "app:firefox.desktop".to_string());

        let apps = vec![
            sample_app("firefox.desktop", "Firefox", "firefox"),
            sample_app("kitty.desktop", "Kitty Terminal", "kitty"),
            sample_app("code.desktop", "Visual Studio Code", "code"),
        ];

        let mut orch = RootSearchOrchestrator::from_apps(cfg, Some(db), apps);
        assert_eq!(orch.visible_app_count(), 3);

        // Pin Kitty and verify it moves into the "Pinned" section on empty query.
        let pin_res = orch.execute_action("app.pin:app:kitty.desktop", "");
        assert!(matches!(pin_res, RootActionOutcome::Refresh { .. }));

        let ViewNode::List(list) = orch.build_root_view("", "all") else {
            panic!("expected list view");
        };
        assert_eq!(list.sections[0].title.as_deref(), Some("Pinned"));
        assert_eq!(list.sections[0].items[0].id, "app:kitty.desktop");

        // Exact alias `ff` ranks Firefox #1.
        let ViewNode::List(alias_list) = orch.build_root_view("ff", "all") else {
            panic!("expected list view");
        };
        assert_eq!(alias_list.sections[0].items[0].id, "app:firefox.desktop");

        // Hide Code and verify visible count drops from 3 to 2.
        let _ = orch.execute_action("app.hide:app:code.desktop", "");
        assert_eq!(orch.visible_app_count(), 2);

        // Restore hidden apps brings count back to 3.
        let _ = orch.execute_action("root.unhide-all", "");
        assert_eq!(orch.visible_app_count(), 3);

        // Non-matching query displays Fallback Commands section.
        let ViewNode::List(fb_list) = orch.build_root_view("zzzzzznotanapp", "all") else {
            panic!("expected list view");
        };
        assert_eq!(
            fb_list.sections[0].title.as_deref(),
            Some("Fallback Commands")
        );
    }

    #[test]
    fn desktop_detail_copy_actions_filter_groups_and_fallback_expansion() {
        let db = Database::open_in_memory().expect("in-memory db");
        let now = now_epoch_secs();
        db.record_frecency_hit("app:kitty.desktop", "ter", now)
            .expect("hit 1");
        db.record_frecency_hit("app:kitty.desktop", "ter", now)
            .expect("hit 2");

        let apps = vec![
            sample_app("foot.desktop", "Foot Terminal", "foot"),
            sample_app("kitty.desktop", "Kitty Terminal", "kitty"),
        ];
        let mut orch = RootSearchOrchestrator::from_apps(AiosConfig::default(), Some(db), apps);

        // Query-prefix affinity ranks Kitty above Foot for "ter".
        let ViewNode::List(list) = orch.build_root_view("ter", "all") else {
            panic!("expected list view");
        };
        assert_eq!(list.sections[0].items[0].id, "app:kitty.desktop");

        // Filter dropdown "commands" returns only AIOS Core commands.
        let ViewNode::List(cmd_list) = orch.build_root_view("", "commands") else {
            panic!("expected list view");
        };
        assert_eq!(cmd_list.sections.len(), 1);
        assert_eq!(cmd_list.sections[0].title.as_deref(), Some("AIOS Core"));

        // Copy exec / copy path / show .desktop detail outcomes.
        assert_eq!(
            orch.execute_action("app.copy-exec:app:kitty.desktop", ""),
            RootActionOutcome::CopyToClipboard {
                text: "kitty".to_string(),
                hud_title: "Copied exec for Kitty Terminal".to_string(),
            }
        );
        assert_eq!(
            orch.execute_action("app.copy-path:app:kitty.desktop", ""),
            RootActionOutcome::CopyToClipboard {
                text: "/usr/share/applications/kitty.desktop".to_string(),
                hud_title: "Copied .desktop Path".to_string(),
            }
        );
        let detail_res = orch.execute_action("app.show-desktop:app:kitty.desktop", "");
        assert!(matches!(
            detail_res,
            RootActionOutcome::PushView { view, .. } if matches!(*view, ViewNode::Detail(_))
        ));

        // Default fallback emits OpenUri with substituted query, and HTTP percent-encodes spaces.
        assert_eq!(
            orch.execute_action("fallback.run:builtin.files:rust wayland", "rust wayland"),
            RootActionOutcome::OpenUri {
                uri: "aios://builtin/files/search?query=rust wayland".to_string(),
            }
        );
        assert_eq!(
            expand_fallback_template("https://docs.rs/releases/search?query={query}", "gpui ce"),
            "https://docs.rs/releases/search?query=gpui%20ce"
        );
    }

    #[test]
    fn plugins_and_script_commands_have_separate_sections_and_filters() {
        let mut orch = RootSearchOrchestrator::from_apps(AiosConfig::default(), None, Vec::new());
        orch.set_external_entries(vec![
            ExternalRootEntry {
                id: "plugin:com.example.demo:open".to_string(),
                kind: ExternalRootKind::Plugin,
                title: "Demo Plugin".to_string(),
                subtitle: "Demo".to_string(),
                badge: "WM".to_string(),
                icon_path: None,
                keywords: vec!["plugin".to_string()],
                filter_group: "plugins",
                action_id: "plugin.run:com.example.demo:open".to_string(),
                secondary_action_id: None,
                tag_label: "WASM Plugin".to_string(),
                inline_summary: None,
            },
            ExternalRootEntry {
                id: "script:uptime".to_string(),
                kind: ExternalRootKind::ScriptCommand,
                title: "System Uptime".to_string(),
                subtitle: "System".to_string(),
                badge: "SH".to_string(),
                icon_path: None,
                keywords: vec!["script".to_string()],
                filter_group: "scripts",
                action_id: "script.run:script:uptime".to_string(),
                secondary_action_id: None,
                tag_label: "Script · inline".to_string(),
                inline_summary: Some("up 2h".to_string()),
            },
        ]);

        let ViewNode::List(all_list) = orch.build_root_view("", "all") else {
            panic!("expected list view");
        };
        let section_titles: Vec<Option<&str>> = all_list
            .sections
            .iter()
            .map(|s| s.title.as_deref())
            .collect();
        assert!(section_titles.contains(&Some("Plugins")));
        assert!(section_titles.contains(&Some("Script Commands")));
        assert!(!section_titles.contains(&Some("Plugins & Scripts")));

        let ViewNode::List(plugins_only) = orch.build_root_view("", "plugins") else {
            panic!("expected list view");
        };
        assert_eq!(plugins_only.sections.len(), 1);
        assert_eq!(plugins_only.sections[0].title.as_deref(), Some("Plugins"));
        assert_eq!(plugins_only.sections[0].items.len(), 1);

        let ViewNode::List(scripts_only) = orch.build_root_view("", "scripts") else {
            panic!("expected list view");
        };
        assert_eq!(scripts_only.sections.len(), 1);
        assert_eq!(
            scripts_only.sections[0].title.as_deref(),
            Some("Script Commands")
        );
        assert_eq!(scripts_only.sections[0].items.len(), 1);

        let ViewNode::List(core_only) = orch.build_root_view("", "commands") else {
            panic!("expected list view");
        };
        assert_eq!(core_only.sections.len(), 1);
        assert_eq!(core_only.sections[0].title.as_deref(), Some("AIOS Core"));
        assert!(
            core_only.sections[0]
                .items
                .iter()
                .any(|i| i.id == "core:scripts")
        );
    }

    #[test]
    fn inline_root_results_rank_first_and_exclusive_hides_other_sections() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let mut orch = RootSearchOrchestrator::from_apps_shared(
            AiosConfig::default(),
            Some(db),
            vec![sample_app("firefox", "Firefox", "firefox")],
        );

        let inline = vec![InlineRootResult {
            id: "inline:calc".to_string(),
            section_title: "Calculator".to_string(),
            section_subtitle: Some("Math".to_string()),
            title: "300".to_string(),
            subtitle: "10*30".to_string(),
            badge: "=".to_string(),
            icon_path: None,
            tag_label: "Math".to_string(),
            copy_action_id: "plugin.inline:com.aios.calculator:copy:300".to_string(),
            copy_action_title: "Copy 300".to_string(),
            open_action_id: "plugin.run-query:com.aios.calculator:calculator:10*30".to_string(),
            open_action_title: "Open Calculator & History".to_string(),
            exclusive: false,
        }];

        let ViewNode::List(non_exclusive) =
            orch.build_root_view_with_inline("10*30", "all", &inline)
        else {
            panic!("expected list view");
        };
        assert_eq!(
            non_exclusive.sections[0].title.as_deref(),
            Some("Calculator")
        );
        assert_eq!(non_exclusive.sections[0].items[0].title, "300");

        let mut exclusive_inline = inline;
        exclusive_inline[0].exclusive = true;
        let ViewNode::List(exclusive_view) =
            orch.build_root_view_with_inline("=10*30", "all", &exclusive_inline)
        else {
            panic!("expected list view");
        };
        assert_eq!(exclusive_view.sections.len(), 1);
        assert_eq!(
            exclusive_view.sections[0].title.as_deref(),
            Some("Calculator")
        );
    }
}
