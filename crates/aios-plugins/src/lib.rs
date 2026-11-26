//! First-party Pure Rust plugins suite for AIOS (Phase 5).
//!
//! Every plugin in this crate speaks the exact same declarative [`aios_plugin_sdk`] contract
//! (`PluginManifest`, `PluginEvent`, `PluginResponse`, `ViewNode`) and persists state in the
//! shared SQLite plugin KV / preferences store via [`PluginHost`], while staying completely
//! decoupled from AIOS core (`aios-builtin`).

#[cfg(feature = "host")]
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "host")]
use aios_core::Database;
pub use aios_plugin_sdk::InlinePluginResult;
#[cfg(feature = "host")]
use aios_plugin_sdk::{PluginEvent, PluginManifest, PluginResponse};

pub mod calculator;

/// Scoped host environment passed into first-party plugins for SQLite KV storage, preferences,
/// and theme awareness.
#[cfg(feature = "host")]
#[derive(Debug)]
pub struct PluginEnv<'a> {
    pub plugin_id: &'a str,
    pub theme_mode: &'a str,
    db: Arc<Mutex<Database>>,
}

#[cfg(feature = "host")]
impl<'a> PluginEnv<'a> {
    #[must_use]
    pub fn new(plugin_id: &'a str, theme_mode: &'a str, db: Arc<Mutex<Database>>) -> Self {
        Self {
            plugin_id,
            theme_mode,
            db,
        }
    }

    #[must_use]
    pub fn storage_get(&self, key: &str) -> Option<Vec<u8>> {
        let db = self.db.lock().ok()?;
        db.plugin_storage_get(self.plugin_id, key).ok().flatten()
    }

    pub fn storage_set(&self, key: &str, value: &[u8]) -> Result<(), String> {
        let db = self
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.plugin_storage_set(self.plugin_id, key, value, now_epoch_secs())
            .map_err(|e| e.to_string())
    }

    pub fn storage_remove(&self, key: &str) -> Result<(), String> {
        let db = self
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.plugin_storage_remove(self.plugin_id, key)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[must_use]
    pub fn preference(&self, name: &str) -> Option<String> {
        let db = self.db.lock().ok()?;
        db.get_plugin_preference(self.plugin_id, name).ok().flatten()
    }

    /// Returns a cloned handle to the shared SQLite database for non-blocking background updates.
    #[must_use]
    pub fn db_arc(&self) -> Arc<Mutex<Database>> {
        Arc::clone(&self.db)
    }
}

/// Contract implemented by all first-party AIOS plugins, matching the WASM plugin lifecycle.
#[cfg(feature = "host")]
pub trait FirstPartyPlugin: Send + Sync + std::fmt::Debug {
    fn manifest(&self) -> PluginManifest;

    fn badge(&self) -> &'static str {
        "PL"
    }

    /// Optional embedded icon asset URI (for example `"asset://icons/plugins/calculator.svg"`).
    fn icon_asset(&self) -> Option<&'static str> {
        None
    }

    fn init_command(
        &self,
        command_id: &str,
        args: &[String],
        env: &PluginEnv<'_>,
    ) -> PluginResponse;

    fn handle_event(
        &self,
        command_id: &str,
        event: &PluginEvent,
        env: &PluginEnv<'_>,
    ) -> PluginResponse;

    /// Optional fast-gated inline Root Search evaluator. Plugins without live inline queries
    /// leave this as `None` (zero execution cost on Root Search keystrokes).
    fn evaluate_inline_query(
        &self,
        _query: &str,
        _env: &PluginEnv<'_>,
    ) -> Option<InlinePluginResult> {
        None
    }

    /// Optional handler for inline actions triggered directly from Root Search (e.g. copying
    /// an inline calculation result while recording it in plugin history).
    fn handle_inline_action(
        &self,
        _action_id: &str,
        _env: &PluginEnv<'_>,
    ) -> Option<PluginResponse> {
        None
    }
}

/// Returns the default suite of enabled first-party AIOS fallback plugins.
#[cfg(feature = "host")]
#[must_use]
pub fn default_first_party_plugins() -> Vec<Box<dyn FirstPartyPlugin>> {
    vec![Box::new(calculator::CalculatorPlugin::new())]
}

#[must_use]
pub fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
