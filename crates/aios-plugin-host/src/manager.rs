//! WASM Plugin Host managing installed WASM Component plugins, capability grants,
//! preferences, MenuBar state, and Settings & Plugins flows.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aios_core::{AiosConfig, AiosPaths, Database};
use aios_plugin_sdk::{
    ActionItem, ActionPanel, CommandMode, HudMessage, MenuBarState, PluginEvent, PluginResponse,
    ToastMessage, ToastStyle, ViewNode,
};
use aios_plugins::{FirstPartyPlugin, InlinePluginResult, PluginEnv, default_first_party_plugins};

use crate::manifest::{
    InstalledPlugin, PluginOrigin, discover_layered_plugins, install_plugin_from_dir,
    load_plugin_from_dir, uninstall_plugin,
};
use crate::permissions::{HostContext, HostSideEffect, now_epoch_secs};
use crate::registry::{
    PluginRegistryIndex, fetch_registry_index, install_plugin_from_archive_file,
    install_plugin_from_github, install_plugin_from_registry_entry, load_cached_registry_index,
};
use crate::settings::{
    build_install_plugin_form, build_manifest_preferences_form, build_plugin_capabilities_view,
    build_plugin_store_view, build_settings_home_view_with_first_party,
    build_uninstall_confirm_alert,
};
use crate::wasm_runtime::{WasmCallOutput, WasmExecutionLimits, WasmPluginRuntime};

/// Summary entry exported to Root Search for one installed WASM plugin command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPluginEntry {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub icon_path: Option<String>,
    pub keywords: Vec<String>,
    pub filter_group: &'static str,
    pub action_id: String,
    pub secondary_action_id: Option<(String, String)>,
    pub tag_label: String,
}

/// Action outcome emitted by [`PluginHost`] back to the GPUI shell.
#[derive(Debug, Clone)]
pub struct HostActionResult {
    pub response: PluginResponse,
    pub side_effects: Vec<HostSideEffect>,
    pub active_plugin_session: Option<(String, String)>,
}

impl HostActionResult {
    #[must_use]
    pub fn response_only(response: PluginResponse) -> Self {
        Self {
            response,
            side_effects: Vec::new(),
            active_plugin_session: None,
        }
    }
}

/// Top-level WASM plugin host owned by the AIOS daemon / preview shell.
#[derive(Debug)]
pub struct PluginHost {
    config: AiosConfig,
    paths: AiosPaths,
    db: Arc<Mutex<Database>>,
    wasm: WasmPluginRuntime,
    bundled_dirs: Vec<PathBuf>,
    plugins: Vec<InstalledPlugin>,
    disabled_plugins: HashSet<String>,
    first_party: Vec<Box<dyn FirstPartyPlugin>>,
    clipboard: Arc<Mutex<Option<String>>>,
    menu_bar_states: HashMap<String, MenuBarState>,
}

impl PluginHost {
    /// Initializes the plugin host, registering first-party plugins and discovering layered WASM bundles.
    pub fn new(
        config: AiosConfig,
        paths: AiosPaths,
        db: Arc<Mutex<Database>>,
        limits: WasmExecutionLimits,
    ) -> Result<Self, String> {
        let wasm = WasmPluginRuntime::new(limits)?;
        let bundled_dirs = paths.bundled_plugins_dirs();
        let mut host = Self {
            config,
            paths,
            db,
            wasm,
            bundled_dirs,
            plugins: Vec::new(),
            disabled_plugins: HashSet::new(),
            first_party: default_first_party_plugins(),
            clipboard: Arc::new(Mutex::new(None)),
            menu_bar_states: HashMap::new(),
        };
        host.reload_all();
        aios_plugins::calculator::maybe_schedule_stale_currency_refresh(&host.db);
        Ok(host)
    }

    /// Overrides the bundled default plugin search directories (useful for tests & custom packaging).
    #[must_use]
    pub fn with_bundled_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.bundled_dirs = dirs;
        self.reload_all();
        self
    }

    /// Returns a cloned handle to the shared SQLite database for non-blocking host tasks.
    #[must_use]
    pub fn db_arc(&self) -> Arc<Mutex<Database>> {
        Arc::clone(&self.db)
    }

    #[must_use]
    pub fn config(&self) -> &AiosConfig {
        &self.config
    }

    #[must_use]
    pub fn paths(&self) -> &AiosPaths {
        &self.paths
    }

    /// Re-scans bundled and user plugin directories plus persisted enable/disable state.
    pub fn reload_all(&mut self) {
        self.plugins = discover_layered_plugins(&self.bundled_dirs, &self.paths.plugins_dir());
        self.disabled_plugins = self
            .db
            .lock()
            .ok()
            .and_then(|db| db.list_disabled_plugins().ok())
            .unwrap_or_default();
    }

    #[must_use]
    pub fn is_plugin_enabled(&self, plugin_id: &str) -> bool {
        !self.disabled_plugins.contains(plugin_id)
    }

    /// Enables or disables `plugin_id` in SQLite without deleting bundled or user files.
    pub fn set_plugin_enabled(&mut self, plugin_id: &str, enabled: bool) -> Result<(), String> {
        {
            let db = self
                .db
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            db.set_plugin_enabled(plugin_id, enabled, now_epoch_secs())
                .map_err(|e| e.to_string())?;
        }
        if enabled {
            self.disabled_plugins.remove(plugin_id);
        } else {
            self.disabled_plugins.insert(plugin_id.to_owned());
        }
        Ok(())
    }

    /// Toggles `plugin_id` between enabled and disabled, returning the new `enabled` state.
    pub fn toggle_plugin_enabled(&mut self, plugin_id: &str) -> Result<bool, String> {
        let next = !self.is_plugin_enabled(plugin_id);
        self.set_plugin_enabled(plugin_id, next)?;
        Ok(next)
    }

    /// Registers an in-development plugin directory (`aios plugin dev <path>`) without copying it.
    pub fn attach_dev_plugin(&mut self, dev_dir: &Path) -> Result<InstalledPlugin, String> {
        let mut bundle = load_plugin_from_dir(dev_dir).map_err(|e| e.to_string())?;
        bundle.origin = PluginOrigin::Dev;
        self.wasm.invalidate_cache(&bundle.wasm_path);
        self.plugins.retain(|p| p.manifest.id != bundle.manifest.id);
        self.plugins.push(bundle.clone());
        self.plugins
            .sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(bundle)
    }

    /// Syncs the host clipboard cache before invoking a plugin that may read clipboard text.
    pub fn set_clipboard_cache(&self, text: Option<String>) {
        if let Ok(mut guard) = self.clipboard.lock() {
            *guard = text;
        }
    }

    #[must_use]
    pub fn plugins(&self) -> &[InstalledPlugin] {
        &self.plugins
    }

    #[must_use]
    pub fn find_plugin(&self, plugin_id: &str) -> Option<&InstalledPlugin> {
        self.plugins.iter().find(|p| p.manifest.id == plugin_id)
    }

    #[must_use]
    pub fn first_party_plugins(&self) -> &[Box<dyn FirstPartyPlugin>] {
        &self.first_party
    }

    #[must_use]
    pub fn find_first_party(&self, plugin_id: &str) -> Option<&dyn FirstPartyPlugin> {
        self.first_party
            .iter()
            .find(|p| p.manifest().id == plugin_id)
            .map(Box::as_ref)
    }

    /// Resolves `(icon_path, fallback_badge)` for any active external WASM or first-party plugin.
    #[must_use]
    pub fn plugin_icon_for(&self, plugin_id: &str) -> Option<(Option<String>, String)> {
        if let Some(wasm) = self.find_plugin(plugin_id) {
            let icon = wasm
                .icon_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            let badge = wasm
                .manifest
                .name
                .chars()
                .take(2)
                .collect::<String>()
                .to_uppercase();
            return Some((icon, badge));
        }
        if let Some(fp) = self.find_first_party(plugin_id) {
            return Some((fp.icon_asset().map(str::to_owned), fp.badge().to_string()));
        }
        None
    }

    /// Evaluates opt-in inline Root Search providers (external WASM plugins with `[inline_trigger]`
    /// plus built-in fallbacks) gated by sub-microsecond host checks so normal app queries incur
    /// zero WASM startup overhead.
    #[must_use]
    pub fn evaluate_inline_query(&self, query: &str, theme_mode: &str) -> Vec<InlinePluginResult> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        let mut handled_ids = HashSet::new();

        // 1. External WASM plugins (bundled or user-installed) with `[inline_trigger]`.
        for plugin in &self.plugins {
            if !self.is_plugin_enabled(&plugin.manifest.id) {
                continue;
            }
            if !plugin.manifest.inline_trigger.matches_fast_gate(query) {
                continue;
            }
            handled_ids.insert(plugin.manifest.id.clone());
            let ctx = HostContext::new(
                plugin.manifest.clone(),
                Arc::clone(&self.db),
                theme_mode,
                Arc::clone(&self.clipboard),
            );
            let event = PluginEvent::InlineQuery {
                query: query.to_owned(),
            };
            match self.wasm.handle_event(plugin, &event, ctx) {
                Ok(output) => {
                    if let PluginResponse::InlineResult {
                        result: Some(mut res),
                    } = output.response
                    {
                        resolve_inline_icon(&mut res, &plugin.root_dir);
                        results.push(*res);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        plugin_id = %plugin.manifest.id,
                        error = %err,
                        "WASM inline query failed"
                    );
                }
            }
        }

        // 2. Built-in first-party fallbacks (skipped if shadowed by an external WASM plugin or disabled).
        for plugin in &self.first_party {
            let manifest = plugin.manifest();
            if handled_ids.contains(&manifest.id)
                || self.find_plugin(&manifest.id).is_some()
                || !self.is_plugin_enabled(&manifest.id)
            {
                continue;
            }
            let env = PluginEnv::new(&manifest.id, theme_mode, Arc::clone(&self.db));
            if let Some(res) = plugin.evaluate_inline_query(query, &env) {
                results.push(res);
            }
        }
        results
    }

    #[must_use]
    pub fn menu_bar_states(&self) -> &HashMap<String, MenuBarState> {
        &self.menu_bar_states
    }

    /// Installs a local WASM plugin directory into `$XDG_DATA_HOME/aios/plugins/<id>/`.
    pub fn install_local_plugin(&mut self, source_dir: &Path) -> Result<InstalledPlugin, String> {
        let installed = install_plugin_from_dir(source_dir, &self.paths.plugins_dir())
            .map_err(|e| e.to_string())?;
        self.wasm.invalidate_cache(&installed.wasm_path);
        self.reload_all();
        Ok(installed)
    }

    /// Installs a `.aios-plugin` archive file into `$XDG_DATA_HOME/aios/plugins/<id>/`.
    pub fn install_archive_plugin(
        &mut self,
        archive_path: &Path,
        expected_sha256: Option<&str>,
    ) -> Result<InstalledPlugin, String> {
        let installed = install_plugin_from_archive_file(
            archive_path,
            expected_sha256,
            &self.paths.plugins_dir(),
            &self.db,
        )?;
        self.wasm.invalidate_cache(&installed.wasm_path);
        self.reload_all();
        Ok(installed)
    }

    /// Installs a plugin by ID from the configured registry (`index.json`) with mandatory SHA-256 verification.
    pub fn install_registry_plugin(&mut self, plugin_id: &str) -> Result<InstalledPlugin, String> {
        let index = fetch_registry_index(
            &self.config.plugins.registry_urls,
            &self.db,
            self.config.plugins.auto_refresh_hours,
            false,
        )?;
        let entry = index
            .plugins
            .iter()
            .find(|p| p.id == plugin_id)
            .ok_or_else(|| format!("plugin '{plugin_id}' not found in configured registries"))?;
        let installed =
            install_plugin_from_registry_entry(entry, &self.paths.plugins_dir(), &self.db)?;
        self.wasm.invalidate_cache(&installed.wasm_path);
        self.reload_all();
        Ok(installed)
    }

    /// Installs the latest `.aios-plugin` release directly from `github:<owner>/<repo>`.
    pub fn install_github_plugin(&mut self, owner_repo: &str) -> Result<InstalledPlugin, String> {
        let installed =
            install_plugin_from_github(owner_repo, &self.paths.plugins_dir(), &self.db)?;
        self.wasm.invalidate_cache(&installed.wasm_path);
        self.reload_all();
        Ok(installed)
    }

    /// Builds the AIOS Plugin Store view from cached SQLite index (zero blocking I/O).
    #[must_use]
    pub fn build_store_view(&self) -> ViewNode {
        let cached = load_cached_registry_index(&self.db);
        build_plugin_store_view(cached.as_ref(), &self.plugins, &self.disabled_plugins)
    }

    /// Builds the AIOS Plugin Store view using an explicitly provided [`PluginRegistryIndex`].
    #[must_use]
    pub fn build_store_view_with_index(&self, index: Option<&PluginRegistryIndex>) -> ViewNode {
        build_plugin_store_view(index, &self.plugins, &self.disabled_plugins)
    }

    /// Uninstalls an installed plugin by ID and removes its persisted permissions & KV state.
    pub fn uninstall_local_plugin(&mut self, plugin_id: &str) -> Result<bool, String> {
        let existed = {
            let db = self
                .db
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            uninstall_plugin(plugin_id, &self.paths.plugins_dir(), Some(&db))
                .map_err(|e| e.to_string())?
        };
        self.reload_all();
        Ok(existed)
    }

    /// Persists a capability decision (`granted = true | false`) in SQLite `plugin_permissions`.
    pub fn set_capability_grant(
        &self,
        plugin_id: &str,
        capability: &str,
        granted: bool,
    ) -> Result<(), String> {
        let db = self
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.set_plugin_permission(plugin_id, capability, granted, now_epoch_secs())
            .map_err(|e| e.to_string())
    }

    /// Persists preference key-value pairs for `plugin_id` in SQLite.
    pub fn save_plugin_preferences(
        &self,
        plugin_id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let db = self
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let now = now_epoch_secs();
        for (key, val) in values {
            db.set_plugin_preference(plugin_id, key, val, now)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Returns Root Search items for all enabled external WASM and fallback first-party plugins.
    #[must_use]
    pub fn root_search_entries(&self) -> Vec<RootPluginEntry> {
        let mut out = Vec::new();
        let mut seen_plugin_ids = HashSet::new();

        for plugin in &self.plugins {
            let m = &plugin.manifest;
            seen_plugin_ids.insert(m.id.clone());
            if !self.is_plugin_enabled(&m.id) {
                continue;
            }
            let icon_path = plugin
                .icon_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            for cmd in &m.commands {
                let mut keywords = cmd.keywords.clone();
                keywords.push(m.name.clone());
                keywords.push("plugin".to_string());
                keywords.push("wasm".to_string());
                out.push(RootPluginEntry {
                    id: format!("plugin:{}:{}", m.id, cmd.id),
                    title: cmd.title.clone(),
                    subtitle: cmd.subtitle.clone().unwrap_or_else(|| m.name.clone()),
                    badge: "WM".to_string(),
                    icon_path: icon_path.clone(),
                    keywords,
                    filter_group: "plugins",
                    action_id: format!("plugin.run:{}:{}", m.id, cmd.id),
                    secondary_action_id: Some((
                        if m.preferences.is_empty() {
                            format!("settings.plugin-caps:{}", m.id)
                        } else {
                            format!("settings.plugin-prefs:{}", m.id)
                        },
                        if m.preferences.is_empty() {
                            "Manage Capabilities".to_string()
                        } else {
                            "Configure Preferences".to_string()
                        },
                    )),
                    tag_label: match cmd.mode {
                        CommandMode::View => match plugin.origin {
                            PluginOrigin::Bundled => "Plugin".to_string(),
                            PluginOrigin::UserInstalled | PluginOrigin::Dev => {
                                "WASM Plugin".to_string()
                            }
                        },
                        CommandMode::NoView => "WASM Action".to_string(),
                        CommandMode::MenuBar => "MenuBar".to_string(),
                    },
                });
            }
        }

        for plugin in &self.first_party {
            let m = plugin.manifest();
            if seen_plugin_ids.contains(&m.id) || !self.is_plugin_enabled(&m.id) {
                continue;
            }
            for cmd in &m.commands {
                let mut keywords = cmd.keywords.clone();
                keywords.push(m.name.clone());
                keywords.push("plugin".to_string());
                out.push(RootPluginEntry {
                    id: format!("plugin:{}:{}", m.id, cmd.id),
                    title: cmd.title.clone(),
                    subtitle: cmd.subtitle.clone().unwrap_or_else(|| m.name.clone()),
                    badge: plugin.badge().to_string(),
                    icon_path: plugin.icon_asset().map(str::to_owned),
                    keywords,
                    filter_group: "plugins",
                    action_id: format!("plugin.run:{}:{}", m.id, cmd.id),
                    secondary_action_id: Some((
                        format!("settings.plugin-prefs:{}", m.id),
                        "Configure Preferences".to_string(),
                    )),
                    tag_label: "Plugin".to_string(),
                });
            }
        }

        out
    }

    /// Invokes an external WASM or fallback first-party plugin command (`lifecycle::init-command`)
    /// with full crash isolation. External WASM bundles always shadow built-in fallbacks of the same ID.
    pub fn run_wasm_command(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        args_json: &str,
        theme_mode: &str,
    ) -> HostActionResult {
        if let Some(plugin) = self.find_plugin(plugin_id).cloned() {
            let ctx = HostContext::new(
                plugin.manifest.clone(),
                Arc::clone(&self.db),
                theme_mode,
                Arc::clone(&self.clipboard),
            );

            return match self.wasm.init_command(&plugin, command_id, args_json, ctx) {
                Ok(WasmCallOutput {
                    mut response,
                    side_effects,
                }) => {
                    resolve_plugin_response_icons(&mut response, &plugin.root_dir);
                    if let PluginResponse::MenuBar { ref state } = response {
                        self.menu_bar_states
                            .insert(format!("{plugin_id}:{command_id}"), state.clone());
                    }
                    let response = match response {
                        PluginResponse::Render { view } => PluginResponse::Push {
                            frame_id: Some(format!("plugin:{plugin_id}:{command_id}")),
                            view,
                        },
                        other => other,
                    };
                    let session = match &response {
                        PluginResponse::Render { .. } | PluginResponse::Push { .. } => {
                            Some((plugin_id.to_owned(), command_id.to_owned()))
                        }
                        _ => None,
                    };
                    HostActionResult {
                        response,
                        side_effects,
                        active_plugin_session: session,
                    }
                }
                Err(err) => {
                    tracing::error!(
                        plugin_id = %plugin.manifest.id,
                        command_id = %command_id,
                        error = %err,
                        "isolated WASM plugin failure"
                    );
                    let error_view =
                        WasmPluginRuntime::isolated_error_view(&plugin, command_id, &err);
                    HostActionResult {
                        response: PluginResponse::push(
                            Some(format!("plugin-error:{plugin_id}:{command_id}")),
                            error_view,
                        ),
                        side_effects: Vec::new(),
                        active_plugin_session: None,
                    }
                }
            };
        }

        if let Some(fp) = self.find_first_party(plugin_id) {
            let args: Vec<String> = serde_json::from_str(args_json).unwrap_or_default();
            let env = PluginEnv::new(plugin_id, theme_mode, Arc::clone(&self.db));
            let raw_resp = fp.init_command(command_id, &args, &env);
            let response = match raw_resp {
                PluginResponse::Render { view } => PluginResponse::Push {
                    frame_id: Some(format!("plugin:{plugin_id}:{command_id}")),
                    view,
                },
                other => other,
            };
            let session = match &response {
                PluginResponse::Render { .. } | PluginResponse::Push { .. } => {
                    Some((plugin_id.to_owned(), command_id.to_owned()))
                }
                _ => None,
            };
            return HostActionResult {
                response,
                side_effects: Vec::new(),
                active_plugin_session: session,
            };
        }

        HostActionResult::response_only(PluginResponse::ShowToast {
            toast: ToastMessage {
                style: ToastStyle::Failure,
                title: format!("Plugin '{plugin_id}' not found"),
                message: Some("Reload plugins in Settings & Extensions.".to_string()),
            },
        })
    }

    /// Delivers a [`PluginEvent`] to an active external WASM or fallback first-party plugin view.
    pub fn send_wasm_event(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        event: &PluginEvent,
        theme_mode: &str,
    ) -> HostActionResult {
        if let Some(plugin) = self.find_plugin(plugin_id).cloned() {
            let ctx = HostContext::new(
                plugin.manifest.clone(),
                Arc::clone(&self.db),
                theme_mode,
                Arc::clone(&self.clipboard),
            );

            return match self.wasm.handle_event(&plugin, event, ctx) {
                Ok(WasmCallOutput {
                    mut response,
                    side_effects,
                }) => {
                    resolve_plugin_response_icons(&mut response, &plugin.root_dir);
                    if let PluginResponse::MenuBar { ref state } = response {
                        self.menu_bar_states
                            .insert(format!("{plugin_id}:{command_id}"), state.clone());
                    }
                    HostActionResult {
                        response,
                        side_effects,
                        active_plugin_session: Some((plugin_id.to_owned(), command_id.to_owned())),
                    }
                }
                Err(err) => {
                    tracing::error!(
                        plugin_id = %plugin.manifest.id,
                        command_id = %command_id,
                        error = %err,
                        "isolated WASM plugin event failure"
                    );
                    let error_view =
                        WasmPluginRuntime::isolated_error_view(&plugin, command_id, &err);
                    HostActionResult {
                        response: PluginResponse::render(error_view),
                        side_effects: Vec::new(),
                        active_plugin_session: None,
                    }
                }
            };
        }

        if let Some(fp) = self.find_first_party(plugin_id) {
            let env = PluginEnv::new(plugin_id, theme_mode, Arc::clone(&self.db));
            let response = fp.handle_event(command_id, event, &env);
            return HostActionResult {
                response,
                side_effects: Vec::new(),
                active_plugin_session: Some((plugin_id.to_owned(), command_id.to_owned())),
            };
        }

        HostActionResult::response_only(PluginResponse::ShowToast {
            toast: ToastMessage {
                style: ToastStyle::Failure,
                title: format!("Plugin '{plugin_id}' is no longer loaded"),
                message: None,
            },
        })
    }

    /// Triggers background refresh for a `MenuBar` or background plugin command.
    pub fn run_background_refresh(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        theme_mode: &str,
    ) -> Result<WasmCallOutput, String> {
        let plugin = self
            .find_plugin(plugin_id)
            .cloned()
            .ok_or_else(|| format!("plugin '{plugin_id}' not found"))?;
        let ctx = HostContext::new(
            plugin.manifest.clone(),
            Arc::clone(&self.db),
            theme_mode,
            Arc::clone(&self.clipboard),
        );
        let output = self.wasm.background_refresh(&plugin, command_id, ctx)?;
        if let PluginResponse::MenuBar { ref state } = output.response {
            self.menu_bar_states
                .insert(format!("{plugin_id}:{command_id}"), state.clone());
        }
        Ok(output)
    }

    /// Builds the main Settings & Plugins `ViewNode`.
    #[must_use]
    pub fn build_settings_view(&self) -> ViewNode {
        let perms = self.collect_all_permissions();
        let fp_entries: Vec<_> = self
            .first_party
            .iter()
            .filter(|p| self.find_plugin(&p.manifest().id).is_none())
            .map(|p| (p.manifest(), p.badge()))
            .collect();
        build_settings_home_view_with_first_party(
            &self.config,
            &self.paths,
            &fp_entries,
            &self.plugins,
            &perms,
            &self.disabled_plugins,
        )
    }

    /// Handles Settings & Plugins and WASM Plugin actions emitted by the UI shell.
    pub fn dispatch_host_action(
        &mut self,
        action_id: &str,
        inline_args: &[String],
        form_values: Option<&BTreeMap<String, String>>,
        theme_mode: &str,
    ) -> Option<HostActionResult> {
        if let Some(rest) = action_id.strip_prefix("plugin.inline:") {
            let (plugin_id, inner_action) = rest.split_once(':')?;
            if let Some(plugin) = self.find_plugin(plugin_id).cloned() {
                let ctx = HostContext::new(
                    plugin.manifest.clone(),
                    Arc::clone(&self.db),
                    theme_mode,
                    Arc::clone(&self.clipboard),
                );
                let event = PluginEvent::InlineAction {
                    action_id: inner_action.to_owned(),
                };
                if let Ok(WasmCallOutput {
                    response,
                    mut side_effects,
                }) = self.wasm.handle_event(&plugin, &event, ctx)
                {
                    if matches!(response, PluginResponse::CopyToClipboard { .. })
                        && !side_effects
                            .iter()
                            .any(|e| matches!(e, HostSideEffect::CloseMainWindow))
                    {
                        side_effects.push(HostSideEffect::CloseMainWindow);
                    }
                    return Some(HostActionResult {
                        response,
                        side_effects,
                        active_plugin_session: None,
                    });
                }
                return None;
            }
            let fp = self.find_first_party(plugin_id)?;
            let env = PluginEnv::new(plugin_id, theme_mode, Arc::clone(&self.db));
            let response = fp.handle_inline_action(inner_action, &env)?;
            let mut side_effects = Vec::new();
            if matches!(response, PluginResponse::CopyToClipboard { .. }) {
                side_effects.push(HostSideEffect::CloseMainWindow);
            }
            return Some(HostActionResult {
                response,
                side_effects,
                active_plugin_session: None,
            });
        }

        if let Some(rest) = action_id.strip_prefix("plugin.run-query:") {
            let (plugin_id, after_plugin) = rest.split_once(':')?;
            let (cmd_id, raw_query) = after_plugin.split_once(':')?;
            let args_json =
                serde_json::to_string(&[raw_query]).unwrap_or_else(|_| "[]".to_string());
            return Some(self.run_wasm_command(plugin_id, cmd_id, &args_json, theme_mode));
        }

        if let Some(rest) = action_id.strip_prefix("plugin.run:") {
            let (plugin_id, cmd_id) = rest.split_once(':')?;
            let args_json = serde_json::to_string(inline_args).unwrap_or_else(|_| "[]".to_string());
            return Some(self.run_wasm_command(plugin_id, cmd_id, &args_json, theme_mode));
        }

        if let Some(plugin_id) = action_id.strip_prefix("plugin.reload:") {
            if let Some(plugin) = self.find_plugin(plugin_id).cloned() {
                self.wasm.invalidate_cache(&plugin.wasm_path);
            }
            self.reload_all();
            return Some(HostActionResult::response_only(PluginResponse::ShowToast {
                toast: ToastMessage {
                    style: ToastStyle::Success,
                    title: format!("Reloaded plugin '{plugin_id}'"),
                    message: None,
                },
            }));
        }

        if action_id == "settings.open" || action_id == "core.open-settings" {
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some("settings.home".to_string()),
                self.build_settings_view(),
            )));
        }

        if action_id == "settings.open-store" || action_id == "core.open-store" {
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some("settings.store".to_string()),
                self.build_store_view(),
            )));
        }

        if let Some(plugin_id) = action_id.strip_prefix("settings.plugin-toggle:") {
            return match self.toggle_plugin_enabled(plugin_id) {
                Ok(enabled) => Some(HostActionResult {
                    response: PluginResponse::render(self.build_settings_view()),
                    side_effects: vec![HostSideEffect::ShowToast(ToastMessage {
                        style: ToastStyle::Success,
                        title: if enabled {
                            format!("Enabled plugin '{plugin_id}'")
                        } else {
                            format!("Disabled plugin '{plugin_id}'")
                        },
                        message: None,
                    })],
                    active_plugin_session: None,
                }),
                Err(err) => Some(HostActionResult::response_only(PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: "Failed to toggle plugin state".to_string(),
                        message: Some(err),
                    },
                })),
            };
        }

        if action_id == "settings.reload-plugins" || action_id == "settings.reload-all" {
            self.reload_all();
            return Some(HostActionResult {
                response: PluginResponse::render(self.build_settings_view()),
                side_effects: vec![HostSideEffect::ShowToast(ToastMessage {
                    style: ToastStyle::Success,
                    title: format!("Loaded {} WASM plugin(s)", self.plugins.len()),
                    message: None,
                })],
                active_plugin_session: None,
            });
        }

        if action_id == "settings.copy-config-path" {
            return Some(HostActionResult::response_only(
                PluginResponse::CopyToClipboard {
                    text: self.paths.config_file().display().to_string(),
                    hud_title: Some("Copied config.toml Path".to_string()),
                },
            ));
        }

        if action_id == "settings.install-plugin-form" {
            let hint = self.paths.plugins_dir().display().to_string();
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some("settings.install".to_string()),
                build_install_plugin_form(&hint),
            )));
        }

        if action_id == "settings.install-plugin-submit" {
            let source_path = form_values
                .and_then(|v| v.get("source_path"))
                .map(|s| s.trim())
                .unwrap_or("");
            if source_path.is_empty() {
                return Some(HostActionResult::response_only(PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: "Plugin directory path is required".to_string(),
                        message: None,
                    },
                }));
            }
            return Some(match self.install_local_plugin(Path::new(source_path)) {
                Ok(installed) => HostActionResult {
                    response: PluginResponse::Pop,
                    side_effects: vec![
                        HostSideEffect::ShowHud(HudMessage {
                            title: format!("Installed {}", installed.manifest.name),
                        }),
                        HostSideEffect::ShowToast(ToastMessage {
                            style: ToastStyle::Success,
                            title: format!("Installed {}", installed.manifest.name),
                            message: Some(format!("Plugin ID: {}", installed.manifest.id)),
                        }),
                    ],
                    active_plugin_session: None,
                },
                Err(err) => HostActionResult::response_only(PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: "Plugin installation failed".to_string(),
                        message: Some(err),
                    },
                }),
            });
        }

        if let Some(plugin_id) = action_id.strip_prefix("settings.plugin-caps:") {
            let plugin = self.find_plugin(plugin_id)?;
            let grants = self
                .db
                .lock()
                .ok()
                .and_then(|db| db.list_plugin_permissions(plugin_id).ok())
                .unwrap_or_default();
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some(format!("settings.caps:{plugin_id}")),
                build_plugin_capabilities_view(plugin, &grants),
            )));
        }

        if let Some(rest) = action_id.strip_prefix("settings.cap-set:") {
            // Format: `<plugin_id>:<cap_key>:<grant|deny>` (cap_key itself may contain `:`, e.g. `http:https://...`).
            let (before_mode, mode) = rest.rsplit_once(':')?;
            let (plugin_id, cap_key) = before_mode.split_once(':')?;
            let granted = mode == "grant";
            if let Err(err) = self.set_capability_grant(plugin_id, cap_key, granted) {
                return Some(HostActionResult::response_only(PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: "Failed to update capability".to_string(),
                        message: Some(err),
                    },
                }));
            }
            let plugin = self.find_plugin(plugin_id)?;
            let grants = self
                .db
                .lock()
                .ok()
                .and_then(|db| db.list_plugin_permissions(plugin_id).ok())
                .unwrap_or_default();
            return Some(HostActionResult {
                response: PluginResponse::render(build_plugin_capabilities_view(plugin, &grants)),
                side_effects: vec![HostSideEffect::ShowToast(ToastMessage {
                    style: ToastStyle::Success,
                    title: format!(
                        "{} '{}' for {}",
                        if granted { "Approved" } else { "Revoked" },
                        cap_key,
                        plugin.manifest.name
                    ),
                    message: None,
                })],
                active_plugin_session: None,
            });
        }

        if let Some(plugin_id) = action_id.strip_prefix("settings.plugin-prefs:") {
            let manifest = self
                .find_plugin(plugin_id)
                .map(|p| p.manifest.clone())
                .or_else(|| self.find_first_party(plugin_id).map(|p| p.manifest()))?;
            let mut values = HashMap::new();
            if let Ok(db) = self.db.lock() {
                for pref in &manifest.preferences {
                    if let Ok(Some(v)) = db.get_plugin_preference(plugin_id, &pref.name) {
                        values.insert(pref.name.clone(), v);
                    }
                }
            }
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some(format!("settings.prefs:{plugin_id}")),
                build_manifest_preferences_form(&manifest, &values),
            )));
        }

        if let Some(plugin_id) = action_id.strip_prefix("settings.plugin-prefs-save:") {
            let empty_map = BTreeMap::new();
            let vals = form_values.unwrap_or(&empty_map);
            return Some(match self.save_plugin_preferences(plugin_id, vals) {
                Ok(()) => HostActionResult {
                    response: PluginResponse::Pop,
                    side_effects: vec![HostSideEffect::ShowToast(ToastMessage {
                        style: ToastStyle::Success,
                        title: "Saved plugin preferences".to_string(),
                        message: None,
                    })],
                    active_plugin_session: None,
                },
                Err(err) => HostActionResult::response_only(PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: "Failed to save preferences".to_string(),
                        message: Some(err),
                    },
                }),
            });
        }

        if let Some(plugin_id) = action_id.strip_prefix("settings.plugin-uninstall-prompt:") {
            let plugin = self.find_plugin(plugin_id)?;
            return Some(HostActionResult::response_only(PluginResponse::Confirm {
                alert: build_uninstall_confirm_alert(plugin),
            }));
        }

        if let Some(plugin_id) = action_id.strip_prefix("settings.plugin-uninstall-confirm:") {
            return Some(match self.uninstall_local_plugin(plugin_id) {
                Ok(_) => HostActionResult {
                    response: PluginResponse::render(self.build_settings_view()),
                    side_effects: vec![HostSideEffect::ShowToast(ToastMessage {
                        style: ToastStyle::Success,
                        title: format!("Uninstalled plugin '{plugin_id}'"),
                        message: None,
                    })],
                    active_plugin_session: None,
                },
                Err(err) => HostActionResult::response_only(PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: "Uninstall failed".to_string(),
                        message: Some(err),
                    },
                }),
            });
        }

        None
    }

    fn collect_all_permissions(&self) -> HashMap<String, HashMap<String, bool>> {
        let mut all = HashMap::new();
        if let Ok(db) = self.db.lock() {
            for plugin in &self.plugins {
                if let Ok(perms) = db.list_plugin_permissions(&plugin.manifest.id) {
                    all.insert(plugin.manifest.id.clone(), perms);
                }
            }
        }
        all
    }
}

/// Helper to build a default Recovery ActionPanel for plugin UI screens.
#[must_use]
pub fn default_plugin_recovery_actions() -> ActionPanel {
    ActionPanel::from_actions(vec![
        ActionItem::new("settings.open", "Open Settings & Plugins").shortcut("Enter"),
        ActionItem::new("nav.pop", "Return").shortcut("Esc"),
    ])
}

fn resolve_relative_icon(path: &mut Option<String>, plugin_root: &Path) {
    let Some(raw) = path.as_deref() else {
        return;
    };
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.starts_with("asset://")
        || raw.starts_with("http://")
        || raw.starts_with("https://")
    {
        return;
    }
    let candidate = plugin_root.join(raw);
    if candidate.is_file() {
        *path = Some(candidate.to_string_lossy().into_owned());
    }
}

fn resolve_inline_icon(res: &mut InlinePluginResult, plugin_root: &Path) {
    resolve_relative_icon(&mut res.icon_path, plugin_root);
}

fn resolve_view_icons(view: &mut ViewNode, plugin_root: &Path) {
    if let ViewNode::List(list) = view {
        for section in &mut list.sections {
            for item in &mut section.items {
                resolve_relative_icon(&mut item.icon_path, plugin_root);
            }
        }
    }
}

fn resolve_plugin_response_icons(response: &mut PluginResponse, plugin_root: &Path) {
    match response {
        PluginResponse::Render { view } | PluginResponse::Push { view, .. } => {
            resolve_view_icons(view, plugin_root);
        }
        PluginResponse::InlineResult { result: Some(res) } => {
            resolve_inline_icon(res, plugin_root);
        }
        _ => {}
    }
}
