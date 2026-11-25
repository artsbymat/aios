//! Background daemon protocol and lifecycle orchestration for AIOS.

use serde::{Deserialize, Serialize};

/// Control messages sent from `aios` CLI invocations to the running daemon over Unix socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum DaemonIpcCommand {
    Toggle,
    Show,
    Hide,
    Open { uri: String },
    Status,
    Ping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum DaemonIpcResponse {
    Ok,
    Pong { version: String },
    Error { message: String },
}
