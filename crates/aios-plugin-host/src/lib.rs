//! Hybrid plugin host runtime (built-in Rust, sandboxed WASM, Script Commands).
//!
//! Full Wasmtime Component Model execution and capability enforcement arrive in Phase 4.

use aios_plugin_sdk::PluginManifest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginTier {
    BuiltinNative,
    WasmComponent,
    ScriptCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    ClipboardRead,
    ClipboardWrite,
    HttpHost(String),
    FsRead(String),
    Secrets,
}

#[derive(Debug, Default)]
pub struct PluginRegistry {
    manifests: Vec<PluginManifest>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, manifest: PluginManifest) {
        self.manifests.push(manifest);
    }

    pub fn manifests(&self) -> &[PluginManifest] {
        &self.manifests
    }
}
