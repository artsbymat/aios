//! Unified Hybrid Plugin Host managing installed WASM plugins, Script Commands,
//! capability grants, preferences, inline script refreshes, and Settings & Extension Manager flows.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aios_core::{AiosConfig, AiosPaths, Database};
use aios_plugin_sdk::{
    ActionItem, ActionPanel, CommandMode, ConfirmAlert, HudMessage, MenuBarState, PluginEvent,
    PluginResponse, ToastMessage, ToastStyle, ViewNode,
};

use crate::manifest::{
    InstalledPlugin, discover_plugins, install_plugin_from_dir, load_plugin_from_dir,
    uninstall_plugin,
};
use crate::permissions::{HostContext, HostSideEffect, now_epoch_secs};
use crate::scripts::{ScriptCommand, ScriptMode, discover_script_commands, execute_script_command};
use crate::settings::{
    build_install_plugin_form, build_plugin_capabilities_view, build_plugin_preferences_form,
    build_settings_home_view, build_uninstall_confirm_alert,
};
use crate::wasm_runtime::{WasmCallOutput, WasmExecutionLimits, WasmPluginRuntime};

/// Summary entry exported to Root Search so installed WASM plugins and Script Commands
/// appear seamlessly alongside desktop applications.
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
    pub inline_summary: Option<String>,
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

/// Top-level hybrid plugin host owned by the AIOS daemon / preview shell.
#[derive(Debug)]
pub struct PluginHost {
    config: AiosConfig,
    paths: AiosPaths,
    db: Arc<Mutex<Database>>,
    wasm: WasmPluginRuntime,
    plugins: Vec<InstalledPlugin>,
    scripts: Vec<ScriptCommand>,
    clipboard: Arc<Mutex<Option<String>>>,
    last_script_outputs: HashMap<String, String>,
    inline_script_summaries: HashMap<String, String>,
    menu_bar_states: HashMap<String, MenuBarState>,
    script_timeout: Duration,
}

impl PluginHost {
    /// Initializes the hybrid plugin host, discovering installed WASM bundles and Script Commands.
    pub fn new(
        config: AiosConfig,
        paths: AiosPaths,
        db: Arc<Mutex<Database>>,
        limits: WasmExecutionLimits,
    ) -> Result<Self, String> {
        let wasm = WasmPluginRuntime::new(limits)?;
        let mut host = Self {
            config,
            paths,
            db,
            wasm,
            plugins: Vec::new(),
            scripts: Vec::new(),
            clipboard: Arc::new(Mutex::new(None)),
            last_script_outputs: HashMap::new(),
            inline_script_summaries: HashMap::new(),
            menu_bar_states: HashMap::new(),
            script_timeout: Duration::from_secs(15),
        };
        host.reload_all();
        Ok(host)
    }

    /// Re-scans `$XDG_DATA_HOME/aios/plugins/` and `$XDG_CONFIG_HOME/aios/scripts/`.
    pub fn reload_all(&mut self) {
        self.plugins = discover_plugins(&self.paths.plugins_dir());
        self.scripts = discover_script_commands(&self.paths.scripts_dir());
        self.refresh_inline_scripts();
    }

    /// Registers an in-development plugin directory (`aios plugin dev <path>`) without copying it.
    pub fn attach_dev_plugin(&mut self, dev_dir: &Path) -> Result<InstalledPlugin, String> {
        let bundle = load_plugin_from_dir(dev_dir).map_err(|e| e.to_string())?;
        self.wasm.invalidate_cache(&bundle.wasm_path);
        self.plugins.retain(|p| p.manifest.id != bundle.manifest.id);
        self.plugins.push(bundle.clone());
        self.plugins
            .sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(bundle)
    }

    pub fn set_script_timeout(&mut self, timeout: Duration) {
        self.script_timeout = timeout;
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
    pub fn scripts(&self) -> &[ScriptCommand] {
        &self.scripts
    }

    #[must_use]
    pub fn find_plugin(&self, plugin_id: &str) -> Option<&InstalledPlugin> {
        self.plugins.iter().find(|p| p.manifest.id == plugin_id)
    }

    #[must_use]
    pub fn find_script(&self, script_id: &str) -> Option<&ScriptCommand> {
        self.scripts.iter().find(|s| s.id == script_id)
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

    /// Runs all `ScriptMode::Inline` scripts once to populate Root Search inline summaries.
    pub fn refresh_inline_scripts(&mut self) {
        let inline_scripts: Vec<ScriptCommand> = self
            .scripts
            .iter()
            .filter(|s| s.mode == ScriptMode::Inline && !s.needs_confirmation)
            .cloned()
            .collect();
        for sc in inline_scripts {
            let (outcome, _) = execute_script_command(&sc, &[], Duration::from_secs(3));
            if let Some(line) = outcome.last_non_empty_line() {
                self.inline_script_summaries
                    .insert(sc.id.clone(), line.to_owned());
            }
        }
    }

    /// Returns Root Search items for all installed WASM plugin commands and Script Commands.
    #[must_use]
    pub fn root_search_entries(&self) -> Vec<RootPluginEntry> {
        let mut out = Vec::new();
        for plugin in &self.plugins {
            let m = &plugin.manifest;
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
                    filter_group: "commands",
                    action_id: format!("plugin.run:{}:{}", m.id, cmd.id),
                    secondary_action_id: Some((
                        format!("settings.plugin-caps:{}", m.id),
                        "Manage Capabilities".to_string(),
                    )),
                    tag_label: match cmd.mode {
                        CommandMode::View => "WASM Plugin".to_string(),
                        CommandMode::NoView => "WASM Action".to_string(),
                        CommandMode::MenuBar => "MenuBar".to_string(),
                    },
                    inline_summary: None,
                });
            }
        }

        for sc in &self.scripts {
            let mut keywords = vec!["script".to_string(), sc.mode.label().to_string()];
            if let Some(pkg) = &sc.package_name {
                keywords.push(pkg.clone());
            }
            out.push(RootPluginEntry {
                id: sc.id.clone(),
                title: sc.title.clone(),
                subtitle: sc
                    .description
                    .clone()
                    .or_else(|| sc.package_name.clone())
                    .unwrap_or_else(|| "Script Command".to_string()),
                badge: sc.icon.clone().unwrap_or_else(|| "SH".to_string()),
                icon_path: None,
                keywords,
                filter_group: "commands",
                action_id: format!("script.run:{}", sc.id),
                secondary_action_id: Some((
                    format!("script.copy-path:{}", sc.id),
                    "Copy Script Path".to_string(),
                )),
                tag_label: format!("Script · {}", sc.mode.label()),
                inline_summary: self.inline_script_summaries.get(&sc.id).cloned(),
            });
        }

        out
    }

    /// Invokes a WASM plugin command (`lifecycle::init-command`) with full crash isolation.
    pub fn run_wasm_command(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        args_json: &str,
        theme_mode: &str,
    ) -> HostActionResult {
        let Some(plugin) = self.find_plugin(plugin_id).cloned() else {
            return HostActionResult::response_only(PluginResponse::ShowToast {
                toast: ToastMessage {
                    style: ToastStyle::Failure,
                    title: format!("Plugin '{plugin_id}' not found"),
                    message: Some("Reload plugins in Settings & Extensions.".to_string()),
                },
            });
        };

        let ctx = HostContext::new(
            plugin.manifest.clone(),
            Arc::clone(&self.db),
            theme_mode,
            Arc::clone(&self.clipboard),
        );

        match self.wasm.init_command(&plugin, command_id, args_json, ctx) {
            Ok(WasmCallOutput {
                response,
                side_effects,
            }) => {
                if let PluginResponse::MenuBar { ref state } = response {
                    self.menu_bar_states
                        .insert(format!("{plugin_id}:{command_id}"), state.clone());
                }
                // Opening a plugin command from Root Search or Settings must always push a new
                // navigation frame rather than overwriting the caller's frame in place.
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
                let error_view = WasmPluginRuntime::isolated_error_view(&plugin, command_id, &err);
                HostActionResult {
                    response: PluginResponse::push(
                        Some(format!("plugin-error:{plugin_id}:{command_id}")),
                        error_view,
                    ),
                    side_effects: Vec::new(),
                    active_plugin_session: None,
                }
            }
        }
    }

    /// Delivers a [`PluginEvent`] to an active WASM plugin view with full crash isolation.
    pub fn send_wasm_event(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        event: &PluginEvent,
        theme_mode: &str,
    ) -> HostActionResult {
        let Some(plugin) = self.find_plugin(plugin_id).cloned() else {
            return HostActionResult::response_only(PluginResponse::ShowToast {
                toast: ToastMessage {
                    style: ToastStyle::Failure,
                    title: format!("Plugin '{plugin_id}' is no longer loaded"),
                    message: None,
                },
            });
        };

        let ctx = HostContext::new(
            plugin.manifest.clone(),
            Arc::clone(&self.db),
            theme_mode,
            Arc::clone(&self.clipboard),
        );

        match self.wasm.handle_event(&plugin, event, ctx) {
            Ok(WasmCallOutput {
                response,
                side_effects,
            }) => {
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
                let error_view = WasmPluginRuntime::isolated_error_view(&plugin, command_id, &err);
                HostActionResult {
                    response: PluginResponse::render(error_view),
                    side_effects: Vec::new(),
                    active_plugin_session: None,
                }
            }
        }
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

    /// Runs a Script Command by ID, presenting a `ConfirmAlert` first when `needs_confirmation` is true.
    pub fn run_script(
        &mut self,
        script_id: &str,
        args: &[String],
        confirmed: bool,
    ) -> HostActionResult {
        let Some(script) = self.find_script(script_id).cloned() else {
            return HostActionResult::response_only(PluginResponse::ShowToast {
                toast: ToastMessage {
                    style: ToastStyle::Failure,
                    title: format!("Script '{script_id}' not found"),
                    message: None,
                },
            });
        };

        if script.needs_confirmation && !confirmed {
            let encoded_args = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
            let alert: ConfirmAlert = script.confirmation_alert(&encoded_args);
            return HostActionResult::response_only(PluginResponse::Confirm { alert });
        }

        let (outcome, response) = execute_script_command(&script, args, self.script_timeout);
        let combined_output = if outcome.stderr.trim().is_empty() {
            outcome.stdout.clone()
        } else if outcome.stdout.trim().is_empty() {
            outcome.stderr.clone()
        } else {
            format!(
                "{}\n{}",
                outcome.stdout.trim_end(),
                outcome.stderr.trim_end()
            )
        };
        self.last_script_outputs
            .insert(script.id.clone(), combined_output);
        if script.mode == ScriptMode::Inline
            && let Some(line) = outcome.last_non_empty_line()
        {
            self.inline_script_summaries
                .insert(script.id.clone(), line.to_owned());
        }

        HostActionResult::response_only(response)
    }

    /// Builds the main Settings & Extension Manager `ViewNode`.
    #[must_use]
    pub fn build_settings_view(&self) -> ViewNode {
        let perms = self.collect_all_permissions();
        build_settings_home_view(
            &self.config,
            &self.paths,
            &self.plugins,
            &self.scripts,
            &perms,
        )
    }

    /// Handles Settings & Extension Manager, Plugin, and Script actions emitted by the UI shell.
    pub fn dispatch_host_action(
        &mut self,
        action_id: &str,
        inline_args: &[String],
        form_values: Option<&BTreeMap<String, String>>,
        theme_mode: &str,
    ) -> Option<HostActionResult> {
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

        if let Some(script_id) = action_id.strip_prefix("script.run:") {
            return Some(self.run_script(script_id, inline_args, false));
        }

        if let Some(rest) = action_id.strip_prefix("script.confirm:") {
            // Format: `script.confirm:script:<stem>:<json-args>`
            let (script_id, args_json) = split_script_confirm(rest);
            let parsed_args: Vec<String> = serde_json::from_str(args_json).unwrap_or_default();
            return Some(self.run_script(script_id, &parsed_args, true));
        }

        if let Some(script_id) = action_id.strip_prefix("script.copy-output:") {
            let text = self
                .last_script_outputs
                .get(script_id)
                .cloned()
                .unwrap_or_default();
            return Some(HostActionResult::response_only(
                PluginResponse::CopyToClipboard {
                    text,
                    hud_title: Some("Copied Script Output".to_string()),
                },
            ));
        }

        if let Some(script_id) = action_id.strip_prefix("script.copy-path:")
            && let Some(sc) = self.find_script(script_id)
        {
            return Some(HostActionResult::response_only(
                PluginResponse::CopyToClipboard {
                    text: sc.path.display().to_string(),
                    hud_title: Some("Copied Script Path".to_string()),
                },
            ));
        }

        if action_id == "settings.open" || action_id == "core.open-settings" {
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some("settings.home".to_string()),
                self.build_settings_view(),
            )));
        }

        if action_id == "settings.reload-all" {
            self.reload_all();
            return Some(HostActionResult {
                response: PluginResponse::render(self.build_settings_view()),
                side_effects: vec![HostSideEffect::ShowToast(ToastMessage {
                    style: ToastStyle::Success,
                    title: format!(
                        "Loaded {} WASM plugin(s) and {} script(s)",
                        self.plugins.len(),
                        self.scripts.len()
                    ),
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
            let plugin = self.find_plugin(plugin_id)?;
            let mut values = HashMap::new();
            if let Ok(db) = self.db.lock() {
                for pref in &plugin.manifest.preferences {
                    if let Ok(Some(v)) = db.get_plugin_preference(plugin_id, &pref.name) {
                        values.insert(pref.name.clone(), v);
                    }
                }
            }
            return Some(HostActionResult::response_only(PluginResponse::push(
                Some(format!("settings.prefs:{plugin_id}")),
                build_plugin_preferences_form(plugin, &values),
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

fn split_script_confirm(rest: &str) -> (&str, &str) {
    // `script:<stem>:<json>` -> split after the second `:`
    if let Some(after_prefix) = rest.strip_prefix("script:")
        && let Some(idx) = after_prefix.find(':')
    {
        let split_at = "script:".len() + idx;
        return (&rest[..split_at], &rest[split_at + 1..]);
    }
    (rest, "[]")
}

/// Helper to build a default Recovery ActionPanel for plugin UI screens.
#[must_use]
pub fn default_plugin_recovery_actions() -> ActionPanel {
    ActionPanel::from_actions(vec![
        ActionItem::new("settings.open", "Open Settings & Extensions").shortcut("Enter"),
        ActionItem::new("nav.pop", "Return").shortcut("Esc"),
    ])
}
