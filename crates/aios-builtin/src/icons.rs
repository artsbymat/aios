//! XDG Icon Theme resolver for desktop application PNG and SVG icons.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Cached XDG icon theme resolver targeting crisp 32-64px PNG or scalable SVG icons.
#[derive(Debug)]
pub struct IconResolver {
    theme_order: Vec<String>,
    icon_roots: Vec<PathBuf>,
    pixmap_dirs: Vec<PathBuf>,
    cache: HashMap<String, Option<PathBuf>>,
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
            cache: HashMap::new(),
        }
    }

    /// Resolves an XDG `Icon=` field (either absolute file path or theme icon name) to an
    /// existing `.png` or `.svg` file on disk.
    pub fn resolve(&mut self, icon_name: &str) -> Option<PathBuf> {
        let key = icon_name.trim();
        if key.is_empty() {
            return None;
        }
        if let Some(cached) = self.cache.get(key) {
            return cached.clone();
        }

        let resolved = self.resolve_uncached(key);
        self.cache.insert(key.to_owned(), resolved.clone());
        resolved
    }

    fn resolve_uncached(&self, icon_name: &str) -> Option<PathBuf> {
        let direct = Path::new(icon_name);
        if direct.is_absolute() && direct.is_file() && is_supported_icon_ext(direct) {
            return Some(direct.to_path_buf());
        }

        // Strip optional extension so `firefox.png` also resolves inside theme directories.
        let stem = icon_name
            .strip_suffix(".png")
            .or_else(|| icon_name.strip_suffix(".svg"))
            .unwrap_or(icon_name);

        // Preferred crisp sizes for 24px launcher badges (prefer PNG 48/64/32 or scalable SVG).
        const SUBDIRS: &[&str] = &[
            "48x48/apps",
            "64x64/apps",
            "scalable/apps",
            "128x128/apps",
            "32x32/apps",
            "256x256/apps",
            "48x48/categories",
            "scalable/categories",
            "48x48/devices",
            "scalable/devices",
            "48x48/places",
            "scalable/places",
            "apps/scalable",
            "apps/48",
            "apps/64",
            "apps/32",
        ];
        const EXTS: &[&str] = &["png", "svg"];

        for theme in &self.theme_order {
            for root in &self.icon_roots {
                let theme_dir = root.join(theme);
                if !theme_dir.is_dir() {
                    continue;
                }
                for sub in SUBDIRS {
                    let dir = theme_dir.join(sub);
                    if !dir.is_dir() {
                        continue;
                    }
                    for ext in EXTS {
                        let candidate = dir.join(format!("{stem}.{ext}"));
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }

        // Fallback to `/usr/share/pixmaps` and Flatpak top-level icon folders.
        for pix in &self.pixmap_dirs {
            if !pix.is_dir() {
                continue;
            }
            for ext in EXTS {
                let candidate = pix.join(format!("{stem}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
            let exact = pix.join(icon_name);
            if exact.is_file() && is_supported_icon_ext(&exact) {
                return Some(exact);
            }
        }

        None
    }
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
