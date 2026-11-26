//! WASM Component Model plugin host runtime (`wasmtime` + WIT), capability security
//! enforcement, hot-reload dev watching, and declarative Settings & Plugins views.
//!
//! Local Script Commands live in the dedicated `aios-scripts` crate.

pub mod manager;
pub mod manifest;
pub mod permissions;
pub mod settings;
pub mod wasm_runtime;
pub mod watcher;

use aios_plugin_sdk::PluginManifest;
use serde::{Deserialize, Serialize};

pub use aios_plugins::calculator::{
    CALCULATOR_REFRESH_RATES_ACTION, CachedCurrencyRates, maybe_schedule_stale_currency_refresh,
    refresh_currency_rates_now,
};
pub use aios_plugins::{FirstPartyPlugin, InlinePluginResult, PluginEnv};
pub use manager::{HostActionResult, PluginHost, RootPluginEntry};
pub use manifest::{
    InstalledPlugin, MANIFEST_FILE_NAME, discover_plugins, install_plugin_from_dir,
    load_plugin_from_dir, uninstall_plugin,
};
pub use permissions::{HostContext, HostSideEffect};
pub use settings::{
    build_install_plugin_form, build_plugin_capabilities_view, build_plugin_preferences_form,
    build_settings_home_view, build_uninstall_confirm_alert,
};
pub use wasm_runtime::{WasmCallOutput, WasmExecutionLimits, WasmPluginRuntime};
pub use watcher::{DevReloadEvent, PluginDevWatcher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginTier {
    BuiltinNative,
    WasmComponent,
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, manifest: PluginManifest) {
        self.manifests.push(manifest);
    }

    #[must_use]
    pub fn manifests(&self) -> &[PluginManifest] {
        &self.manifests
    }
}
