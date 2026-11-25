//! Tier-3 Script Commands discovery, metadata header parser (`@aios.*`), and timeout-safe runner.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aios_plugin_sdk::{
    ActionItem, ActionPanel, ConfirmAlert, DetailViewBuilder, ErrorView, HudMessage,
    InlineArgumentSpec, MetadataEntry, PluginResponse, SemanticTone, ToastMessage, ToastStyle,
    ViewNode,
};
use serde::{Deserialize, Serialize};

/// Output presentation mode declared via `# @aios.mode <mode>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptMode {
    Silent,
    #[default]
    Compact,
    FullOutput,
    Inline,
}

impl ScriptMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Compact => "compact",
            Self::FullOutput => "fullOutput",
            Self::Inline => "inline",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "silent" => Some(Self::Silent),
            "compact" => Some(Self::Compact),
            "fulloutput" | "full-output" => Some(Self::FullOutput),
            "inline" => Some(Self::Inline),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawScriptArg {
    #[serde(rename = "type", default = "default_arg_type")]
    arg_type: String,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    optional: bool,
}

fn default_arg_type() -> String {
    "text".to_string()
}

/// Parsed Script Command discovered in `$XDG_CONFIG_HOME/aios/scripts/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCommand {
    pub id: String,
    pub schema_version: u32,
    pub title: String,
    pub package_name: Option<String>,
    pub description: Option<String>,
    pub mode: ScriptMode,
    pub needs_confirmation: bool,
    pub refresh_time_secs: Option<u64>,
    pub icon: Option<String>,
    pub arguments: Vec<InlineArgumentSpec>,
    pub path: PathBuf,
}

impl ScriptCommand {
    /// Builds a standard destructive/confirmation modal if `needs_confirmation` is enabled.
    #[must_use]
    pub fn confirmation_alert(&self, encoded_args: &str) -> ConfirmAlert {
        ConfirmAlert {
            id: format!("script.confirm:{}:{encoded_args}", self.id),
            title: format!("Run script \"{}\"?", self.title),
            message: self
                .description
                .clone()
                .unwrap_or_else(|| format!("Execute local script at {}.", self.path.display())),
            primary_action: format!("Run {}", self.title),
            dismiss_action: "Cancel".to_string(),
            destructive: true,
        }
    }
}

/// Raw outcome of running a Script Command subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRunOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

impl ScriptRunOutcome {
    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }

    #[must_use]
    pub fn last_non_empty_line(&self) -> Option<&str> {
        self.stdout
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
    }
}

/// Scans `scripts_dir` for files carrying `@aios.schemaVersion` + `@aios.title` headers.
#[must_use]
pub fn discover_script_commands(scripts_dir: &Path) -> Vec<ScriptCommand> {
    let Ok(entries) = fs::read_dir(scripts_dir) else {
        return Vec::new();
    };
    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(cmd) = parse_script_metadata(&path, &content) {
            scripts.push(cmd);
        }
    }
    scripts.sort_by_key(|a| a.title.to_lowercase());
    scripts
}

/// Parses `# @aios.<key> <value>` (or `// @aios.<key> <value>`) metadata headers from the top
/// 40 lines of a script file.
#[must_use]
pub fn parse_script_metadata(path: &Path, content: &str) -> Option<ScriptCommand> {
    let mut schema_version: Option<u32> = None;
    let mut title: Option<String> = None;
    let mut package_name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut mode = ScriptMode::Compact;
    let mut needs_confirmation = false;
    let mut refresh_time_secs: Option<u64> = None;
    let mut icon: Option<String> = None;
    let mut args_by_slot: [(u8, Option<InlineArgumentSpec>); 3] = [(1, None), (2, None), (3, None)];

    for line in content.lines().take(40) {
        let trimmed = line.trim();
        let comment_body = trimmed
            .strip_prefix('#')
            .or_else(|| trimmed.strip_prefix("//"))
            .map(str::trim);
        let Some(body) = comment_body else {
            continue;
        };
        let Some(directive) = body.strip_prefix("@aios.") else {
            continue;
        };
        let (key, val) = match directive.split_once(char::is_whitespace) {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (directive.trim(), ""),
        };

        match key {
            "schemaVersion" => schema_version = val.parse::<u32>().ok(),
            "title" if !val.is_empty() => title = Some(val.to_owned()),
            "packageName" if !val.is_empty() => package_name = Some(val.to_owned()),
            "description" if !val.is_empty() => description = Some(val.to_owned()),
            "mode" => {
                if let Some(m) = ScriptMode::parse(val) {
                    mode = m;
                }
            }
            "needsConfirmation" => needs_confirmation = val.eq_ignore_ascii_case("true"),
            "refreshTime" => refresh_time_secs = parse_refresh_duration(val),
            "icon" if !val.is_empty() => icon = Some(val.to_owned()),
            "argument1" | "argument2" | "argument3" => {
                let slot = match key {
                    "argument1" => 0,
                    "argument2" => 1,
                    _ => 2,
                };
                if let Ok(raw_arg) = serde_json::from_str::<RawScriptArg>(val) {
                    let placeholder = if raw_arg.placeholder.is_empty() {
                        format!("arg{}", slot + 1)
                    } else {
                        raw_arg.placeholder
                    };
                    args_by_slot[slot].1 = Some(InlineArgumentSpec {
                        id: key.to_string(),
                        placeholder,
                        required: !raw_arg.optional,
                    });
                }
            }
            _ => {}
        }
    }

    let schema_version = schema_version?;
    let title = title?;
    let stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("script")
        .to_owned();

    let arguments = args_by_slot
        .into_iter()
        .filter_map(|(_, spec)| spec)
        .collect();

    Some(ScriptCommand {
        id: format!("script:{stem}"),
        schema_version,
        title,
        package_name,
        description,
        mode,
        needs_confirmation,
        refresh_time_secs,
        icon,
        arguments,
        path: path.to_path_buf(),
    })
}

fn parse_refresh_duration(raw: &str) -> Option<u64> {
    let s = raw.trim().to_ascii_lowercase();
    if let Some(num) = s.strip_suffix('s') {
        num.trim().parse::<u64>().ok()
    } else if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<u64>().ok().map(|v| v.saturating_mul(60))
    } else if let Some(num) = s.strip_suffix('h') {
        num.trim()
            .parse::<u64>()
            .ok()
            .map(|v| v.saturating_mul(3600))
    } else {
        s.parse::<u64>().ok()
    }
}

/// Runs a Script Command with a hard wall-clock timeout and maps its output to a [`PluginResponse`].
pub fn execute_script_command(
    script: &ScriptCommand,
    args: &[String],
    timeout: Duration,
) -> (ScriptRunOutcome, PluginResponse) {
    let start = Instant::now();
    let mut cmd = build_script_process(&script.path);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("AIOS_SCRIPT_ID", &script.id)
        .env("AIOS_SCRIPT_TITLE", &script.title);

    for (idx, val) in args.iter().enumerate() {
        cmd.env(format!("AIOS_ARG_{}", idx + 1), val);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            let outcome = ScriptRunOutcome {
                exit_code: None,
                timed_out: false,
                stdout: String::new(),
                stderr: err.to_string(),
                duration_ms: start.elapsed().as_millis(),
            };
            let view = ViewNode::Error(ErrorView {
                title: format!("Failed to start script '{}'", script.title),
                message: err.to_string(),
                detail: Some(format!("path={}", script.path.display())),
                actions: ActionPanel::from_actions(vec![
                    ActionItem::new("nav.pop", "Return to Root").shortcut("Enter"),
                ]),
            });
            return (outcome, PluginResponse::push(None, view));
        }
    };

    // Poll until process finishes or timeout expires.
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => break,
        }
    }

    let output = child.wait_with_output();
    let duration_ms = start.elapsed().as_millis();
    let outcome = match output {
        Ok(out) => ScriptRunOutcome {
            exit_code: out.status.code(),
            timed_out,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            duration_ms,
        },
        Err(err) => ScriptRunOutcome {
            exit_code: None,
            timed_out,
            stdout: String::new(),
            stderr: err.to_string(),
            duration_ms,
        },
    };

    let response = map_outcome_to_response(script, &outcome);
    (outcome, response)
}

fn build_script_process(path: &Path) -> Command {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let is_exec = fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
        if is_exec {
            return Command::new(path);
        }
    }
    let mut fallback = Command::new("sh");
    fallback.arg(path);
    fallback
}

fn map_outcome_to_response(script: &ScriptCommand, outcome: &ScriptRunOutcome) -> PluginResponse {
    if outcome.timed_out {
        return PluginResponse::push(
            Some(format!("script-err:{}", script.id)),
            ViewNode::Error(ErrorView {
                title: format!("Script '{}' timed out", script.title),
                message: "The script exceeded its execution timeout and was terminated safely."
                    .to_string(),
                detail: Some(format!(
                    "path={}  duration={}ms",
                    script.path.display(),
                    outcome.duration_ms
                )),
                actions: ActionPanel::from_actions(vec![
                    ActionItem::new(format!("script.run:{}", script.id), "Re-run Script")
                        .shortcut("Enter"),
                    ActionItem::new("nav.pop", "Return").shortcut("Esc"),
                ]),
            }),
        );
    }

    match script.mode {
        ScriptMode::Silent => {
            if outcome.succeeded() {
                let hud = outcome.last_non_empty_line().map(|title| HudMessage {
                    title: title.to_owned(),
                });
                PluginResponse::CloseWindow { hud }
            } else {
                PluginResponse::ShowToast {
                    toast: ToastMessage {
                        style: ToastStyle::Failure,
                        title: format!("{} failed", script.title),
                        message: non_empty_trimmed(&outcome.stderr)
                            .or_else(|| non_empty_trimmed(&outcome.stdout))
                            .map(ToOwned::to_owned),
                    },
                }
            }
        }
        ScriptMode::Compact | ScriptMode::Inline => {
            let summary = outcome
                .last_non_empty_line()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    if outcome.succeeded() {
                        format!("{} completed", script.title)
                    } else {
                        format!("{} exited with {:?}", script.title, outcome.exit_code)
                    }
                });
            PluginResponse::ShowToast {
                toast: ToastMessage {
                    style: if outcome.succeeded() {
                        ToastStyle::Success
                    } else {
                        ToastStyle::Failure
                    },
                    title: summary,
                    message: if outcome.succeeded() {
                        None
                    } else {
                        non_empty_trimmed(&outcome.stderr).map(ToOwned::to_owned)
                    },
                },
            }
        }
        ScriptMode::FullOutput => {
            let mut md = format!("# {}\n\n", script.title);
            if let Some(desc) = &script.description {
                md.push_str(desc);
                md.push_str("\n\n");
            }
            if !outcome.stdout.trim().is_empty() {
                md.push_str("## Output\n\n```text\n");
                md.push_str(outcome.stdout.trim_end());
                md.push_str("\n```\n\n");
            }
            if !outcome.stderr.trim().is_empty() {
                md.push_str("## Standard Error\n\n```text\n");
                md.push_str(outcome.stderr.trim_end());
                md.push_str("\n```\n\n");
            }
            if outcome.stdout.trim().is_empty() && outcome.stderr.trim().is_empty() {
                md.push_str("_(Script produced no output.)_\n");
            }

            let status_tone = if outcome.succeeded() {
                SemanticTone::Success
            } else {
                SemanticTone::Danger
            };
            let status_label = match outcome.exit_code {
                Some(0) => "Exit 0 (OK)".to_string(),
                Some(code) => format!("Exit {code}"),
                None => "Terminated".to_string(),
            };

            let detail = DetailViewBuilder::new(script.title.clone(), md)
                .metadata(vec![
                    MetadataEntry::TagList {
                        title: "Status".to_string(),
                        tags: vec![(status_label, status_tone)],
                    },
                    MetadataEntry::Label {
                        title: "Duration".to_string(),
                        text: format!("{} ms", outcome.duration_ms),
                    },
                    MetadataEntry::Label {
                        title: "Mode".to_string(),
                        text: script.mode.label().to_string(),
                    },
                    MetadataEntry::Separator,
                    MetadataEntry::Link {
                        title: "Script Path".to_string(),
                        text: script.path.display().to_string(),
                        target: script.path.display().to_string(),
                    },
                ])
                .actions(ActionPanel::from_actions(vec![
                    ActionItem::new(format!("script.copy-output:{}", script.id), "Copy Output")
                        .shortcut("Enter"),
                    ActionItem::new(format!("script.run:{}", script.id), "Re-run Script")
                        .shortcut("Ctrl+Enter"),
                    ActionItem::new("nav.pop", "Back to Root").shortcut("Esc"),
                ]))
                .build();

            PluginResponse::push(Some(format!("script-output:{}", script.id)), detail)
        }
    }
}

fn non_empty_trimmed(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t) }
}
