//! XDG Desktop Entry (`.desktop`) scanner, Flatpak/AppImage discovery, and detached launcher.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aios_core::{AiosError, Result};

use crate::icons::IconResolver;

/// Origin of a discovered desktop application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSource {
    System,
    User,
    Flatpak,
    AppImage,
}

impl AppSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Application",
            Self::User => "User App",
            Self::Flatpak => "Flatpak",
            Self::AppImage => "AppImage",
        }
    }
}

/// Parsed desktop application entry ready for Root Search indexing and launching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopApp {
    /// Stable ID (`app:<desktop-id>`, e.g. `app:firefox.desktop`).
    pub id: String,
    /// Freedesktop desktop file ID (e.g. `firefox.desktop` or `org.mozilla.firefox.desktop`).
    pub desktop_id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub exec_raw: String,
    pub exec_program: String,
    pub exec_args: Vec<String>,
    pub icon_name: Option<String>,
    pub icon_path: Option<PathBuf>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub terminal: bool,
    pub working_dir: Option<PathBuf>,
    pub desktop_file_path: PathBuf,
    pub source: AppSource,
}

/// Scans standard XDG application directories (User, Flatpak, System) plus `~/Applications`
/// AppImages, resolving desktop ID shadowing according to the XDG Desktop Entry Spec.
#[must_use]
pub fn scan_desktop_applications() -> Vec<DesktopApp> {
    let dirs = discover_application_dirs();
    let appimage_dirs = discover_appimage_dirs();
    let mut icon_resolver = IconResolver::new();
    scan_desktop_applications_in(&dirs, &appimage_dirs, &mut icon_resolver)
}

/// Scans explicit lists of `.desktop` directories (highest priority first) and AppImage directories.
#[must_use]
pub fn scan_desktop_applications_in(
    desktop_dirs: &[(PathBuf, AppSource)],
    appimage_dirs: &[PathBuf],
    icon_resolver: &mut IconResolver,
) -> Vec<DesktopApp> {
    let mut by_desktop_id: BTreeMap<String, DesktopApp> = BTreeMap::new();
    let mut shadowed_hidden: HashSet<String> = HashSet::new();

    for (dir, source) in desktop_dirs {
        if !dir.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_desktop_files(dir, dir, &mut files);
        files.sort();

        for (desktop_id, path) in files {
            if by_desktop_id.contains_key(&desktop_id) || shadowed_hidden.contains(&desktop_id) {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            match parse_desktop_entry(&desktop_id, &path, *source, &raw, icon_resolver) {
                ParsedEntry::App(app) => {
                    by_desktop_id.insert(desktop_id, *app);
                }
                ParsedEntry::HiddenOrNoDisplay => {
                    // User override marking Hidden=true / NoDisplay=true suppresses lower-priority copies.
                    shadowed_hidden.insert(desktop_id);
                }
                ParsedEntry::Skip => {}
            }
        }
    }

    // Discover executable AppImages in ~/Applications and ~/.local/bin
    for dir in appimage_dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if !file_name.to_ascii_lowercase().ends_with(".appimage") {
                continue;
            }
            if !is_executable(&path) {
                continue;
            }
            let desktop_id = format!("appimage:{file_name}");
            if by_desktop_id.contains_key(&desktop_id) {
                continue;
            }
            let clean_name = file_name
                .trim_end_matches(".AppImage")
                .trim_end_matches(".appimage")
                .replace(['-', '_'], " ");
            let stem = path
                .file_stem()
                .and_then(OsStr::to_str)
                .map_or_else(|| clean_name.clone(), ToOwned::to_owned);
            let icon_path = icon_resolver.resolve(&stem);
            let exec_str = path.to_string_lossy().into_owned();
            by_desktop_id.insert(
                desktop_id.clone(),
                DesktopApp {
                    id: format!("app:{desktop_id}"),
                    desktop_id,
                    name: clean_name,
                    generic_name: Some("AppImage".to_string()),
                    comment: Some(path.display().to_string()),
                    exec_raw: exec_str.clone(),
                    exec_program: exec_str,
                    exec_args: Vec::new(),
                    icon_name: Some(stem),
                    icon_path,
                    keywords: vec!["appimage".to_string()],
                    categories: vec!["AppImage".to_string()],
                    terminal: false,
                    working_dir: path.parent().map(Path::to_path_buf),
                    desktop_file_path: path,
                    source: AppSource::AppImage,
                },
            );
        }
    }

    let mut apps: Vec<DesktopApp> = by_desktop_id.into_values().collect();
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    apps
}

fn discover_application_dirs() -> Vec<(PathBuf, AppSource)> {
    let mut dirs = Vec::new();
    let home = env::var_os("HOME").map(PathBuf::from);
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));

    if let Some(dh) = data_home {
        dirs.push((dh.join("applications"), AppSource::User));
        dirs.push((
            dh.join("flatpak/exports/share/applications"),
            AppSource::Flatpak,
        ));
    }

    dirs.push((
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        AppSource::Flatpak,
    ));

    let data_dirs_raw = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());

    for segment in data_dirs_raw.split(':') {
        let p = PathBuf::from(segment);
        if p.is_absolute() {
            let app_dir = p.join("applications");
            if !dirs.iter().any(|(existing, _)| existing == &app_dir) {
                let src = if segment.contains("flatpak") {
                    AppSource::Flatpak
                } else {
                    AppSource::System
                };
                dirs.push((app_dir, src));
            }
        }
    }

    dirs
}

fn discover_appimage_dirs() -> Vec<PathBuf> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    vec![home.join("Applications"), home.join(".local/bin")]
}

fn collect_desktop_files(root: &Path, current: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_files(root, &path, out);
        } else if path.extension().and_then(OsStr::to_str) == Some("desktop")
            && let Ok(rel) = path.strip_prefix(root)
        {
            // Per XDG Desktop Entry Spec, subdirectory separators become `-` in desktop IDs.
            let desktop_id = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("-");
            out.push((desktop_id, path));
        }
    }
}

enum ParsedEntry {
    App(Box<DesktopApp>),
    HiddenOrNoDisplay,
    Skip,
}

fn parse_desktop_entry(
    desktop_id: &str,
    path: &Path,
    source: AppSource,
    content: &str,
    icon_resolver: &mut IconResolver,
) -> ParsedEntry {
    let mut in_main_section = false;
    let mut entry_type: Option<String> = None;
    let mut name: Option<String> = None;
    let mut generic_name: Option<String> = None;
    let mut comment: Option<String> = None;
    let mut exec_raw: Option<String> = None;
    let mut try_exec: Option<String> = None;
    let mut icon_name: Option<String> = None;
    let mut keywords: Vec<String> = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    let mut terminal = false;
    let mut working_dir: Option<PathBuf> = None;
    let mut hidden = false;
    let mut no_display = false;
    let mut not_show_in: Vec<String> = Vec::new();
    let mut wm_class: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_main_section = line == "[Desktop Entry]";
            continue;
        }
        if !in_main_section {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        match key {
            "Type" => entry_type = Some(val.to_owned()),
            "Name" => name = Some(unescape_desktop_string(val)),
            "GenericName" => generic_name = Some(unescape_desktop_string(val)),
            "Comment" => comment = Some(unescape_desktop_string(val)),
            "Exec" => exec_raw = Some(val.to_owned()),
            "TryExec" => try_exec = Some(val.to_owned()),
            "Icon" => {
                if !val.is_empty() {
                    icon_name = Some(val.to_owned());
                }
            }
            "Keywords" => {
                keywords = val
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "Categories" => {
                categories = val
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "Terminal" => terminal = val.eq_ignore_ascii_case("true"),
            "Path" => {
                if !val.is_empty() {
                    working_dir = Some(PathBuf::from(val));
                }
            }
            "Hidden" => hidden = val.eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = val.eq_ignore_ascii_case("true"),
            "NotShowIn" => {
                not_show_in = val
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "StartupWMClass" if !val.is_empty() => {
                wm_class = Some(val.to_owned());
            }
            _ => {}
        }
    }

    if hidden || no_display {
        return ParsedEntry::HiddenOrNoDisplay;
    }
    if !not_show_in.is_empty() && excluded_by_current_desktop(&not_show_in) {
        return ParsedEntry::Skip;
    }
    if entry_type.as_deref() != Some("Application") {
        return ParsedEntry::Skip;
    }
    if let Some(ref check_bin) = try_exec
        && !command_exists(check_bin)
    {
        return ParsedEntry::Skip;
    }

    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return ParsedEntry::Skip;
    };
    let Some(exec_raw) = exec_raw.filter(|e| !e.is_empty()) else {
        return ParsedEntry::Skip;
    };

    let Some((exec_program, exec_args)) = parse_exec_command(&exec_raw, &name, path) else {
        return ParsedEntry::Skip;
    };

    if let Some(ref wmc) = wm_class
        && !keywords.iter().any(|k| k.eq_ignore_ascii_case(wmc))
    {
        keywords.push(wmc.clone());
    }
    for cat in &categories {
        if !keywords.iter().any(|k| k.eq_ignore_ascii_case(cat)) {
            keywords.push(cat.clone());
        }
    }

    let icon_path = icon_name
        .as_deref()
        .and_then(|ic| icon_resolver.resolve(ic));

    ParsedEntry::App(Box::new(DesktopApp {
        id: format!("app:{desktop_id}"),
        desktop_id: desktop_id.to_owned(),
        name,
        generic_name,
        comment,
        exec_raw,
        exec_program,
        exec_args,
        icon_name,
        icon_path,
        keywords,
        categories,
        terminal,
        working_dir,
        desktop_file_path: path.to_path_buf(),
        source,
    }))
}

fn unescape_desktop_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses a Freedesktop `Exec=` command string, handling quoting/escaping and stripping
/// field codes (`%u`, `%U`, `%f`, `%F`, `%i`, `%c`, `%k`, etc.) cleanly.
#[must_use]
pub fn parse_exec_command(
    exec_raw: &str,
    app_name: &str,
    desktop_path: &Path,
) -> Option<(String, Vec<String>)> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut chars = exec_raw.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '"' if !in_single => in_double = !in_double,
            '\'' if !in_double => in_single = !in_single,
            c if c.is_whitespace() && !in_double && !in_single => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut expanded: Vec<String> = Vec::with_capacity(tokens.len());
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" | "%i" => {
                continue;
            }
            "%c" => expanded.push(app_name.to_owned()),
            "%k" => expanded.push(desktop_path.to_string_lossy().into_owned()),
            other => {
                // Replace inline field codes like `--url=%u` or `%%` -> `%`
                let mut cleaned = other.replace("%%", "\x00");
                for code in [
                    "%f", "%F", "%u", "%U", "%d", "%D", "%n", "%N", "%v", "%m", "%i",
                ] {
                    cleaned = cleaned.replace(code, "");
                }
                cleaned = cleaned.replace("%c", app_name);
                cleaned = cleaned.replace("%k", &desktop_path.to_string_lossy());
                cleaned = cleaned.replace('\x00', "%");
                if !cleaned.is_empty() {
                    expanded.push(cleaned);
                }
            }
        }
    }

    if expanded.is_empty() {
        return None;
    }
    let program = expanded.remove(0);
    Some((program, expanded))
}

/// Spawns `app` cleanly detached from AIOS's stdio so closing the launcher never kills the app.
pub fn launch_desktop_app(app: &DesktopApp, force_terminal: bool) -> Result<()> {
    let use_terminal = app.terminal || force_terminal;

    let mut cmd = if use_terminal {
        let (term_bin, term_exec_flag) = resolve_terminal_emulator().ok_or_else(|| {
            AiosError::InvalidConfig(
                "No terminal emulator found. Set $TERMINAL or install foot/kitty/alacritty/ghostty."
                    .to_string(),
            )
        })?;
        let mut c = Command::new(&term_bin);
        if let Some(flag) = term_exec_flag {
            c.arg(flag);
        }
        c.arg(&app.exec_program);
        c.args(&app.exec_args);
        c
    } else {
        let mut c = Command::new(&app.exec_program);
        c.args(&app.exec_args);
        c
    };

    if let Some(ref dir) = app.working_dir
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach_process_group(&mut cmd);

    cmd.spawn().map_err(|source| AiosError::IoPath {
        path: PathBuf::from(&app.exec_program),
        source,
    })?;

    tracing::info!(
        app_id = %app.id,
        program = %app.exec_program,
        terminal = use_terminal,
        "launched desktop application"
    );
    Ok(())
}

/// Opens a URL or local file URI cleanly detached via `xdg-open`.
pub fn open_url_detached(url: &str) -> Result<()> {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach_process_group(&mut cmd);

    cmd.spawn().map_err(|source| AiosError::IoPath {
        path: PathBuf::from("xdg-open"),
        source,
    })?;

    tracing::info!(url = %url, "opened external URI via xdg-open");
    Ok(())
}

#[cfg(unix)]
fn detach_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn detach_process_group(_cmd: &mut Command) {}

fn excluded_by_current_desktop(not_show_in: &[String]) -> bool {
    let Ok(current) = env::var("XDG_CURRENT_DESKTOP") else {
        return false;
    };
    current
        .split(':')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(|desk| not_show_in.iter().any(|ex| ex.eq_ignore_ascii_case(desk)))
}

/// Resolves the user's preferred Wayland terminal emulator (`$TERMINAL` or common defaults).
#[must_use]
pub fn resolve_terminal_emulator() -> Option<(String, Option<&'static str>)> {
    if let Ok(term) = env::var("TERMINAL")
        && !term.trim().is_empty()
        && command_exists(term.trim())
    {
        let flag = if term.contains("gnome-terminal") {
            Some("--")
        } else {
            Some("-e")
        };
        return Some((term.trim().to_owned(), flag));
    }

    let candidates: &[(&str, Option<&'static str>)] = &[
        ("xdg-terminal-exec", None),
        ("ghostty", Some("-e")),
        ("kitty", None),
        ("foot", None),
        ("alacritty", Some("-e")),
        ("wezterm", Some("start")),
        ("konsole", Some("-e")),
        ("gnome-terminal", Some("--")),
    ];

    for &(bin, flag) in candidates {
        if command_exists(bin) {
            return Some((bin.to_owned(), flag));
        }
    }
    None
}

fn command_exists(bin: &str) -> bool {
    let p = Path::new(bin);
    if p.is_absolute() {
        return is_executable(p);
    }
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| is_executable(&dir.join(bin)))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_exec_field_codes_and_quotes() {
        let p = Path::new("/usr/share/applications/firefox.desktop");
        let (prog, args) =
            parse_exec_command("/usr/lib/firefox/firefox --new-window %u", "Firefox", p)
                .expect("valid exec");
        assert_eq!(prog, "/usr/lib/firefox/firefox");
        assert_eq!(args, vec!["--new-window"]);

        let (prog2, args2) =
            parse_exec_command("env \"FOO=bar baz\" code --reuse-window %F", "Code", p)
                .expect("quoted exec");
        assert_eq!(prog2, "env");
        assert_eq!(args2, vec!["FOO=bar baz", "code", "--reuse-window"]);
    }

    #[test]
    fn user_desktop_override_shadows_system_and_respects_nodisplay() {
        let tmp = TempDir::new().expect("temp dir");
        let user_dir = tmp.path().join("user");
        let sys_dir = tmp.path().join("sys");
        fs::create_dir_all(&user_dir).expect("mkdir user");
        fs::create_dir_all(&sys_dir).expect("mkdir sys");

        // System has two apps: demo.desktop and secret.desktop
        fs::write(
            sys_dir.join("demo.desktop"),
            "[Desktop Entry]\nType=Application\nName=Demo System\nExec=demo-sys %U\nKeywords=editor;code;\n",
        )
        .expect("write sys demo");
        fs::write(
            sys_dir.join("secret.desktop"),
            "[Desktop Entry]\nType=Application\nName=Secret App\nExec=secret\n",
        )
        .expect("write sys secret");

        // User overrides demo.desktop and hides secret.desktop with NoDisplay=true
        fs::write(
            user_dir.join("demo.desktop"),
            "[Desktop Entry]\nType=Application\nName=Demo User Override\nExec=demo-user --fast\n",
        )
        .expect("write user demo");
        fs::write(
            user_dir.join("secret.desktop"),
            "[Desktop Entry]\nType=Application\nName=Secret App\nExec=secret\nNoDisplay=true\n",
        )
        .expect("write user secret");

        let mut icons = IconResolver::new();
        let apps = scan_desktop_applications_in(
            &[(user_dir, AppSource::User), (sys_dir, AppSource::System)],
            &[],
            &mut icons,
        );

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Demo User Override");
        assert_eq!(apps[0].exec_program, "demo-user");
        assert_eq!(apps[0].source, AppSource::User);
    }

    #[cfg(unix)]
    #[test]
    fn discovers_executable_appimages_and_skips_non_executable() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("temp dir");
        let appimage_dir = tmp.path().join("Applications");
        fs::create_dir_all(&appimage_dir).expect("mkdir Applications");

        let exec_img = appimage_dir.join("Obsidian-1.7.AppImage");
        fs::write(&exec_img, "#!/bin/sh\n").expect("write exec appimage");
        fs::set_permissions(&exec_img, fs::Permissions::from_mode(0o755)).expect("chmod +x");

        let non_exec_img = appimage_dir.join("Ignored.AppImage");
        fs::write(&non_exec_img, "not executable").expect("write non-exec appimage");
        fs::set_permissions(&non_exec_img, fs::Permissions::from_mode(0o644)).expect("chmod 644");

        let mut icons = IconResolver::new();
        let apps = scan_desktop_applications_in(&[], &[appimage_dir], &mut icons);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Obsidian 1.7");
        assert_eq!(apps[0].source, AppSource::AppImage);
        assert_eq!(apps[0].id, "app:appimage:Obsidian-1.7.AppImage");
    }
}
