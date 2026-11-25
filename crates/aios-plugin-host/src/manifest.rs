//! Manifest loader, plugin directory scanner, and local plugin installer/uninstaller.

use std::fs;
use std::path::{Path, PathBuf};

use aios_core::{AiosError, Database, Result};
use aios_plugin_sdk::{PluginManifest, PluginManifestFile};

pub const MANIFEST_FILE_NAME: &str = "aios-plugin.toml";

/// One installed external WASM plugin bundle discovered on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    pub root_dir: PathBuf,
    pub wasm_path: PathBuf,
    pub icon_path: Option<PathBuf>,
}

impl InstalledPlugin {
    /// Returns the path to this plugin's `aios-plugin.toml`.
    #[must_use]
    pub fn manifest_path(&self) -> PathBuf {
        self.root_dir.join(MANIFEST_FILE_NAME)
    }
}

/// Loads and validates a single external plugin bundle directory containing `aios-plugin.toml`
/// and its compiled `.wasm` component entry file.
pub fn load_plugin_from_dir(dir: &Path) -> Result<InstalledPlugin> {
    let manifest_path = dir.join(MANIFEST_FILE_NAME);
    let raw = fs::read_to_string(&manifest_path).map_err(|source| AiosError::IoPath {
        path: manifest_path.clone(),
        source,
    })?;
    let parsed = PluginManifestFile::from_toml_str(&raw)
        .map_err(|msg| AiosError::InvalidConfig(format!("{}: {msg}", manifest_path.display())))?;
    let manifest = parsed.into_manifest();

    let wasm_path = dir.join(&manifest.entry);
    if !wasm_path.is_file() {
        return Err(AiosError::InvalidConfig(format!(
            "plugin '{}' entry file not found at {}",
            manifest.id,
            wasm_path.display()
        )));
    }

    let icon_path = manifest
        .icon
        .as_deref()
        .map(|rel| dir.join(rel))
        .filter(|p| p.is_file());

    Ok(InstalledPlugin {
        manifest,
        root_dir: dir.to_path_buf(),
        wasm_path,
        icon_path,
    })
}

/// Scans `plugins_dir` (`$XDG_DATA_HOME/aios/plugins/`) for valid installed WASM plugin bundles.
/// Invalid bundles are logged and skipped so one broken directory never prevents AIOS from starting.
#[must_use]
pub fn discover_plugins(plugins_dir: &Path) -> Vec<InstalledPlugin> {
    let Ok(entries) = fs::read_dir(plugins_dir) else {
        return Vec::new();
    };
    let mut plugins = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join(MANIFEST_FILE_NAME).is_file() {
            continue;
        }
        match load_plugin_from_dir(&path) {
            Ok(plugin) => plugins.push(plugin),
            Err(err) => {
                tracing::warn!(
                    dir = %path.display(),
                    error = %err,
                    "skipping invalid external plugin bundle"
                );
            }
        }
    }
    plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    plugins
}

/// Installs (or upgrades) a local plugin bundle directory into `plugins_dir/<plugin.id>/`.
pub fn install_plugin_from_dir(source_dir: &Path, plugins_dir: &Path) -> Result<InstalledPlugin> {
    let validated = load_plugin_from_dir(source_dir)?;
    let target_dir = plugins_dir.join(&validated.manifest.id);

    fs::create_dir_all(plugins_dir).map_err(|source| AiosError::IoPath {
        path: plugins_dir.to_path_buf(),
        source,
    })?;

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(|source| AiosError::IoPath {
            path: target_dir.clone(),
            source,
        })?;
    }
    copy_dir_recursive(source_dir, &target_dir)?;

    load_plugin_from_dir(&target_dir)
}

/// Uninstalls an external plugin bundle from `plugins_dir` and purges its SQLite state if provided.
pub fn uninstall_plugin(
    plugin_id: &str,
    plugins_dir: &Path,
    db: Option<&Database>,
) -> Result<bool> {
    if plugin_id.is_empty()
        || plugin_id.contains('/')
        || plugin_id.contains('\\')
        || plugin_id.contains("..")
    {
        return Err(AiosError::InvalidConfig(format!(
            "invalid plugin id '{plugin_id}'"
        )));
    }

    let target_dir = plugins_dir.join(plugin_id);
    let existed = if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(|source| AiosError::IoPath {
            path: target_dir,
            source,
        })?;
        true
    } else {
        false
    };

    if let Some(db) = db {
        db.purge_plugin_state(plugin_id)?;
    }

    Ok(existed)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).map_err(|source| AiosError::IoPath {
        path: dst.to_path_buf(),
        source,
    })?;
    let entries = fs::read_dir(src).map_err(|source| AiosError::IoPath {
        path: src.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| AiosError::IoPath {
            path: src.to_path_buf(),
            source,
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|source| AiosError::IoPath {
                path: dst_path,
                source,
            })?;
        }
    }
    Ok(())
}
