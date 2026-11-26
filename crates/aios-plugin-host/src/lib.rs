//! WASM Component Model plugin host runtime (`wasmtime` + WIT), capability security
//! enforcement, hot-reload dev watching, and declarative Settings & Plugins views.
//!
//! Local Script Commands live in the dedicated `aios-scripts` crate.

pub mod archive;
pub mod manager;
pub mod manifest;
pub mod permissions;
pub mod registry;
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
pub use archive::{
    PLUGIN_ARCHIVE_EXT, install_plugin_archive_bytes, pack_plugin_dir, sha256_bytes, sha256_file,
};
pub use manager::{HostActionResult, PluginHost, RootPluginEntry};
pub use manifest::{
    InstalledPlugin, MANIFEST_FILE_NAME, PluginOrigin, discover_layered_plugins, discover_plugins,
    install_plugin_from_dir, load_plugin_from_dir, uninstall_plugin,
};
pub use permissions::{HostContext, HostSideEffect};
pub use registry::{
    PluginRegistryIndex, RegistryPluginEntry, RegistryRelease, build_registry_index_from_dir,
    fetch_registry_index, install_plugin_from_archive_file, install_plugin_from_github,
    install_plugin_from_registry_entry, is_newer_version, load_cached_registry_index,
    validate_registry_entry,
};
pub use settings::{
    build_install_plugin_form, build_plugin_capabilities_view, build_plugin_preferences_form,
    build_plugin_store_view, build_settings_home_view, build_uninstall_confirm_alert,
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
