//! Polling hot-reload watcher for local plugin development (`aios plugin dev <path>`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::manifest::{InstalledPlugin, MANIFEST_FILE_NAME, load_plugin_from_dir};

/// Event emitted when a watched local plugin bundle changes on disk.
#[derive(Debug, Clone)]
pub enum DevReloadEvent {
    Reloaded(Box<InstalledPlugin>),
    InvalidBundle(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

/// Lightweight cross-compositor file watcher tracking `aios-plugin.toml` and `plugin.wasm`
/// timestamps so `aios plugin dev <path>` hot-reloads without external OS watcher flakes.
#[derive(Debug)]
pub struct PluginDevWatcher {
    dev_dir: PathBuf,
    manifest_stamp: Option<FileStamp>,
    wasm_path: Option<PathBuf>,
    wasm_stamp: Option<FileStamp>,
    poll_interval: Duration,
}

impl PluginDevWatcher {
    pub fn new(dev_dir: &Path, poll_interval: Duration) -> Self {
        let mut watcher = Self {
            dev_dir: dev_dir.to_path_buf(),
            manifest_stamp: None,
            wasm_path: None,
            wasm_stamp: None,
            poll_interval,
        };
        let _ = watcher.snapshot_current();
        watcher
    }

    #[must_use]
    pub fn dev_dir(&self) -> &Path {
        &self.dev_dir
    }

    #[must_use]
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// Loads the current bundle immediately (used for initial dev startup).
    pub fn load_initial(&mut self) -> Result<InstalledPlugin, String> {
        let bundle = load_plugin_from_dir(&self.dev_dir).map_err(|e| e.to_string())?;
        self.manifest_stamp = stamp_of(&bundle.manifest_path());
        self.wasm_path = Some(bundle.wasm_path.clone());
        self.wasm_stamp = stamp_of(&bundle.wasm_path);
        Ok(bundle)
    }

    /// Checks whether `aios-plugin.toml` or the entry `.wasm` changed since the last check.
    pub fn poll_change(&mut self) -> Option<DevReloadEvent> {
        let manifest_path = self.dev_dir.join(MANIFEST_FILE_NAME);
        let new_manifest_stamp = stamp_of(&manifest_path);
        let new_wasm_stamp = self.wasm_path.as_deref().and_then(stamp_of);

        let changed =
            new_manifest_stamp != self.manifest_stamp || new_wasm_stamp != self.wasm_stamp;
        if !changed {
            return None;
        }

        self.manifest_stamp = new_manifest_stamp;
        match load_plugin_from_dir(&self.dev_dir) {
            Ok(bundle) => {
                self.wasm_path = Some(bundle.wasm_path.clone());
                self.wasm_stamp = stamp_of(&bundle.wasm_path);
                Some(DevReloadEvent::Reloaded(Box::new(bundle)))
            }
            Err(err) => {
                self.wasm_stamp = new_wasm_stamp;
                Some(DevReloadEvent::InvalidBundle(err.to_string()))
            }
        }
    }

    fn snapshot_current(&mut self) -> Option<InstalledPlugin> {
        let manifest_path = self.dev_dir.join(MANIFEST_FILE_NAME);
        self.manifest_stamp = stamp_of(&manifest_path);
        if let Ok(bundle) = load_plugin_from_dir(&self.dev_dir) {
            self.wasm_path = Some(bundle.wasm_path.clone());
            self.wasm_stamp = stamp_of(&bundle.wasm_path);
            Some(bundle)
        } else {
            None
        }
    }
}

fn stamp_of(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}
