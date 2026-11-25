//! TOML configuration loader and writer for AIOS (`config.toml`).

use crate::error::{AiosError, Result};
use crate::paths::AiosPaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// User-selected theme preference supporting Dark, Light, and System modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

/// Concrete resolved theme palette after applying `System` portal detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedTheme {
    Dark,
    Light,
}

impl ThemeMode {
    pub fn resolve(self, system_prefers_dark: bool) -> ResolvedTheme {
        match self {
            Self::Dark => ResolvedTheme::Dark,
            Self::Light => ResolvedTheme::Light,
            Self::System => {
                if system_prefers_dark {
                    ResolvedTheme::Dark
                } else {
                    ResolvedTheme::Light
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub mode: ThemeMode,
    pub translucent_root: bool,
    pub reduced_motion: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: ThemeMode::System,
            translucent_root: true,
            reduced_motion: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub default_height: u32,
    pub max_height: u32,
    pub compact_mode: bool,
    pub pop_to_root_seconds: u64,
    pub top_offset_percent: u8,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 750,
            default_height: 480,
            max_height: 620,
            compact_mode: false,
            pop_to_root_seconds: 60,
            top_offset_percent: 18,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub retention_days: u32,
    pub ignored_apps: Vec<String>,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 1000,
            retention_days: 30,
            ignored_apps: vec![
                "org.keepassxc.KeePassXC".to_string(),
                "bitwarden".to_string(),
                "1password".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FallbackCommandConfig {
    pub id: String,
    pub title: String,
    pub template: String,
}

impl Default for FallbackCommandConfig {
    fn default() -> Self {
        Self {
            id: "builtin.files".to_string(),
            title: "Search Local Files".to_string(),
            template: "aios://builtin/files/search?query={query}".to_string(),
        }
    }
}

/// Top-level user configuration persisted at `$XDG_CONFIG_HOME/aios/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiosConfig {
    pub theme: ThemeConfig,
    pub window: WindowConfig,
    pub clipboard: ClipboardConfig,
    pub aliases: BTreeMap<String, String>,
    pub hotkeys: BTreeMap<String, String>,
    pub fallback_commands: Vec<FallbackCommandConfig>,
}

impl Default for AiosConfig {
    fn default() -> Self {
        let mut aliases = BTreeMap::new();
        aliases.insert("cb".to_string(), "aios.clipboard:history".to_string());
        aliases.insert("sn".to_string(), "aios.snippets:search".to_string());

        Self {
            theme: ThemeConfig::default(),
            window: WindowConfig::default(),
            clipboard: ClipboardConfig::default(),
            aliases,
            hotkeys: BTreeMap::new(),
            fallback_commands: vec![FallbackCommandConfig::default()],
        }
    }
}

impl AiosConfig {
    /// Loads configuration from `paths.config_file()`, returning defaults when the file does not exist yet.
    pub fn load_or_default(paths: &AiosPaths) -> Result<Self> {
        let config_path = paths.config_file();
        if !config_path.exists() {
            return Ok(Self::default());
        }
        Self::load_from_path(&config_path)
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|source| AiosError::IoPath {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self = toml::from_str(&raw).map_err(|source| AiosError::ConfigParse {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Writes configuration atomically via a sibling temp file and rename.
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AiosError::IoPath {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let serialized = toml::to_string_pretty(self)?;
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, serialized).map_err(|source| AiosError::IoPath {
            path: tmp_path.clone(),
            source,
        })?;
        std::fs::rename(&tmp_path, path).map_err(|source| AiosError::IoPath {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.window.width < 480 || self.window.width > 1600 {
            return Err(AiosError::InvalidConfig(format!(
                "window.width ({}) must be between 480 and 1600",
                self.window.width
            )));
        }
        if self.window.default_height > self.window.max_height {
            return Err(AiosError::InvalidConfig(format!(
                "window.default_height ({}) cannot exceed window.max_height ({})",
                self.window.default_height, self.window.max_height
            )));
        }
        if self.window.top_offset_percent > 50 {
            return Err(AiosError::InvalidConfig(format!(
                "window.top_offset_percent ({}) must be <= 50",
                self.window.top_offset_percent
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn config_round_trip_preserves_all_fields() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("aios").join("config.toml");

        let mut cfg = AiosConfig::default();
        cfg.theme.mode = ThemeMode::Light;
        cfg.theme.reduced_motion = true;
        cfg.window.compact_mode = true;
        cfg.window.width = 800;
        cfg.aliases
            .insert("wm".to_string(), "aios.window-management:list".to_string());
        cfg.hotkeys.insert(
            "aios.clipboard:history".to_string(),
            "Super+Shift+V".to_string(),
        );

        cfg.save_to_path(&path).expect("save config");
        let reloaded = AiosConfig::load_from_path(&path).expect("reload config");

        assert_eq!(cfg, reloaded);
    }

    #[test]
    fn missing_config_returns_defaults() {
        let tmp = TempDir::new().expect("temp dir");
        let paths = AiosPaths::from_roots(tmp.path(), None, None, None, None);

        let cfg = AiosConfig::load_or_default(&paths).expect("default config");
        assert_eq!(cfg, AiosConfig::default());
        assert_eq!(cfg.theme.mode, ThemeMode::System);
        assert_eq!(cfg.theme.mode.resolve(true), ResolvedTheme::Dark);
        assert_eq!(cfg.theme.mode.resolve(false), ResolvedTheme::Light);
    }

    #[test]
    fn partial_toml_fills_missing_fields_with_defaults() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[theme]\nmode = \"dark\"\n").expect("write partial toml");

        let cfg = AiosConfig::load_from_path(&path).expect("load partial config");
        assert_eq!(cfg.theme.mode, ThemeMode::Dark);
        assert_eq!(cfg.window, WindowConfig::default());
        assert_eq!(cfg.clipboard, ClipboardConfig::default());
    }

    #[test]
    fn rejects_invalid_window_bounds() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("config.toml");
        let mut cfg = AiosConfig::default();
        cfg.window.width = 200;

        let err = cfg
            .save_to_path(&path)
            .expect_err("must reject narrow width");
        assert!(matches!(err, AiosError::InvalidConfig(_)));
    }
}
