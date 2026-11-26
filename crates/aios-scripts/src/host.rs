//! Standalone `ScriptHost` managing local Script Commands discovery, execution,
//! confirmation prompts, inline summaries, and the Script Commands Manager view.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aios_core::AiosPaths;
use aios_plugin_sdk::{
    ConfirmAlert, PluginResponse, ToastMessage, ToastStyle, ViewNode,
};

use crate::manager_view::build_scripts_manager_view;
use crate::runner::{
    ScriptCommand, ScriptMode, discover_script_commands, execute_script_command,
    should_try_xdg_icon_lookup,
};

type IconLookupFn = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

/// Summary entry exported to Root Search for one discovered Script Command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootScriptEntry {
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

/// Action outcome emitted by [`ScriptHost`] back to the GPUI shell.
#[derive(Debug, Clone)]
pub struct ScriptActionResult {
    pub response: PluginResponse,
    pub toast: Option<ToastMessage>,
}

impl ScriptActionResult {
    #[must_use]
    pub fn response_only(response: PluginResponse) -> Self {
        Self {
            response,
            toast: None,
        }
    }
}

/// Top-level Script Commands host owned by the AIOS daemon / preview shell.
pub struct ScriptHost {
    paths: AiosPaths,
    scripts: Vec<ScriptCommand>,
    last_script_outputs: HashMap<String, String>,
    inline_script_summaries: HashMap<String, String>,
    icon_resolver: Option<IconLookupFn>,
    script_timeout: Duration,
}

impl std::fmt::Debug for ScriptHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHost")
            .field("paths", &self.paths)
            .field("scripts", &self.scripts.len())
            .field("has_icon_resolver", &self.icon_resolver.is_some())
            .finish_non_exhaustive()
    }
}

impl ScriptHost {
    /// Initializes the Script Commands host by scanning `$XDG_CONFIG_HOME/aios/scripts/`.
    #[must_use]
    pub fn new(paths: AiosPaths) -> Self {
        Self::new_internal(paths, None)
    }

    /// Initializes the Script Commands host with a system XDG icon theme resolver so scripts
    /// can declare `# @aios.icon drive-harddisk` in addition to local `.svg`/`.png` paths.
    #[must_use]
    pub fn with_icon_resolver(
        paths: AiosPaths,
        resolver: impl Fn(&str) -> Option<PathBuf> + Send + Sync + 'static,
    ) -> Self {
        Self::new_internal(paths, Some(Arc::new(resolver)))
    }

    fn new_internal(paths: AiosPaths, icon_resolver: Option<IconLookupFn>) -> Self {
        let mut host = Self {
            paths,
            scripts: Vec::new(),
            last_script_outputs: HashMap::new(),
            inline_script_summaries: HashMap::new(),
            icon_resolver,
            script_timeout: Duration::from_secs(15),
        };
        host.reload();
        host
    }

    /// Re-scans `$XDG_CONFIG_HOME/aios/scripts/` and refreshes inline script summaries.
    pub fn reload(&mut self) {
        self.scripts = discover_script_commands(&self.paths.scripts_dir());
        if let Some(resolver) = &self.icon_resolver {
            for sc in &mut self.scripts {
                if sc.icon_path.is_none()
                    && let Some(spec) = sc.icon.as_deref()
                    && should_try_xdg_icon_lookup(spec)
                    && let Some(resolved) = resolver(spec)
                {
                    sc.icon_path = Some(resolved);
                }
            }
        }
        self.inline_script_summaries.clear();
        self.refresh_inline_scripts();
    }

    pub fn set_script_timeout(&mut self, timeout: Duration) {
        self.script_timeout = timeout;
    }

    #[must_use]
    pub fn scripts(&self) -> &[ScriptCommand] {
        &self.scripts
    }

    /// Finds a script by exact ID (`script:<stem>`) or bare stem (`<stem>`).
    #[must_use]
    pub fn find_script(&self, script_id: &str) -> Option<&ScriptCommand> {
        self.scripts.iter().find(|s| {
            s.id == script_id || s.id.strip_prefix("script:") == Some(script_id)
        })
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

    /// Returns Root Search entries for all discovered Script Commands (under filter group `"scripts"`).
    #[must_use]
    pub fn root_search_entries(&self) -> Vec<RootScriptEntry> {
        self.scripts
            .iter()
            .map(|sc| {
                let mut keywords = vec!["script".to_string(), sc.mode.label().to_string()];
                if let Some(pkg) = &sc.package_name {
                    keywords.push(pkg.clone());
                }
                RootScriptEntry {
                    id: sc.id.clone(),
                    title: sc.title.clone(),
                    subtitle: sc
                        .description
                        .clone()
                        .or_else(|| sc.package_name.clone())
                        .unwrap_or_else(|| "Script Command".to_string()),
                    badge: sc.badge_label(),
                    icon_path: sc.icon_path.as_ref().map(|p| p.display().to_string()),
                    keywords,
                    filter_group: "scripts",
                    action_id: format!("script.run:{}", sc.id),
                    secondary_action_id: Some((
                        format!("script.copy-path:{}", sc.id),
                        "Copy Script Path".to_string(),
                    )),
                    tag_label: format!("Script · {}", sc.mode.label()),
                    inline_summary: self.inline_script_summaries.get(&sc.id).cloned(),
                }
            })
            .collect()
    }

    /// Builds the dedicated Script Commands Manager `ViewNode`.
    #[must_use]
    pub fn build_manager_view(&self) -> ViewNode {
        build_scripts_manager_view(&self.paths, &self.scripts, &self.inline_script_summaries)
    }

    /// Runs a Script Command by ID, presenting a `ConfirmAlert` first when `needs_confirmation` is true.
    pub fn run_script(
        &mut self,
        script_id: &str,
        args: &[String],
        confirmed: bool,
    ) -> ScriptActionResult {
        let Some(script) = self.find_script(script_id).cloned() else {
            return ScriptActionResult::response_only(PluginResponse::ShowToast {
                toast: ToastMessage {
                    style: ToastStyle::Failure,
                    title: format!("Script '{script_id}' not found"),
                    message: Some("Reload scripts in Script Commands manager.".to_string()),
                },
            });
        };

        if script.needs_confirmation && !confirmed {
            let encoded_args = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
            let alert: ConfirmAlert = script.confirmation_alert(&encoded_args);
            return ScriptActionResult::response_only(PluginResponse::Confirm { alert });
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

        ScriptActionResult::response_only(response)
    }

    /// Dispatches `script.*`, `scripts.*`, and `core.open-scripts` actions emitted by the UI shell.
    pub fn dispatch_action(
        &mut self,
        action_id: &str,
        inline_args: &[String],
    ) -> Option<ScriptActionResult> {
        if action_id == "scripts.open" || action_id == "core.open-scripts" {
            return Some(ScriptActionResult::response_only(PluginResponse::push(
                Some("scripts.manager".to_string()),
                self.build_manager_view(),
            )));
        }

        if action_id == "scripts.reload" {
            self.reload();
            return Some(ScriptActionResult {
                response: PluginResponse::render(self.build_manager_view()),
                toast: Some(ToastMessage {
                    style: ToastStyle::Success,
                    title: format!("Loaded {} script command(s)", self.scripts.len()),
                    message: None,
                }),
            });
        }

        if action_id == "scripts.copy-dir" {
            return Some(ScriptActionResult::response_only(
                PluginResponse::CopyToClipboard {
                    text: self.paths.scripts_dir().display().to_string(),
                    hud_title: Some("Copied Scripts Directory Path".to_string()),
                },
            ));
        }

        if let Some(script_id) = action_id.strip_prefix("script.run:") {
            return Some(self.run_script(script_id, inline_args, false));
        }

        if let Some(rest) = action_id.strip_prefix("script.confirm:") {
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
            return Some(ScriptActionResult::response_only(
                PluginResponse::CopyToClipboard {
                    text,
                    hud_title: Some("Copied Script Output".to_string()),
                },
            ));
        }

        if let Some(script_id) = action_id.strip_prefix("script.copy-path:")
            && let Some(sc) = self.find_script(script_id)
        {
            return Some(ScriptActionResult::response_only(
                PluginResponse::CopyToClipboard {
                    text: sc.path.display().to_string(),
                    hud_title: Some("Copied Script Path".to_string()),
                },
            ));
        }

        None
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
