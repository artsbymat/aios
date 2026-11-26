//! Ergonomic plugin SDK shared by built-in Rust plugins, host manifest loaders, and WASM guests.

pub mod builders;
#[cfg(feature = "guest")]
pub mod guest;
pub mod protocol;

/// Convenience re-exports for plugin authors (`use aios_plugin_sdk::prelude::*;`).
pub mod prelude {
    pub use crate::builders::{
        ActionPanelBuilder, DetailViewBuilder, FormViewBuilder, GridItemBuilder, GridViewBuilder,
        ListItemBuilder, ListViewBuilder, error_view,
    };
    #[cfg(feature = "guest")]
    pub use crate::guest::{AiosPlugin, host};
    pub use crate::protocol::{
        InlinePluginResult, MenuBarItem, MenuBarState, PluginEvent, PluginResponse,
    };
    pub use aios_core::view::{
        Accessory, ActionItem, ActionPanel, ActionSection, CommandMode, ConfirmAlert, DetailView,
        EmptyView, ErrorView, FilterDropdown, FilterMode, FilterOption, FormField, FormView,
        GridAspectRatio, GridItem, GridItemContent, GridSection, GridView, HudMessage,
        InlineArgumentSpec, ItemDetail, ListItem, ListSection, ListView, MetadataEntry,
        SemanticTone, ToastMessage, ToastStyle, ViewNode,
    };
}

pub use aios_core::view::{
    Accessory, ActionItem, ActionPanel, ActionSection, CommandMode, ConfirmAlert, DetailView,
    EmptyView, ErrorView, FilterDropdown, FilterMode, FilterOption, FormField, FormView,
    GridAspectRatio, GridItem, GridItemContent, GridSection, GridView, HudMessage,
    InlineArgumentSpec, ItemDetail, ListItem, ListSection, ListView, MetadataEntry, SemanticTone,
    ToastMessage, ToastStyle, ViewNode,
};
pub use builders::{
    ActionPanelBuilder, DetailViewBuilder, FormViewBuilder, GridItemBuilder, GridViewBuilder,
    ListItemBuilder, ListViewBuilder, error_view,
};
pub use protocol::{InlinePluginResult, MenuBarItem, MenuBarState, PluginEvent, PluginResponse};

use serde::{Deserialize, Serialize};

/// Top-level `aios-plugin.toml` manifest document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifestFile {
    pub plugin: PluginMetadata,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub inline_trigger: InlineTriggerSpec,
    #[serde(default)]
    pub preferences: Vec<PreferenceSpec>,
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
}

impl PluginManifestFile {
    /// Parses and validates an `aios-plugin.toml` document.
    pub fn from_toml_str(raw: &str) -> Result<Self, String> {
        let parsed: Self =
            toml::from_str(raw).map_err(|e| format!("invalid manifest TOML: {e}"))?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Validates required fields, plugin ID format, and command definitions.
    pub fn validate(&self) -> Result<(), String> {
        if self.plugin.id.trim().is_empty() {
            return Err("plugin.id must not be empty".to_string());
        }
        if !self
            .plugin
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(format!(
                "plugin.id '{}' contains invalid characters (allowed: a-z, A-Z, 0-9, ., -, _)",
                self.plugin.id
            ));
        }
        if self.plugin.name.trim().is_empty() {
            return Err("plugin.name must not be empty".to_string());
        }
        if self.plugin.entry.trim().is_empty() {
            return Err("plugin.entry must specify a .wasm component file".to_string());
        }
        if self.commands.is_empty() {
            return Err("manifest must declare at least one [[commands]] entry".to_string());
        }
        for cmd in &self.commands {
            if cmd.id.trim().is_empty() || cmd.title.trim().is_empty() {
                return Err("every command must have a non-empty id and title".to_string());
            }
        }
        Ok(())
    }

    /// Converts this manifest file into the flattened [`PluginManifest`] representation.
    #[must_use]
    pub fn into_manifest(self) -> PluginManifest {
        PluginManifest {
            id: self.plugin.id,
            name: self.plugin.name,
            version: self.plugin.version,
            description: self.plugin.description,
            author: self.plugin.author,
            min_aios_version: self.plugin.min_aios_version,
            entry: self.plugin.entry,
            icon: self.plugin.icon,
            capabilities: self.capabilities,
            inline_trigger: self.inline_trigger,
            preferences: self.preferences,
            commands: self.commands,
        }
    }
}

/// Declarative sub-microsecond host fast-gate (`[inline_trigger]` in `aios-plugin.toml`).
///
/// The AIOS host evaluates this rule natively before invoking Wasmtime so normal app queries
/// (`"firefox"`, `"code"`, `"settings"`) incur zero WASM startup overhead.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InlineTriggerSpec {
    /// Explicit prefixes that always trigger inline evaluation (e.g. `["="]`).
    pub prefixes: Vec<String>,
    /// Accept queries beginning with an ASCII digit followed by an operator, unit, or base suffix.
    pub match_numeric_expressions: bool,
    /// Leading characters/symbols that trigger inline evaluation (e.g. `["$", "€", "(", "-"]`).
    pub leading_chars: Vec<String>,
    /// Case-insensitive leading keywords/prefixes (e.g. `["today", "now ", "sqrt("]`).
    pub leading_keywords: Vec<String>,
}

impl InlineTriggerSpec {
    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.prefixes.is_empty()
            || self.match_numeric_expressions
            || !self.leading_chars.is_empty()
            || !self.leading_keywords.is_empty()
    }

    /// Evaluates the fast-gate in nanoseconds with zero regex overhead.
    #[must_use]
    pub fn matches_fast_gate(&self, raw: &str) -> bool {
        let s = raw.trim();
        if s.is_empty() || !self.is_configured() {
            return false;
        }
        if self
            .prefixes
            .iter()
            .any(|p| !p.is_empty() && s.starts_with(p.as_str()))
        {
            return true;
        }
        if self
            .leading_chars
            .iter()
            .any(|c| !c.is_empty() && s.starts_with(c.as_str()))
        {
            return true;
        }
        let first = s.as_bytes()[0];
        if self.match_numeric_expressions && first.is_ascii_digit() {
            let has_indicator = s.bytes().any(|b| {
                matches!(
                    b,
                    b'+' | b'-'
                        | b'*'
                        | b'/'
                        | b'%'
                        | b'^'
                        | b'!'
                        | b'&'
                        | b'|'
                        | b'<'
                        | b'>'
                        | b' '
                        | b'a'..=b'z'
                        | b'A'..=b'Z'
                )
            });
            if has_indicator {
                return true;
            }
        }
        if !self.leading_keywords.is_empty() {
            let lower = s.to_ascii_lowercase();
            if self.leading_keywords.iter().any(|kw| {
                let kw_l = kw.to_ascii_lowercase();
                !kw_l.is_empty() && (lower == kw_l || lower.starts_with(&kw_l))
            }) {
                return true;
            }
        }
        false
    }
}

/// `[plugin]` header inside `aios-plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub min_aios_version: Option<String>,
    pub entry: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// `[capabilities]` section declaring every privileged resource the plugin may request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginCapabilities {
    pub clipboard_read: bool,
    pub clipboard_write: bool,
    pub http_hosts: Vec<String>,
    pub fs_read: Vec<String>,
    pub secrets: bool,
}

impl PluginCapabilities {
    /// Returns the list of canonical capability keys declared by this manifest.
    #[must_use]
    pub fn declared_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if self.clipboard_read {
            keys.push("clipboard_read".to_string());
        }
        if self.clipboard_write {
            keys.push("clipboard_write".to_string());
        }
        if self.secrets {
            keys.push("secrets".to_string());
        }
        for host in &self.http_hosts {
            keys.push(format!("http:{host}"));
        }
        for path in &self.fs_read {
            keys.push(format!("fs_read:{path}"));
        }
        keys
    }
}

/// Input control type for `[[preferences]]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreferenceKind {
    #[default]
    Textfield,
    Password,
    Checkbox,
    Dropdown,
}

/// User-configurable plugin setting declared in `[[preferences]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferenceSpec {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: PreferenceKind,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<FilterOption>,
}

/// Flattened plugin manifest used at runtime by the host registry and Extension Manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub min_aios_version: Option<String>,
    pub entry: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub inline_trigger: InlineTriggerSpec,
    #[serde(default)]
    pub preferences: Vec<PreferenceSpec>,
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
}

/// Optional hint for the primary view type rendered by a `CommandMode::View` command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultViewKind {
    #[default]
    List,
    Grid,
    Detail,
    Form,
}

/// Command declared inside `[[commands]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    pub mode: CommandMode,
    #[serde(default)]
    pub default_view: DefaultViewKind,
    #[serde(default)]
    pub needs_confirmation: bool,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub arguments: Vec<InlineArgumentSpec>,
    #[serde(default)]
    pub refresh_interval_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_toml_with_capabilities_preferences_and_commands() {
        let toml_src = r#"
[plugin]
id = "com.example.sample-plugin"
name = "Sample Plugin"
version = "0.1.0"
description = "Sample plugin manifest for unit tests."
author = "AIOS"
min_aios_version = "0.1.0"
entry = "plugin.wasm"

[capabilities]
clipboard_write = true
http_hosts = ["https://api.example.com"]
fs_read = ["/tmp/sample"]
secrets = false

[[preferences]]
name = "default_tag"
title = "Default Tag"
kind = "textfield"
default = "inbox"

[[commands]]
id = "browse"
title = "Browse Sample Items"
subtitle = "Sample Plugin"
mode = "view"
default_view = "list"
"#;

        let parsed = PluginManifestFile::from_toml_str(toml_src).expect("valid manifest");
        let manifest = parsed.into_manifest();
        assert_eq!(manifest.id, "com.example.sample-plugin");
        assert!(manifest.capabilities.clipboard_write);
        assert_eq!(
            manifest.capabilities.declared_keys(),
            vec![
                "clipboard_write",
                "http:https://api.example.com",
                "fs_read:/tmp/sample"
            ]
        );
        assert_eq!(manifest.preferences.len(), 1);
        assert_eq!(manifest.commands.len(), 1);
        assert_eq!(manifest.commands[0].mode, CommandMode::View);
    }

    #[test]
    fn builders_and_protocol_round_trip_json() {
        let view = ListViewBuilder::new("Demo Plugin")
            .showing_detail(true)
            .section(
                Some("Entries"),
                None,
                vec![
                    ListItemBuilder::new("n1", "First Note")
                        .subtitle("Inbox")
                        .detail("# First Note\n\nBody text", vec![])
                        .actions(
                            ActionPanelBuilder::new()
                                .action(ActionItem::new("copy", "Copy Note").shortcut("Enter"))
                                .build(),
                        )
                        .build(),
                ],
            )
            .build();

        let resp = PluginResponse::render(view);
        let json = resp.to_json().expect("serialize");
        let decoded = PluginResponse::from_json(&json).expect("deserialize");
        assert_eq!(resp, decoded);
    }
}
