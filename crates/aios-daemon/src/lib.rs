//! Background daemon protocol and lifecycle orchestration for AIOS.

use serde::{Deserialize, Serialize};

/// Case sensitivity behavior for `aios dmenu` query matching.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DmenuCaseMode {
    /// Case-insensitive when query is all lowercase; case-sensitive if query has uppercase.
    #[default]
    Smart,
    /// Always match case-insensitively (`-i` / `--insensitive`).
    Insensitive,
    /// Always match case-sensitively (`-s` / `--sensitive`).
    Sensitive,
}

/// Request payload for `aios dmenu` interactive stdin -> stdout fuzzy selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmenuRequest {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub case_mode: DmenuCaseMode,
    #[serde(default)]
    pub allow_custom: bool,
    #[serde(default)]
    pub return_index: bool,
    #[serde(default)]
    pub theme: Option<String>,
    /// Optional header icon (resolved `.svg`/`.png` file path or short fallback badge).
    #[serde(default)]
    pub icon: Option<String>,
    /// Whether to display the per-row icon column (`--show-icons`).
    #[serde(default)]
    pub show_icons: bool,
    /// Optional per-row icon path/badge aligned 1-to-1 with `items` (parsed from rofi `\0icon\x1f...`).
    #[serde(default)]
    pub item_icons: Vec<Option<String>>,
    /// When `true`, additional items arrive asynchronously via [`DmenuStreamMessage`] frames
    /// over the same IPC connection while the picker is already open and interactive.
    #[serde(default)]
    pub streaming: bool,
}

/// Incremental streaming frames sent from `aios dmenu` CLI to `aios daemon` while stdin is open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stream", rename_all = "kebab-case")]
pub enum DmenuStreamMessage {
    Batch {
        items: Vec<String>,
        #[serde(default)]
        item_icons: Vec<Option<String>>,
    },
    Eof,
}

/// Control messages sent from `aios` CLI invocations to the running daemon over Unix socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum DaemonIpcCommand {
    Toggle,
    Show,
    Hide,
    Open { uri: String },
    Dmenu(DmenuRequest),
    Status,
    Ping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub visible: bool,
    pub compositor: String,
    pub active_theme: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum DaemonIpcResponse {
    Ok,
    Pong { version: String },
    Status(DaemonStatus),
    DmenuSelected { value: String, index: Option<usize> },
    DmenuCancelled,
    Error { message: String },
}
