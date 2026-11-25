//! XDG Icon Theme resolver for desktop application PNG and SVG icons.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

/// Pre-indexed XDG icon theme resolver targeting crisp 24-64px PNG or scalable SVG icons.
///
/// Scans theme directories once on first lookup via `read_dir` into an O(1) hash map so
/// resolving thousands of icon names (e.g. `aios dmenu --show-icons`) takes milliseconds
/// instead of millions of `stat()` syscalls.
#[derive(Debug)]
pub struct IconResolver {
    theme_order: Vec<String>,
    icon_roots: Vec<PathBuf>,
    pixmap_dirs: Vec<PathBuf>,
    index: Option<HashMap<String, PathBuf>>,
}

impl Default for IconResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl IconResolver {
    #[must_use]
    pub fn new() -> Self {
        let mut theme_order = Vec::new();
        if let Some(active) = detect_active_gtk_icon_theme() {
            theme_order.push(active);
        }
        for fallback in ["Papirus-Dark", "Papirus", "Adwaita", "breeze", "hicolor"] {
            if !theme_order.iter().any(|t| t.eq_ignore_ascii_case(fallback)) {
                theme_order.push(fallback.to_string());
            }
        }

        let (icon_roots, pixmap_dirs) = discover_icon_roots();
        Self {
            theme_order,
            icon_roots,
            pixmap_dirs,
            index: None,
        }
    }

    /// Resolves an XDG `Icon=` field (either absolute file path or theme icon name) to an
    /// existing `.png` or `.svg` file on disk in O(1) amortized time.
    pub fn resolve(&mut self, icon_name: &str) -> Option<PathBuf> {
        let key = icon_name.trim();
        if key.is_empty() {
            return None;
        }

        let direct = Path::new(key);
        if direct.is_absolute() && direct.is_file() && is_supported_icon_ext(direct) {
            return Some(direct.to_path_buf());
        }

        let stem = key
            .strip_suffix(".png")
            .or_else(|| key.strip_suffix(".svg"))
            .unwrap_or(key);

        let index = self.ensure_index();
        index.get(stem).or_else(|| index.get(key)).cloned()
    }

    /// Pre-warms the icon index eagerly (useful on a background thread at daemon startup
    /// so subsequent `aios dmenu --show-icons` calls incur 0ms index overhead).
    pub fn warm_index(&mut self) {
        let _ = self.ensure_index();
    }

    fn ensure_index(&mut self) -> &HashMap<String, PathBuf> {
        if self.index.is_none() {
            self.index = Some(self.build_index());
        }
        self.index.as_ref().expect("index initialized")
    }

    fn build_index(&self) -> HashMap<String, PathBuf> {
        // Preferred crisp sizes for 24px launcher badges (prefer PNG 48/64/32 or scalable SVG).
        const SUBDIRS: &[&str] = &[
            "48x48/apps",
            "64x64/apps",
            "scalable/apps",
            "128x128/apps",
            "32x32/apps",
            "256x256/apps",
            "48x48/actions",
            "32x32/actions",
            "24x24/actions",
            "scalable/actions",
            "48x48/categories",
            "scalable/categories",
            "48x48/devices",
            "24x24/devices",
            "scalable/devices",
            "48x48/places",
            "24x24/places",
            "scalable/places",
            "48x48/status",
            "24x24/status",
            "scalable/status",
            "48x48/mimetypes",
            "scalable/mimetypes",
            "apps/scalable",
            "apps/48",
            "apps/64",
            "apps/32",
            "actions/32",
            "actions/24",
            "actions/22",
            "devices/24",
            "places/24",
            "status/24",
        ];

        // Collect all existing candidate directories in strict theme/size priority order.
        let mut dirs = Vec::with_capacity(64);
        for theme in &self.theme_order {
            for root in &self.icon_roots {
                let theme_dir = root.join(theme);
                if !theme_dir.is_dir() {
                    continue;
                }
                for sub in SUBDIRS {
                    let dir = theme_dir.join(sub);
                    if dir.is_dir() {
                        dirs.push(dir);
                    }
                }
            }
        }
        for pix in &self.pixmap_dirs {
            if pix.is_dir() {
                dirs.push(pix.clone());
            }
        }

        // Scan every directory in parallel across CPU cores, then merge in strict priority order.
        let scanned: Vec<Vec<(String, PathBuf, bool)>> =
            dirs.par_iter().map(|dir| scan_icon_dir(dir)).collect();

        let total_hint: usize = scanned.iter().map(Vec::len).sum();
        let mut map: HashMap<String, PathBuf> = HashMap::with_capacity(total_hint.min(16_384));
        for entries in scanned {
            for (stem, path, is_png) in entries {
                match map.get(&stem) {
                    None => {
                        map.insert(stem, path);
                    }
                    Some(existing) if is_png && existing.parent() == path.parent() => {
                        if existing
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
                        {
                            map.insert(stem, path);
                        }
                    }
                    Some(_) => {}
                }
            }
        }

        map
    }
}

fn scan_icon_dir(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let is_png = ext.eq_ignore_ascii_case("png");
        let is_svg = ext.eq_ignore_ascii_case("svg");
        if !is_png && !is_svg {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        out.push((stem.to_owned(), path, is_png));
    }
    out
}

fn is_supported_icon_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "svg" | "jpg" | "webp"
            )
        })
}

fn detect_active_gtk_icon_theme() -> Option<String> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;

    for sub in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        let path = config_home.join(sub);
        let Ok(raw) = fs::read_to_string(path) else {
            continue;
        };
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some((k, v)) = trimmed.split_once('=')
                && k.trim() == "gtk-icon-theme-name"
            {
                let theme = v.trim().trim_matches('"').trim_matches('\'');
                if !theme.is_empty() {
                    return Some(theme.to_owned());
                }
            }
        }
    }
    None
}

fn discover_icon_roots() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut icon_roots = Vec::new();
    let mut pixmaps = Vec::new();

    let home = env::var_os("HOME").map(PathBuf::from);
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));

    if let Some(ref h) = home {
        icon_roots.push(h.join(".icons"));
    }
    if let Some(ref dh) = data_home {
        icon_roots.push(dh.join("icons"));
        icon_roots.push(dh.join("flatpak/exports/share/icons"));
    }
    icon_roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));

    let data_dirs = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());

    for segment in data_dirs.split(':') {
        let p = PathBuf::from(segment);
        if p.is_absolute() {
            icon_roots.push(p.join("icons"));
            pixmaps.push(p.join("pixmaps"));
        }
    }

    (icon_roots, pixmaps)
}
