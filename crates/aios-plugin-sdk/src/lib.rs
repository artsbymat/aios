//! Ergonomic plugin SDK shared by built-in Rust plugins and WASM guests.

pub use aios_core::view::{
    ActionItem, ActionPanel, CommandMode, ConfirmAlert, DetailView, EmptyView, FilterMode,
    FormField, FormView, GridView, HudMessage, ListItem, ListSection, ListView, ToastMessage,
    ToastStyle, ViewNode,
};
use serde::{Deserialize, Serialize};

/// Declarative manifest parsed from `aios-plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub entry: String,
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub mode: CommandMode,
}
