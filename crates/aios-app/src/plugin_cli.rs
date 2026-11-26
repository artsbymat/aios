//! Complete `aios plugin ...` developer & store CLI commands:
//! - `new` (scaffold a ready-to-build Rust `wasm32-wasip2` plugin + GitHub Release workflow)
//! - `build` (compile `--target wasm32-wasip2 --release` + stage `plugin.wasm`)
//! - `pack` (create deterministic `.aios-plugin` tarball + SHA-256 + registry TOML snippet)
//! - `search` / `install` / `update` / `uninstall` / `enable` / `disable` / `list`
//! - `registry-build` (compile `registry/plugins/*.toml` into `index.json` for GitHub Pages/CDN)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use aios_core::{AiosConfig, AiosPaths, Database, ThemeMode};
use aios_plugin_host::{
    PLUGIN_ARCHIVE_EXT, PluginHost, WasmExecutionLimits, build_registry_index_from_dir,
    discover_layered_plugins, fetch_registry_index, install_plugin_from_archive_file,
    install_plugin_from_dir, install_plugin_from_github, install_plugin_from_registry_entry,
    is_newer_version, load_plugin_from_dir, pack_plugin_dir, uninstall_plugin,
};
use aios_ui::run_dev_plugin_app;
use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};

use crate::CliTheme;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PluginTemplate {
    List,
    Grid,
    Form,
    Detail,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List bundled and user-installed external WASM plugins with enabled/disabled state.
    List,
    /// Scaffold a ready-to-build Rust WASM plugin project (+ GitHub Releases workflow).
    New {
        /// Plugin directory / short name (e.g. `my-tool` -> `com.author.my-tool`).
        name: String,
        /// Optional reverse-domain plugin ID (default: `com.local.<name>`).
        #[arg(long)]
        id: Option<String>,
        /// Optional author name for `aios-plugin.toml`.
        #[arg(long, default_value = "Community")]
        author: String,
        /// Starter view template (`list`, `grid`, `form`, or `detail`).
        #[arg(long, value_enum, default_value_t = PluginTemplate::List)]
        template: PluginTemplate,
        /// Path to `aios-plugin-sdk` (defaults to local workspace crate if detected).
        #[arg(long)]
        sdk_path: Option<PathBuf>,
    },
    /// Compile a Rust WASM plugin (`wasm32-wasip2`) and stage `plugin.wasm` alongside `aios-plugin.toml`.
    Build {
        /// Plugin project directory (defaults to current working directory).
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Package a validated plugin directory into `<id>-v<version>.aios-plugin` + SHA-256 + registry snippet.
    Pack {
        /// Plugin bundle directory (defaults to current working directory).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output directory for the `.aios-plugin` archive (defaults to `<path>/dist`).
        #[arg(short = 'o', long)]
        out: Option<PathBuf>,
    },
    /// Search plugins in the configured AIOS Plugin Store registries (`index.json`).
    Search {
        /// Optional search term (lists all catalog plugins when omitted).
        query: Option<String>,
        /// Force refreshing `index.json` from remote registries.
        #[arg(long)]
        refresh: bool,
    },
    /// Install a plugin from a local directory, `.aios-plugin` archive, `github:<owner>/<repo>`, or registry ID.
    Install {
        /// Local directory, `.aios-plugin` file, `github:<owner>/<repo>`, or registry plugin ID.
        source: String,
        /// Optional expected SHA-256 hex digest when installing a local archive file.
        #[arg(long)]
        sha256: Option<String>,
    },
    /// Update one or all installed plugins from configured registries.
    Update {
        /// Optional plugin ID (updates all outdated plugins when omitted).
        plugin_id: Option<String>,
    },
    /// Enable a previously disabled plugin (bundled or user-installed).
    Enable { plugin_id: String },
    /// Disable a plugin without deleting its files (works for both bundled and user plugins).
    Disable { plugin_id: String },
    /// Uninstall a user-installed external WASM plugin by ID and purge its local state.
    Uninstall { plugin_id: String },
    /// Run a live hot-reloading preview window bound to a local plugin directory.
    Dev {
        path: PathBuf,
        /// Optional command ID inside `aios-plugin.toml` to launch immediately.
        #[arg(long)]
        command: Option<String>,
        /// Optional theme override for the dev preview session.
        #[arg(long, value_enum)]
        theme: Option<CliTheme>,
    },
    /// Compile a registry's `plugins/*.toml` directory into a static `index.json` (used by CI & local dev).
    RegistryBuild {
        /// Path to the registry root directory containing `plugins/*.toml`.
        #[arg(default_value = "registry")]
        registry_dir: PathBuf,
        /// Output path for generated `index.json` (defaults to `<registry_dir>/index.json`).
        #[arg(short = 'o', long)]
        out: Option<PathBuf>,
    },
}

pub fn run_plugin_cli(action: PluginCommand) -> Result<()> {
    // Pure authoring/packaging/CI subcommands must not require a writable user XDG home directory.
    match action {
        PluginCommand::New {
            name,
            id,
            author,
            template,
            sdk_path,
        } => {
            return scaffold_new_plugin(
                &name,
                id.as_deref(),
                &author,
                template,
                sdk_path.as_deref(),
            );
        }
        PluginCommand::Build { path } => return build_wasm_plugin(&path),
        PluginCommand::Pack { path, out } => return pack_wasm_plugin(&path, out.as_deref()),
        PluginCommand::RegistryBuild { registry_dir, out } => {
            let index =
                build_registry_index_from_dir(&registry_dir).map_err(|e| anyhow::anyhow!(e))?;
            let out_path = out.unwrap_or_else(|| registry_dir.join("index.json"));
            let json = serde_json::to_string_pretty(&index)?;
            fs::write(&out_path, format!("{json}\n"))
                .with_context(|| format!("failed to write '{}'", out_path.display()))?;
            println!(
                "Built registry index with {} plugin(s) -> {}",
                index.plugins.len(),
                out_path.display()
            );
            return Ok(());
        }
        _ => {}
    }

    let paths = AiosPaths::discover()?;
    paths.ensure_dirs()?;
    let config = AiosConfig::load_or_default(&paths)?;

    match action {
        PluginCommand::New { .. }
        | PluginCommand::Build { .. }
        | PluginCommand::Pack { .. }
        | PluginCommand::RegistryBuild { .. } => unreachable!(),
        PluginCommand::List => {
            let db = Database::open(&paths.database_file())?;
            let disabled = db.list_disabled_plugins()?;
            let plugins =
                discover_layered_plugins(&paths.bundled_plugins_dirs(), &paths.plugins_dir());
            println!("External WASM Plugins ({}):", plugins.len());
            if plugins.is_empty() {
                println!("  (none in {})", paths.plugins_dir().display());
            } else {
                for p in &plugins {
                    let state = if disabled.contains(&p.manifest.id) {
                        "disabled"
                    } else {
                        "enabled"
                    };
                    println!(
                        "  - {} v{} [{}] ({}, {}, {} command(s))",
                        p.manifest.name,
                        p.manifest.version,
                        p.manifest.id,
                        p.origin.as_str(),
                        state,
                        p.manifest.commands.len()
                    );
                }
            }
            Ok(())
        }
        PluginCommand::Search { query, refresh } => {
            let db = Arc::new(Mutex::new(Database::open(&paths.database_file())?));
            let index = fetch_registry_index(
                &config.plugins.registry_urls,
                &db,
                config.plugins.auto_refresh_hours,
                refresh,
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            let needle = query.unwrap_or_default().trim().to_ascii_lowercase();
            let matches: Vec<_> = index
                .plugins
                .iter()
                .filter(|p| {
                    needle.is_empty()
                        || p.id.to_ascii_lowercase().contains(&needle)
                        || p.name.to_ascii_lowercase().contains(&needle)
                        || p.description.to_ascii_lowercase().contains(&needle)
                        || p.tags
                            .iter()
                            .any(|t| t.to_ascii_lowercase().contains(&needle))
                })
                .collect();
            println!("AIOS Plugin Store ({} match(es)):", matches.len());
            for p in matches {
                println!(
                    "  - {} v{} [{}] by {} ({})\n    {}",
                    p.name, p.latest.version, p.id, p.author, p.category, p.description
                );
            }
            Ok(())
        }
        PluginCommand::Install { source, sha256 } => {
            let db = Arc::new(Mutex::new(Database::open(&paths.database_file())?));
            let src_path = Path::new(&source);
            let installed = if src_path.is_dir() {
                install_plugin_from_dir(src_path, &paths.plugins_dir())?
            } else if src_path.is_file() {
                install_plugin_from_archive_file(
                    src_path,
                    sha256.as_deref(),
                    &paths.plugins_dir(),
                    &db,
                )
                .map_err(|e| anyhow::anyhow!(e))?
            } else if source.starts_with("github:") {
                install_plugin_from_github(&source, &paths.plugins_dir(), &db)
                    .map_err(|e| anyhow::anyhow!(e))?
            } else {
                let index = fetch_registry_index(
                    &config.plugins.registry_urls,
                    &db,
                    config.plugins.auto_refresh_hours,
                    false,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                let entry = index
                    .plugins
                    .iter()
                    .find(|p| p.id == source)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "plugin '{source}' not found locally or in configured registries. Use a local path, `.{PLUGIN_ARCHIVE_EXT}`, `github:<owner>/<repo>`, or a registry ID."
                        )
                    })?;
                install_plugin_from_registry_entry(entry, &paths.plugins_dir(), &db)
                    .map_err(|e| anyhow::anyhow!(e))?
            };
            println!(
                "Installed WASM plugin '{}' ({}) v{} -> {}",
                installed.manifest.name,
                installed.manifest.id,
                installed.manifest.version,
                installed.root_dir.display()
            );
            let caps = installed.manifest.capabilities.declared_keys();
            if !caps.is_empty() {
                println!(
                    "Declared capabilities (denied until approved in Settings & Plugins): {}",
                    caps.join(", ")
                );
            }
            Ok(())
        }
        PluginCommand::Update { plugin_id } => {
            let db = Arc::new(Mutex::new(Database::open(&paths.database_file())?));
            let index = fetch_registry_index(
                &config.plugins.registry_urls,
                &db,
                config.plugins.auto_refresh_hours,
                true,
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            let local_plugins =
                discover_layered_plugins(&paths.bundled_plugins_dirs(), &paths.plugins_dir());
            let mut updated = 0usize;
            for local in &local_plugins {
                if let Some(target_id) = &plugin_id
                    && &local.manifest.id != target_id
                {
                    continue;
                }
                if let Some(entry) = index.plugins.iter().find(|p| p.id == local.manifest.id)
                    && is_newer_version(&local.manifest.version, &entry.latest.version)
                {
                    let res = install_plugin_from_registry_entry(entry, &paths.plugins_dir(), &db)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "Updated {} ({} -> {})",
                        res.manifest.id, local.manifest.version, res.manifest.version
                    );
                    updated += 1;
                }
            }
            if updated == 0 {
                println!("All checked plugins are already up to date.");
            }
            Ok(())
        }
        PluginCommand::Enable { plugin_id } => {
            let db = Database::open(&paths.database_file())?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            db.set_plugin_enabled(&plugin_id, true, now)?;
            println!("Enabled plugin '{plugin_id}'.");
            Ok(())
        }
        PluginCommand::Disable { plugin_id } => {
            let db = Database::open(&paths.database_file())?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            db.set_plugin_enabled(&plugin_id, false, now)?;
            println!("Disabled plugin '{plugin_id}'.");
            Ok(())
        }
        PluginCommand::Uninstall { plugin_id } => {
            let db = Database::open(&paths.database_file())?;
            let removed = uninstall_plugin(&plugin_id, &paths.plugins_dir(), Some(&db))?;
            if removed {
                println!("Uninstalled plugin '{plugin_id}' and purged local plugin state.");
            } else {
                println!(
                    "Plugin '{plugin_id}' is not in user plugins dir (use `aios plugin disable {plugin_id}` for bundled defaults)."
                );
            }
            Ok(())
        }
        PluginCommand::Dev {
            path,
            command,
            theme,
        } => {
            let db = Arc::new(Mutex::new(Database::open(&paths.database_file())?));
            let mut probe = PluginHost::new(
                config.clone(),
                paths.clone(),
                db,
                WasmExecutionLimits::default(),
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            let bundle = probe
                .attach_dev_plugin(&path)
                .map_err(|e| anyhow::anyhow!(e))?;
            tracing::info!(
                plugin_id = %bundle.manifest.id,
                dir = %path.display(),
                "starting AIOS plugin dev session with hot-reload"
            );
            run_dev_plugin_app(config, paths, path, command, theme.map(ThemeMode::from));
            Ok(())
        }
    }
}

fn build_wasm_plugin(project_dir: &Path) -> Result<()> {
    let cargo_toml = project_dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        bail!(
            "no Cargo.toml found in '{}'. Run `aios plugin new <name>` first.",
            project_dir.display()
        );
    }

    let status = Command::new("cargo")
        .arg("build")
        .arg("--target")
        .arg("wasm32-wasip2")
        .arg("--release")
        .current_dir(project_dir)
        .status()
        .context("failed to invoke `cargo build --target wasm32-wasip2 --release`")?;
    if !status.success() {
        bail!(
            "WASM build failed. Ensure `wasm32-wasip2` target is installed (`rustup target add wasm32-wasip2`)."
        );
    }

    // Locate compiled `.wasm` artifact under target/wasm32-wasip2/release/*.wasm.
    let release_dir = project_dir
        .join("target")
        .join("wasm32-wasip2")
        .join("release");
    let mut wasm_files: Vec<PathBuf> = fs::read_dir(&release_dir)
        .with_context(|| format!("failed to inspect '{}'", release_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wasm"))
        .collect();
    wasm_files.sort();
    let compiled = wasm_files.first().ok_or_else(|| {
        anyhow::anyhow!(
            "no compiled .wasm file found in '{}'. Ensure Cargo.toml defines `crate-type = [\"cdylib\"]`.",
            release_dir.display()
        )
    })?;

    let dest = project_dir.join("plugin.wasm");
    fs::copy(compiled, &dest).with_context(|| {
        format!(
            "failed to copy '{}' -> '{}'",
            compiled.display(),
            dest.display()
        )
    })?;

    let validated = load_plugin_from_dir(project_dir)?;
    let size_kb = fs::metadata(&dest).map_or(0, |m| m.len()) / 1024;
    println!(
        "Built external WASM plugin '{}' ({}) v{} -> {} ({} KiB)",
        validated.manifest.name,
        validated.manifest.id,
        validated.manifest.version,
        dest.display(),
        size_kb
    );
    Ok(())
}

fn pack_wasm_plugin(bundle_dir: &Path, out_dir: Option<&Path>) -> Result<()> {
    let validated = load_plugin_from_dir(bundle_dir)?;
    let target_dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| bundle_dir.join("dist"));
    let archive_name = format!(
        "{}-v{}.{PLUGIN_ARCHIVE_EXT}",
        validated.manifest.id, validated.manifest.version
    );
    let archive_path = target_dir.join(&archive_name);
    let (plugin, sha256) = pack_plugin_dir(bundle_dir, &archive_path)?;

    let sha_file = target_dir.join("SHA256SUMS");
    fs::write(&sha_file, format!("{sha256}  {archive_name}\n"))?;

    println!("Packed archive : {}", archive_path.display());
    println!("SHA-256        : {sha256}");
    println!(
        "\nRegistry PR snippet (registry/plugins/{}.toml):",
        plugin.manifest.id
    );
    println!("```toml");
    println!("id = \"{}\"", plugin.manifest.id);
    println!("name = \"{}\"", plugin.manifest.name);
    println!("author = \"{}\"", plugin.manifest.author);
    println!("description = \"{}\"", plugin.manifest.description);
    println!("repository = \"https://github.com/<owner>/<repo>\"");
    println!("category = \"utilities\"");
    println!("tags = []");
    let caps = plugin.manifest.capabilities.declared_keys();
    println!("capabilities = {:?}", caps);
    println!();
    println!("[latest]");
    println!("version = \"{}\"", plugin.manifest.version);
    if let Some(min_v) = &plugin.manifest.min_aios_version {
        println!("min_aios_version = \"{min_v}\"");
    }
    println!(
        "download_url = \"https://github.com/<owner>/<repo>/releases/download/v{}/{}\"",
        plugin.manifest.version, archive_name
    );
    println!("sha256 = \"{sha256}\"");
    println!("```");
    Ok(())
}

fn scaffold_new_plugin(
    raw_name: &str,
    explicit_id: Option<&str>,
    author: &str,
    template: PluginTemplate,
    sdk_path: Option<&Path>,
) -> Result<()> {
    let slug: String = raw_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        bail!("plugin name must contain at least one alphanumeric character");
    }
    let dir = PathBuf::from(&slug);
    if dir.exists() {
        bail!("directory '{}' already exists", dir.display());
    }
    let plugin_id = explicit_id
        .map(str::to_owned)
        .unwrap_or_else(|| format!("com.community.{slug}"));
    let title: String = slug
        .split('-')
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut ch = part.chars();
            match ch.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), ch.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let crate_name = slug.replace('-', "_");
    let default_view = match template {
        PluginTemplate::List => "list",
        PluginTemplate::Grid => "grid",
        PluginTemplate::Form => "form",
        PluginTemplate::Detail => "detail",
    };

    let sdk_dep = if let Some(p) = sdk_path {
        format!(
            "aios-plugin-sdk = {{ path = \"{}\", features = [\"guest\"] }}",
            p.display()
        )
    } else if let Ok(cwd) = std::env::current_dir()
        && cwd.join("crates/aios-plugin-sdk/Cargo.toml").is_file()
    {
        format!(
            "aios-plugin-sdk = {{ path = \"{}\", features = [\"guest\"] }}",
            cwd.join("crates/aios-plugin-sdk").display()
        )
    } else {
        "aios-plugin-sdk = { git = \"https://github.com/artsbymat/aios\", features = [\"guest\"] }"
            .to_string()
    };

    fs::create_dir_all(dir.join("src"))?;
    fs::create_dir_all(dir.join(".github/workflows"))?;

    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[workspace]

[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
{sdk_dep}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"

[profile.release]
opt-level = "s"
lto = true
strip = true
"#
        ),
    )?;

    fs::write(
        dir.join("aios-plugin.toml"),
        format!(
            r#"[plugin]
id = "{plugin_id}"
name = "{title}"
version = "0.1.0"
description = "External WASM plugin for AIOS."
author = "{author}"
min_aios_version = "0.1.0"
icon = "icon.svg"
entry = "plugin.wasm"

[capabilities]
clipboard_write = true
clipboard_read = false
http_hosts = []
fs_read = []
secrets = false

[[commands]]
id = "main"
title = "{title}"
subtitle = "External WASM plugin"
mode = "view"
default_view = "{default_view}"
keywords = ["{slug}", "plugin"]
"#
        ),
    )?;

    let initials: String = title
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    fs::write(
        dir.join("icon.svg"),
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64" fill="none">
  <title>{title}</title>
  <rect x="1" y="1" width="62" height="62" rx="15" fill="#3A414A"/>
  <rect x="3" y="3" width="58" height="58" rx="13" fill="#1B1E22"/>
  <rect x="14" y="14" width="16" height="16" rx="4" fill="#E87143"/>
  <rect x="34" y="14" width="16" height="16" rx="4" fill="#F1EFEA"/>
  <rect x="14" y="34" width="36" height="16" rx="4" fill="#F1EFEA"/>
</svg>
"##
        ),
    )?;

    let body_rs = match template {
        PluginTemplate::List => format!(
            r##"use aios_plugin_sdk::prelude::*;

#[derive(Default)]
struct Plugin;

impl AiosPlugin for Plugin {{
    fn init_command(&self, _command_id: &str, _args: &[String]) -> PluginResponse {{
        let view = ListViewBuilder::new("{title}")
            .placeholder("Filter items...")
            .showing_detail(true)
            .section(
                Some("Starter Items"),
                Some("1 item"),
                vec![
                    ListItemBuilder::new("hello", "Hello from {title}")
                        .subtitle("Edit src/lib.rs and run `aios plugin build`")
                        .icon("{initials}")
                        .icon_path("icon.svg")
                        .accessory_tag("WASM", SemanticTone::Accent)
                        .detail(
                            "# {title}\n\nYour external `wasm32-wasip2` AIOS plugin is running in Wasmtime.",
                            vec![MetadataEntry::Label {{
                                title: "Runtime".to_string(),
                                text: "WASM Component".to_string(),
                            }}],
                        )
                        .actions(
                            ActionPanelBuilder::new()
                                .action(
                                    ActionItem::new("copy.greeting", "Copy Greeting")
                                        .shortcut("Enter"),
                                )
                                .build(),
                        )
                        .build(),
                ],
            )
            .build();
        PluginResponse::render(view)
    }}

    fn handle_event(&self, _command_id: &str, event: PluginEvent) -> PluginResponse {{
        match event {{
            PluginEvent::ActionTriggered {{ action_id, .. }} if action_id == "copy.greeting" => {{
                PluginResponse::CopyToClipboard {{
                    text: "Hello from {title}!".to_string(),
                    hud_title: Some("Copied greeting".to_string()),
                }}
            }}
            _ => PluginResponse::Noop,
        }}
    }}
}}

aios_plugin_sdk::export_plugin!(Plugin);
"##
        ),
        PluginTemplate::Grid => format!(
            r##"use aios_plugin_sdk::prelude::*;

#[derive(Default)]
struct Plugin;

impl AiosPlugin for Plugin {{
    fn init_command(&self, _command_id: &str, _args: &[String]) -> PluginResponse {{
        let view = GridViewBuilder::new("{title}", 4)
            .placeholder("Search tiles...")
            .section(
                Some("Palette"),
                None,
                vec![
                    GridItemBuilder::new(
                        "copper",
                        "Signal Copper",
                        GridItemContent::ColorSwatch {{
                            hex: "#E87143".to_string(),
                        }},
                    )
                    .subtitle("#E87143")
                    .actions(
                        ActionPanelBuilder::new()
                            .action(ActionItem::new("copy:#E87143", "Copy HEX").shortcut("Enter"))
                            .build(),
                    )
                    .build(),
                ],
            )
            .build();
        PluginResponse::render(view)
    }}

    fn handle_event(&self, _command_id: &str, event: PluginEvent) -> PluginResponse {{
        match event {{
            PluginEvent::ActionTriggered {{ action_id, .. }} => {{
                if let Some(hex) = action_id.strip_prefix("copy:") {{
                    return PluginResponse::CopyToClipboard {{
                        text: hex.to_string(),
                        hud_title: Some(format!("Copied {{hex}}")),
                    }};
                }}
                PluginResponse::Noop
            }}
            _ => PluginResponse::Noop,
        }}
    }}
}}

aios_plugin_sdk::export_plugin!(Plugin);
"##
        ),
        PluginTemplate::Form => format!(
            r##"use aios_plugin_sdk::prelude::*;

#[derive(Default)]
struct Plugin;

impl AiosPlugin for Plugin {{
    fn init_command(&self, _command_id: &str, _args: &[String]) -> PluginResponse {{
        let view = FormViewBuilder::new("{title}")
            .field(FormField::TextField {{
                id: "note".to_string(),
                label: "Note".to_string(),
                placeholder: Some("Type something...".to_string()),
                value: String::new(),
                required: true,
                error: None,
            }})
            .actions(
                ActionPanelBuilder::new()
                    .action(ActionItem::new("submit", "Save").shortcut("Enter"))
                    .build(),
            )
            .build();
        PluginResponse::render(view)
    }}

    fn handle_event(&self, _command_id: &str, event: PluginEvent) -> PluginResponse {{
        match event {{
            PluginEvent::FormSubmitted {{ values, .. }} => {{
                let note = values.get("note").cloned().unwrap_or_default();
                PluginResponse::CopyToClipboard {{
                    text: note,
                    hud_title: Some("Copied form value".to_string()),
                }}
            }}
            _ => PluginResponse::Noop,
        }}
    }}
}}

aios_plugin_sdk::export_plugin!(Plugin);
"##
        ),
        PluginTemplate::Detail => format!(
            r##"use aios_plugin_sdk::prelude::*;

#[derive(Default)]
struct Plugin;

impl AiosPlugin for Plugin {{
    fn init_command(&self, _command_id: &str, _args: &[String]) -> PluginResponse {{
        let view = DetailViewBuilder::new(
            "{title}",
            "# {title}\n\nThis detail view is rendered from a sandboxed `wasm32-wasip2` plugin.",
        )
        .metadata(vec![MetadataEntry::Label {{
            title: "Plugin ID".to_string(),
            text: "{plugin_id}".to_string(),
        }}])
        .actions(
            ActionPanelBuilder::new()
                .action(ActionItem::new("nav.pop", "Back").shortcut("Esc"))
                .build(),
        )
        .build();
        PluginResponse::render(view)
    }}

    fn handle_event(&self, _command_id: &str, _event: PluginEvent) -> PluginResponse {{
        PluginResponse::Noop
    }}
}}

aios_plugin_sdk::export_plugin!(Plugin);
"##
        ),
    };
    fs::write(dir.join("src/lib.rs"), body_rs)?;

    fs::write(
        dir.join(".github/workflows/release.yml"),
        format!(
            r#"name: Release AIOS Plugin

on:
  push:
    tags:
      - "v*"

permissions:
  contents: write

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-wasip2
      - name: Build WASM Component
        run: |
          cargo build --target wasm32-wasip2 --release
          cp target/wasm32-wasip2/release/{crate_name}.wasm plugin.wasm
      - name: Package .aios-plugin archive
        run: |
          VERSION="${{GITHUB_REF_NAME#v}}"
          ARCHIVE="{plugin_id}-v${{VERSION}}.{PLUGIN_ARCHIVE_EXT}"
          tar -czf "$ARCHIVE" aios-plugin.toml plugin.wasm icon.svg
          sha256sum "$ARCHIVE" > SHA256SUMS
      - name: Publish GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            *.{PLUGIN_ARCHIVE_EXT}
            SHA256SUMS
"#
        ),
    )?;

    fs::write(
        dir.join("README.md"),
        format!(
            r#"# {title} (`{plugin_id}`)

External WebAssembly Component Model (`wasm32-wasip2`) plugin for AIOS.

## Development

```bash
aios plugin build .
aios plugin dev .
```

## Packaging & Free Release via GitHub

```bash
aios plugin pack .
git tag v0.1.0 && git push --tags
```
"#
        ),
    )?;

    println!("Scaffolded AIOS WASM plugin '{title}' ({plugin_id}) in ./{slug}/");
    println!("Next steps:");
    println!("  cd {slug}");
    println!("  aios plugin build .");
    println!("  aios plugin dev .");
    Ok(())
}
