//! Zero-cost GitHub-native Plugin Registry client, index builder, and remote installer.
//!
//! Architecture:
//! - Registry index (`index.json`) is hosted statically on GitHub Pages / jsDelivr / raw GitHub
//!   (or a local file path for development/testing) and cached offline-first in SQLite.
//! - Plugin binaries (`.aios-plugin` tarballs) are hosted on each author's GitHub Releases and
//!   verified via mandatory SHA-256 before extraction.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aios_core::Database;
use serde::{Deserialize, Serialize};

use crate::archive::{
    MAX_COMPRESSED_ARCHIVE_BYTES, PLUGIN_ARCHIVE_EXT, install_plugin_archive_bytes,
};
use crate::manifest::InstalledPlugin;
use crate::permissions::now_epoch_secs;

const REGISTRY_CACHE_NS: &str = "__aios_registry__";
const REGISTRY_CACHE_KEY: &str = "index_v1";
const HTTP_TIMEOUT_SECS: u64 = 15;

/// Root JSON document published as `registry/index.json` on GitHub Pages / jsDelivr CDN.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRegistryIndex {
    pub schema_version: u32,
    pub generated_at: i64,
    #[serde(default)]
    pub plugins: Vec<RegistryPluginEntry>,
}

/// One plugin catalog entry parsed from `registry/plugins/<id>.toml` and compiled into `index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryPluginEntry {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    #[serde(default)]
    pub repository: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub latest: RegistryRelease,
}

fn default_category() -> String {
    "utilities".to_string()
}

/// Latest downloadable release metadata for a registry plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryRelease {
    pub version: String,
    #[serde(default)]
    pub min_aios_version: Option<String>,
    pub download_url: String,
    pub sha256: String,
    #[serde(default)]
    pub icon_url: Option<String>,
}

/// Compiles all `*.toml` entries inside `<registry_dir>/plugins/` into a sorted [`PluginRegistryIndex`].
pub fn build_registry_index_from_dir(registry_dir: &Path) -> Result<PluginRegistryIndex, String> {
    let plugins_dir = registry_dir.join("plugins");
    let entries = fs::read_dir(&plugins_dir)
        .map_err(|e| format!("failed to read '{}': {e}", plugins_dir.display()))?;

    let mut plugins = Vec::new();
    let mut seen_ids = HashMap::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read '{}': {e}", path.display()))?;
        let parsed: RegistryPluginEntry = toml::from_str(&raw)
            .map_err(|e| format!("invalid registry entry '{}': {e}", path.display()))?;
        validate_registry_entry(&parsed)?;
        if seen_ids.insert(parsed.id.clone(), path.clone()).is_some() {
            return Err(format!("duplicate plugin id '{}' in registry", parsed.id));
        }
        plugins.push(parsed);
    }

    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(PluginRegistryIndex {
        schema_version: 1,
        generated_at: now_epoch_secs(),
        plugins,
    })
}

/// Validates schema invariants for a registry TOML entry (used by both CI PR checks and local builds).
pub fn validate_registry_entry(entry: &RegistryPluginEntry) -> Result<(), String> {
    if entry.id.trim().is_empty()
        || !entry
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!("invalid plugin id '{}'", entry.id));
    }
    if entry.name.trim().is_empty() {
        return Err(format!("plugin '{}' must have a non-empty name", entry.id));
    }
    if entry.latest.version.trim().is_empty() {
        return Err(format!("plugin '{}' must specify latest.version", entry.id));
    }
    if entry.latest.download_url.trim().is_empty() {
        return Err(format!(
            "plugin '{}' must specify latest.download_url",
            entry.id
        ));
    }
    let sha = entry.latest.sha256.trim();
    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "plugin '{}' latest.sha256 must be a 64-char hex SHA-256 digest",
            entry.id
        ));
    }
    Ok(())
}

/// Loads the cached registry index from SQLite if present (zero network I/O).
#[must_use]
pub fn load_cached_registry_index(db: &Arc<Mutex<Database>>) -> Option<PluginRegistryIndex> {
    let guard = db.lock().ok()?;
    let bytes = guard
        .plugin_storage_get(REGISTRY_CACHE_NS, REGISTRY_CACHE_KEY)
        .ok()
        .flatten()?;
    serde_json::from_slice::<PluginRegistryIndex>(&bytes).ok()
}

/// Saves a merged registry index into SQLite cache.
pub fn save_cached_registry_index(
    db: &Arc<Mutex<Database>>,
    index: &PluginRegistryIndex,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(index).map_err(|e| e.to_string())?;
    let guard = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    guard
        .plugin_storage_set(
            REGISTRY_CACHE_NS,
            REGISTRY_CACHE_KEY,
            &bytes,
            now_epoch_secs(),
        )
        .map_err(|e| e.to_string())
}

/// Fetches and merges `index.json` across all configured `registry_urls` (supports `https://`,
/// `http://`, `file://`, and direct local paths, plus fallback to local `./registry/index.json`).
pub fn fetch_registry_index(
    registry_urls: &[String],
    db: &Arc<Mutex<Database>>,
    ttl_hours: u64,
    force_refresh: bool,
) -> Result<PluginRegistryIndex, String> {
    let now = now_epoch_secs();
    if !force_refresh
        && let Some(cached) = load_cached_registry_index(db)
        && ttl_hours > 0
        && now.saturating_sub(cached.generated_at)
            < i64::try_from(ttl_hours * 3_600).unwrap_or(i64::MAX)
    {
        return Ok(cached);
    }

    let mut merged: HashMap<String, RegistryPluginEntry> = HashMap::new();
    let mut errors = Vec::new();
    let mut fetched_any = false;

    for url in registry_urls {
        match load_single_registry_json(url) {
            Ok(idx) => {
                fetched_any = true;
                for p in idx.plugins {
                    merged.entry(p.id.clone()).or_insert(p);
                }
            }
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    // Local dev convenience fallback: if remote URLs failed (e.g. offline), check `./registry/index.json`
    // or compile `./registry/plugins/*.toml` on the fly.
    if !fetched_any {
        let local_registry = Path::new("registry");
        let local_dev_index = local_registry.join("index.json");
        if local_dev_index.is_file()
            && let Ok(raw) = fs::read_to_string(&local_dev_index)
            && let Ok(idx) = serde_json::from_str::<PluginRegistryIndex>(&raw)
        {
            fetched_any = true;
            for p in idx.plugins {
                merged.entry(p.id.clone()).or_insert(p);
            }
        } else if local_registry.join("plugins").is_dir()
            && let Ok(idx) = build_registry_index_from_dir(local_registry)
        {
            fetched_any = true;
            for p in idx.plugins {
                merged.entry(p.id.clone()).or_insert(p);
            }
        }
    }

    if !fetched_any {
        if let Some(stale) = load_cached_registry_index(db) {
            return Ok(stale);
        }
        return Err(errors
            .into_iter()
            .last()
            .unwrap_or_else(|| "no registry URLs configured".to_string()));
    }

    let mut plugins: Vec<RegistryPluginEntry> = merged.into_values().collect();
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    let index = PluginRegistryIndex {
        schema_version: 1,
        generated_at: now,
        plugins,
    };
    let _ = save_cached_registry_index(db, &index);
    Ok(index)
}

/// Downloads, verifies SHA-256, and installs a plugin listed in [`PluginRegistryIndex`].
pub fn install_plugin_from_registry_entry(
    entry: &RegistryPluginEntry,
    plugins_dir: &Path,
    db: &Arc<Mutex<Database>>,
) -> Result<InstalledPlugin, String> {
    let bytes = fetch_binary_resource(&entry.latest.download_url)?;
    let (installed, actual_sha) =
        install_plugin_archive_bytes(&bytes, Some(&entry.latest.sha256), plugins_dir)
            .map_err(|e| e.to_string())?;

    if installed.manifest.id != entry.id {
        let _ = fs::remove_dir_all(&installed.root_dir);
        return Err(format!(
            "archive plugin ID '{}' does not match registry ID '{}'",
            installed.manifest.id, entry.id
        ));
    }

    if let Ok(guard) = db.lock() {
        let _ = guard.record_plugin_install_meta(
            &installed.manifest.id,
            "registry",
            &installed.manifest.version,
            Some(&actual_sha),
            now_epoch_secs(),
        );
    }
    Ok(installed)
}

/// Installs a plugin directly from a local `.aios-plugin` archive file (100% offline).
pub fn install_plugin_from_archive_file(
    archive_path: &Path,
    expected_sha256: Option<&str>,
    plugins_dir: &Path,
    db: &Arc<Mutex<Database>>,
) -> Result<InstalledPlugin, String> {
    let bytes = fs::read(archive_path)
        .map_err(|e| format!("failed to read '{}': {e}", archive_path.display()))?;
    let (installed, actual_sha) =
        install_plugin_archive_bytes(&bytes, expected_sha256, plugins_dir)
            .map_err(|e| e.to_string())?;
    if let Ok(guard) = db.lock() {
        let _ = guard.record_plugin_install_meta(
            &installed.manifest.id,
            "archive",
            &installed.manifest.version,
            Some(&actual_sha),
            now_epoch_secs(),
        );
    }
    Ok(installed)
}

/// Installs the latest `.aios-plugin` release directly from `github:<owner>/<repo>` without
/// requiring the plugin to be listed in the central registry.
pub fn install_plugin_from_github(
    owner_repo: &str,
    plugins_dir: &Path,
    db: &Arc<Mutex<Database>>,
) -> Result<InstalledPlugin, String> {
    let cleaned = owner_repo
        .trim()
        .trim_start_matches("github:")
        .trim_matches('/');
    let (owner, repo) = cleaned
        .split_once('/')
        .filter(|(o, r)| !o.is_empty() && !r.is_empty() && !r.contains('/'))
        .ok_or_else(|| format!("expected 'github:<owner>/<repo>', got '{owner_repo}'"))?;

    let api_url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let body = fetch_text_resource(&api_url)?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("invalid GitHub release JSON: {e}"))?;
    let assets = json
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "GitHub release contains no assets".to_string())?;

    let archive_asset = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|n| n.ends_with(&format!(".{PLUGIN_ARCHIVE_EXT}")))
        })
        .ok_or_else(|| {
            format!(
                "latest release of {owner}/{repo} does not contain a '.{PLUGIN_ARCHIVE_EXT}' asset"
            )
        })?;
    let archive_name = archive_asset
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let download_url = archive_asset
        .get("browser_download_url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "missing browser_download_url in GitHub release asset".to_string())?;

    // Optional SHA256SUMS asset verification if published alongside the archive.
    let expected_sha = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case("SHA256SUMS") || n.ends_with(".sha256"))
        })
        .and_then(|a| {
            a.get("browser_download_url")
                .and_then(serde_json::Value::as_str)
        })
        .and_then(|url| fetch_text_resource(url).ok())
        .and_then(|sums| parse_sha256sums_for_file(&sums, archive_name));

    let bytes = fetch_binary_resource(download_url)?;
    let (installed, actual_sha) =
        install_plugin_archive_bytes(&bytes, expected_sha.as_deref(), plugins_dir)
            .map_err(|e| e.to_string())?;
    if let Ok(guard) = db.lock() {
        let _ = guard.record_plugin_install_meta(
            &installed.manifest.id,
            &format!("github:{owner}/{repo}"),
            &installed.manifest.version,
            Some(&actual_sha),
            now_epoch_secs(),
        );
    }
    Ok(installed)
}

fn parse_sha256sums_for_file(raw: &str, filename: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let hash = parts.next()?;
        if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            let file_part = parts.next().unwrap_or("");
            if file_part.is_empty() || file_part.ends_with(filename) {
                return Some(hash.to_ascii_lowercase());
            }
        }
    }
    None
}

/// Returns `true` when `available` is strictly newer than `installed` using `major.minor.patch` semver.
#[must_use]
pub fn is_newer_version(installed: &str, available: &str) -> bool {
    parse_semver_tuple(available) > parse_semver_tuple(installed)
}

fn parse_semver_tuple(raw: &str) -> (u64, u64, u64) {
    let core = raw
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or("");
    let mut parts = core.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

fn load_single_registry_json(url: &str) -> Result<PluginRegistryIndex, String> {
    let raw = fetch_text_resource(url)?;
    serde_json::from_str::<PluginRegistryIndex>(&raw)
        .map_err(|e| format!("invalid registry index.json: {e}"))
}

fn fetch_text_resource(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read_to_string(path).map_err(|e| format!("failed to read '{path}': {e}"));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return fs::read_to_string(url).map_err(|e| format!("failed to read '{url}': {e}"));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .user_agent("aios-plugin-registry/0.1")
        .build();
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.into_string().map_err(|e| e.to_string())
}

fn fetch_binary_resource(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read(path).map_err(|e| format!("failed to read '{path}': {e}"));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return fs::read(url).map_err(|e| format!("failed to read '{url}': {e}"));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .user_agent("aios-plugin-registry/0.1")
        .build();
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take((MAX_COMPRESSED_ARCHIVE_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() > MAX_COMPRESSED_ARCHIVE_BYTES {
        return Err(format!(
            "downloaded archive exceeds {} MiB limit",
            MAX_COMPRESSED_ARCHIVE_BYTES / (1024 * 1024)
        ));
    }
    Ok(buf)
}
