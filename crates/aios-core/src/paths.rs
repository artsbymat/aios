//! XDG Base Directory path resolution for AIOS.

use crate::error::{AiosError, Result};
use std::path::{Path, PathBuf};

const APP_DIR_NAME: &str = "aios";
const SOCKET_FILE_NAME: &str = "aios.sock";

/// Resolved XDG filesystem locations used by AIOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiosPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub runtime_socket: PathBuf,
}

impl AiosPaths {
    /// Resolves AIOS paths from the current process environment following the
    /// XDG Base Directory Specification.
    pub fn discover() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or(AiosError::MissingHomeDir)?;

        let xdg_config = non_empty_abs_env("XDG_CONFIG_HOME");
        let xdg_data = non_empty_abs_env("XDG_DATA_HOME");
        let xdg_cache = non_empty_abs_env("XDG_CACHE_HOME");
        let xdg_runtime = non_empty_abs_env("XDG_RUNTIME_DIR");

        Ok(Self::from_roots(
            &home,
            xdg_config.as_deref(),
            xdg_data.as_deref(),
            xdg_cache.as_deref(),
            xdg_runtime.as_deref(),
        ))
    }

    /// Deterministic path constructor for testing and custom portable overrides.
    pub fn from_roots(
        home: &Path,
        xdg_config_home: Option<&Path>,
        xdg_data_home: Option<&Path>,
        xdg_cache_home: Option<&Path>,
        xdg_runtime_dir: Option<&Path>,
    ) -> Self {
        let config_base = xdg_config_home
            .filter(|p| p.is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));

        let data_base = xdg_data_home
            .filter(|p| p.is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));

        let cache_base = xdg_cache_home
            .filter(|p| p.is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));

        let runtime_base = xdg_runtime_dir
            .filter(|p| p.is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);

        Self {
            config_dir: config_base.join(APP_DIR_NAME),
            data_dir: data_base.join(APP_DIR_NAME),
            cache_dir: cache_base.join(APP_DIR_NAME),
            runtime_socket: runtime_base.join(SOCKET_FILE_NAME),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.config_dir.join("scripts")
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("aios.db")
    }

    pub fn blobs_dir(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    pub fn icons_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("icons")
    }

    /// Creates all persistent XDG directories required by AIOS if missing.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.scripts_dir(),
            &self.data_dir,
            &self.blobs_dir(),
            &self.plugins_dir(),
            &self.cache_dir,
            &self.icons_cache_dir(),
        ] {
            std::fs::create_dir_all(dir).map_err(|source| AiosError::IoPath {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

fn non_empty_abs_env(key: &str) -> Option<PathBuf> {
    let val = std::env::var_os(key)?;
    if val.is_empty() {
        return None;
    }
    let path = PathBuf::from(val);
    if path.is_absolute() { Some(path) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolves_default_xdg_paths_from_home() {
        let home = Path::new("/home/tester");
        let paths =
            AiosPaths::from_roots(home, None, None, None, Some(Path::new("/run/user/1000")));

        assert_eq!(paths.config_dir, PathBuf::from("/home/tester/.config/aios"));
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/home/tester/.local/share/aios")
        );
        assert_eq!(paths.cache_dir, PathBuf::from("/home/tester/.cache/aios"));
        assert_eq!(
            paths.runtime_socket,
            PathBuf::from("/run/user/1000/aios.sock")
        );
        assert_eq!(
            paths.config_file(),
            PathBuf::from("/home/tester/.config/aios/config.toml")
        );
        assert_eq!(
            paths.database_file(),
            PathBuf::from("/home/tester/.local/share/aios/aios.db")
        );
    }

    #[test]
    fn honors_custom_xdg_overrides_and_ignores_relative_paths() {
        let home = Path::new("/home/tester");
        let paths = AiosPaths::from_roots(
            home,
            Some(Path::new("/custom/cfg")),
            Some(Path::new("relative/ignored")),
            Some(Path::new("/custom/cache")),
            Some(Path::new("/run/custom")),
        );

        assert_eq!(paths.config_dir, PathBuf::from("/custom/cfg/aios"));
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/home/tester/.local/share/aios")
        );
        assert_eq!(paths.cache_dir, PathBuf::from("/custom/cache/aios"));
        assert_eq!(paths.runtime_socket, PathBuf::from("/run/custom/aios.sock"));
    }

    #[test]
    fn ensure_dirs_creates_full_hierarchy() {
        let tmp = TempDir::new().expect("temp dir");
        let home = tmp.path().join("home");
        let runtime = tmp.path().join("run");
        let paths = AiosPaths::from_roots(&home, None, None, None, Some(&runtime));

        paths.ensure_dirs().expect("create xdg directories");

        assert!(paths.config_dir.is_dir());
        assert!(paths.scripts_dir().is_dir());
        assert!(paths.data_dir.is_dir());
        assert!(paths.blobs_dir().is_dir());
        assert!(paths.plugins_dir().is_dir());
        assert!(paths.cache_dir.is_dir());
        assert!(paths.icons_cache_dir().is_dir());
    }
}
