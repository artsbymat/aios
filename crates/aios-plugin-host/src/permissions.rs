//! Capability security policy enforcement, SQLite-backed persistence, and host side-effects.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aios_core::Database;
use aios_plugin_sdk::{HudMessage, PluginManifest, ToastMessage};

/// Side effect emitted by a plugin via `host-api` calls during execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSideEffect {
    ShowToast(ToastMessage),
    ShowHud(HudMessage),
    CloseMainWindow,
    PopToRoot,
    ClipboardWrite(String),
    OpenUrl(String),
}

/// Shared host bridge state passed into each sandboxed WASM execution store.
#[derive(Debug, Clone)]
pub struct HostContext {
    pub manifest: PluginManifest,
    pub db: Arc<Mutex<Database>>,
    pub theme_mode: String,
    pub clipboard: Arc<Mutex<Option<String>>>,
}

impl HostContext {
    #[must_use]
    pub fn new(
        manifest: PluginManifest,
        db: Arc<Mutex<Database>>,
        theme_mode: impl Into<String>,
        clipboard: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            manifest,
            db,
            theme_mode: theme_mode.into(),
            clipboard,
        }
    }

    /// Checks whether `capability_key` is both declared in `aios-plugin.toml` AND granted by the user.
    pub fn ensure_capability(&self, capability_key: &str, declared: bool) -> Result<(), String> {
        if !declared {
            return Err(format!(
                "capability '{capability_key}' is not declared in aios-plugin.toml for '{}'",
                self.manifest.id
            ));
        }
        let db = self
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let granted = db
            .get_plugin_permission(&self.manifest.id, capability_key)
            .map_err(|e| format!("failed to query permission: {e}"))?;
        if granted == Some(true) {
            Ok(())
        } else {
            Err(format!(
                "capability '{capability_key}' has not been approved by user for '{}'. Approve it in Settings & Extensions.",
                self.manifest.id
            ))
        }
    }

    pub fn ensure_http_url(&self, url: &str) -> Result<(), String> {
        let matched = self
            .manifest
            .capabilities
            .http_hosts
            .iter()
            .find(|prefix| url.starts_with(prefix.as_str()));
        let Some(prefix) = matched else {
            return Err(format!(
                "HTTP host for '{url}' is not declared in capabilities.http_hosts"
            ));
        };
        let cap_key = format!("http:{prefix}");
        self.ensure_capability(&cap_key, true)
    }

    pub fn ensure_fs_read(&self, raw_path: &str) -> Result<PathBuf, String> {
        let target = normalize_abs_path(Path::new(raw_path))?;
        let matched = self.manifest.capabilities.fs_read.iter().find(|allowed| {
            normalize_abs_path(Path::new(allowed))
                .map(|base| target.starts_with(&base))
                .unwrap_or(false)
        });
        let Some(allowed_root) = matched else {
            return Err(format!(
                "path '{}' is outside declared capabilities.fs_read roots",
                target.display()
            ));
        };
        let cap_key = format!("fs_read:{allowed_root}");
        self.ensure_capability(&cap_key, true)?;
        Ok(target)
    }

    /// Resolves a preference value from SQLite or falls back to the manifest default.
    #[must_use]
    pub fn resolve_preference(&self, name: &str) -> Option<String> {
        if let Ok(db) = self.db.lock()
            && let Ok(Some(stored)) = db.get_plugin_preference(&self.manifest.id, name)
        {
            return Some(stored);
        }
        self.manifest
            .preferences
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.default.clone())
    }
}

/// Normalizes an absolute path by resolving `.` and `..` lexically (and canonicalizing if present)
/// to block directory traversal escapes out of allowed `fs_read` roots.
fn normalize_abs_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("fs_read path must be absolute: {}", path.display()));
    }
    if let Ok(canon) = std::fs::canonicalize(path) {
        return Ok(canon);
    }
    let mut normalized = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => normalized.push(p.as_os_str()),
            Component::RootDir => normalized.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

#[must_use]
pub fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
