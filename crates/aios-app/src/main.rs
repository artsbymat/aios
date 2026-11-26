mod plugin_cli;

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aios_core::{AiosConfig, AiosPaths, Database, LogFormat, LogOptions, ThemeMode, init_logging};
use aios_daemon::{
    DaemonIpcCommand, DaemonIpcResponse, DmenuCaseMode, DmenuRequest, DmenuStreamMessage,
};
use aios_scripts::{discover_script_commands, execute_script_command};
use aios_ui::{
    DmenuOutcome, palette_for, run_daemon_app, run_dmenu_app_streaming, run_preview_app,
};
use aios_wayland::{IpcClient, detect_compositor, read_system_color_scheme};
use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use plugin_cli::{PluginCommand, run_plugin_cli};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CliTheme {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliCaseMode {
    /// Case-insensitive when query is lowercase; case-sensitive once query contains uppercase.
    Smart,
    /// Always ignore letter case (same as -i / --insensitive).
    Insensitive,
    /// Always match exact letter case (same as -s / --sensitive).
    Sensitive,
}

impl From<CliCaseMode> for DmenuCaseMode {
    fn from(value: CliCaseMode) -> Self {
        match value {
            CliCaseMode::Smart => Self::Smart,
            CliCaseMode::Insensitive => Self::Insensitive,
            CliCaseMode::Sensitive => Self::Sensitive,
        }
    }
}

impl From<CliTheme> for ThemeMode {
    fn from(value: CliTheme) -> Self {
        match value {
            CliTheme::System => Self::System,
            CliTheme::Dark => Self::Dark,
            CliTheme::Light => Self::Light,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "aios",
    version,
    about = "AIOS (All in One Search) Wayland launcher"
)]
struct Cli {
    /// Emit multi-line pretty structured logs instead of compact single-line logs.
    #[arg(long, global = true)]
    log_pretty: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect resolved XDG paths, configuration, SQLite WAL status, portal theme, and compositor.
    Doctor,
    /// Start the persistent single-instance AIOS Wayland daemon (`$XDG_RUNTIME_DIR/aios.sock`).
    Daemon {
        /// Reveal the launcher overlay immediately on daemon startup.
        #[arg(long)]
        show: bool,
    },
    /// Open the interactive launcher directly in foreground (useful for testing views & themes).
    Preview {
        /// Optional deep-link fixture URI (e.g. aios://demo/grid, aios://demo/form, aios://demo/detail).
        #[arg(long)]
        uri: Option<String>,
        /// Override theme mode for this preview session.
        #[arg(long, value_enum)]
        theme: Option<CliTheme>,
    },
    /// Toggle the main AIOS launcher overlay via the running daemon.
    Toggle,
    /// Show the main AIOS launcher overlay via the running daemon.
    Show,
    /// Hide the main AIOS launcher overlay via the running daemon.
    Hide,
    /// Open an AIOS deep link URI (e.g. aios://search?q=screenshot, aios://settings, aios://scripts).
    Open { uri: String },
    /// Interactive dmenu/rofi-compatible stdin -> stdout fuzzy picker.
    #[command(
        long_about = "Interactive dmenu/rofi-compatible stdin -> stdout fuzzy picker powered by GPUI Wayland LayerShell and aios-fuzzy.\n\nReads newline-separated items from stdin, opens a compact overlay via the running `aios daemon` (or falls back automatically to a standalone window), and prints the selected entry to stdout.",
        after_long_help = "CASE MATCHING:\n  Default is `smart` (case-insensitive for all-lowercase queries, case-sensitive once any uppercase letter is typed).\n  Use `-i` / `--insensitive` (or `--case-mode insensitive`) to always ignore case.\n  Use `-s` / `--sensitive`   (or `--case-mode sensitive`)   to always enforce exact case.\n\nICONS:\n  --icon <ICON>    Sets the header icon (path to .svg/.png, XDG icon theme name like `camera-photo`, or short badge).\n  --show-icons     Enables per-row icons; supports rofi-style `Label\\0icon\\x1f<name-or-path>` lines on stdin.\n                   Only the clean `Label` is printed to stdout.\n\nKEYBINDS:\n  Up / Down / Ctrl+P / Ctrl+N / Tab  Move selection\n  Enter                              Confirm highlighted match (or custom text if --allow-custom and no matches)\n  Ctrl+Enter                         Force custom typed text when --allow-custom is enabled\n  Ctrl+T                             Cycle System / Dark / Light theme\n  Esc                                Clear query, or cancel when query is empty\n  Ctrl+C                             Cancel immediately\n\nEXIT CODES:\n  0  An item (or custom text) was selected and printed to stdout\n  1  Cancelled by user or empty stdin (without --allow-custom)\n\nEXAMPLES:\n  # Drop-in replacement for rofi -dmenu:\n  echo -e \"Full\\nArea\\nWindow\\nDecodeQR\" | aios dmenu -p \"Screenshot\" -l 7\n\n  # Custom header icon (XDG icon name or .svg/.png file path):\n  echo -e \"Full\\nArea\\nWindow\" | aios dmenu -p \"Screenshot\" --icon camera-photo\n\n  # Per-row icons using rofi protocol (stdout still outputs clean label):\n  printf \"Full\\0icon\\x1fcamera-photo\\nArea\\0icon\\x1fedit-cut\\nQR\\0icon\\x1fview-barcode\\n\" \\\n    | aios dmenu --show-icons -p \"Screenshot\"\n\n  # Force case-sensitive or case-insensitive matching:\n  printf \"alpha\\nAlpha\\nALPHA\\n\" | aios dmenu -s -p \"Exact Case\"\n  printf \"alpha\\nAlpha\\nALPHA\\n\" | aios dmenu -i -p \"Ignore Case\"\n\n  # Freeform text prompt (replacement for yad/zenity entry):\n  printf \"\" | aios dmenu --allow-custom -p \"Enter new filename\"\n\n  # Return 0-based index instead of text:\n  printf \"First\\nSecond\\nThird\\n\" | aios dmenu --index -p \"Pick index\""
    )]
    Dmenu {
        /// Prompt / placeholder shown in the search bar.
        #[arg(short = 'p', long)]
        prompt: Option<String>,
        /// Target number of visible rows (controls compact window height).
        #[arg(short = 'l', long)]
        lines: Option<usize>,
        /// Header icon (file path to .svg/.png, XDG icon name like `camera-photo`, or short badge).
        #[arg(long)]
        icon: Option<String>,
        /// Show per-row icon column (supports rofi `Label\0icon\x1f<icon>` lines on stdin).
        #[arg(long)]
        show_icons: bool,
        /// Match case-insensitively (ignores letter case, compatible with dmenu/rofi -i).
        #[arg(
            short = 'i',
            long,
            conflicts_with_all = ["sensitive", "case_mode"]
        )]
        insensitive: bool,
        /// Match case-sensitively (enforces exact upper/lowercase matching).
        #[arg(
            short = 's',
            long,
            conflicts_with_all = ["insensitive", "case_mode"]
        )]
        sensitive: bool,
        /// Explicit case matching mode (overrides default smart-case).
        #[arg(long, value_enum, conflicts_with_all = ["insensitive", "sensitive"])]
        case_mode: Option<CliCaseMode>,
        /// Allow submitting custom typed text when nothing matches (or via Ctrl+Enter).
        #[arg(long)]
        allow_custom: bool,
        /// Print the 0-based index of the selected line instead of its text.
        #[arg(long)]
        index: bool,
        /// Force standalone window without using a running `aios daemon`.
        #[arg(long)]
        standalone: bool,
        /// Override theme mode for this dmenu invocation.
        #[arg(long, value_enum)]
        theme: Option<CliTheme>,
    },
    /// Query live status from the running AIOS daemon.
    Status,
    /// Manage external WASM plugins and local plugin development (`aios plugin ...`).
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
    /// Manage and run local Script Commands (`aios script ...`).
    Script {
        #[command(subcommand)]
        action: ScriptCliCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ScriptCliCommand {
    /// List discovered Script Commands in `$XDG_CONFIG_HOME/aios/scripts/`.
    List,
    /// Execute a discovered Script Command by ID (`script:<name>` or `<name>`).
    Run {
        /// Script ID (`script:<name>`) or file stem (`<name>`).
        id: String,
        /// Optional positional arguments passed to the script.
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Print the resolved Script Commands directory path.
    Dir,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(&LogOptions {
        format: if cli.log_pretty {
            LogFormat::Pretty
        } else {
            LogFormat::Compact
        },
        ..LogOptions::default()
    });

    match cli.command.unwrap_or(Command::Doctor) {
        Command::Doctor => run_doctor(),
        Command::Daemon { show } => {
            let paths = AiosPaths::discover()?;
            paths.ensure_dirs()?;
            let config = AiosConfig::load_or_default(&paths)?;
            run_daemon_app(config, paths, show)?;
            Ok(())
        }
        Command::Preview { uri, theme } => {
            let paths = AiosPaths::discover()?;
            let config = AiosConfig::load_or_default(&paths)?;
            run_preview_app(config, paths, uri, theme.map(ThemeMode::from));
            Ok(())
        }
        Command::Toggle => dispatch_ipc(DaemonIpcCommand::Toggle),
        Command::Show => dispatch_ipc(DaemonIpcCommand::Show),
        Command::Hide => dispatch_ipc(DaemonIpcCommand::Hide),
        Command::Open { uri } => dispatch_ipc(DaemonIpcCommand::Open { uri }),
        Command::Dmenu {
            prompt,
            lines,
            icon,
            show_icons,
            insensitive,
            sensitive,
            case_mode,
            allow_custom,
            index,
            standalone,
            theme,
        } => {
            let resolved_case = if sensitive {
                DmenuCaseMode::Sensitive
            } else if insensitive {
                DmenuCaseMode::Insensitive
            } else if let Some(cm) = case_mode {
                cm.into()
            } else {
                DmenuCaseMode::Smart
            };
            run_dmenu(
                prompt,
                lines,
                icon,
                show_icons,
                resolved_case,
                allow_custom,
                index,
                standalone,
                theme,
            )
        }
        Command::Status => dispatch_ipc(DaemonIpcCommand::Status),
        Command::Plugin { action } => run_plugin_cli(action),
        Command::Script { action } => run_script_cli(action),
    }
}

fn run_script_cli(action: ScriptCliCommand) -> Result<()> {
    let paths = AiosPaths::discover()?;
    paths.ensure_dirs()?;

    match action {
        ScriptCliCommand::List => {
            let scripts = discover_script_commands(&paths.scripts_dir());
            println!("Script Commands ({}):", scripts.len());
            if scripts.is_empty() {
                println!("  (none in {})", paths.scripts_dir().display());
            } else {
                for s in &scripts {
                    let confirm = if s.needs_confirmation {
                        " confirm=true"
                    } else {
                        ""
                    };
                    println!(
                        "  - {} [{}] mode={} args={}{} ({})",
                        s.title,
                        s.id,
                        s.mode.label(),
                        s.arguments.len(),
                        confirm,
                        s.path.display()
                    );
                }
            }
            Ok(())
        }
        ScriptCliCommand::Run { id, args } => {
            let scripts = discover_script_commands(&paths.scripts_dir());
            let Some(script) = scripts
                .iter()
                .find(|s| s.id == id || s.id.strip_prefix("script:") == Some(id.as_str()))
            else {
                bail!(
                    "script '{id}' not found in {}. Run `aios script list` to inspect available scripts.",
                    paths.scripts_dir().display()
                );
            };
            let (outcome, _) = execute_script_command(script, &args, Duration::from_secs(30));
            if !outcome.stdout.is_empty() {
                print!("{}", outcome.stdout);
            }
            if !outcome.stderr.is_empty() {
                eprint!("{}", outcome.stderr);
            }
            if outcome.timed_out {
                bail!("script '{}' timed out after {}ms", script.title, outcome.duration_ms);
            }
            match outcome.exit_code {
                Some(0) => Ok(()),
                Some(code) => std::process::exit(code),
                None => bail!("script '{}' terminated by signal", script.title),
            }
        }
        ScriptCliCommand::Dir => {
            println!("{}", paths.scripts_dir().display());
            Ok(())
        }
    }
}

fn run_doctor() -> Result<()> {
    let paths = AiosPaths::discover()?;
    paths.ensure_dirs()?;
    let config = AiosConfig::load_or_default(&paths)?;
    let db = Database::open(&paths.database_file())?;
    let compositor = detect_compositor();
    let portal_scheme = read_system_color_scheme();
    let resolved_theme = config.theme.mode.resolve(portal_scheme.prefers_dark());
    let palette = palette_for(resolved_theme);
    let daemon_running = IpcClient::new(&paths.runtime_socket).is_daemon_running();
    let discovered_apps = aios_builtin::scan_desktop_applications();
    let apps_with_icons = discovered_apps
        .iter()
        .filter(|a| a.icon_path.is_some())
        .count();
    let wasm_plugins = aios_plugin_host::discover_layered_plugins(
        &paths.bundled_plugins_dirs(),
        &paths.plugins_dir(),
    );
    let script_cmds = discover_script_commands(&paths.scripts_dir());

    tracing::info!(
        compositor = ?compositor,
        ?portal_scheme,
        daemon_running,
        discovered_apps = discovered_apps.len(),
        apps_with_icons,
        schema_version = db.schema_version()?,
        journal_mode = %db.journal_mode()?,
        "aios doctor check complete"
    );

    println!("AIOS Doctor");
    println!("  compositor    : {compositor:?}");
    println!("  daemon_active : {daemon_running}");
    println!("  config_file   : {}", paths.config_file().display());
    println!("  database_file : {}", paths.database_file().display());
    println!("  cache_dir     : {}", paths.cache_dir.display());
    println!("  runtime_sock  : {}", paths.runtime_socket.display());
    println!("  portal_scheme : {portal_scheme:?}");
    println!(
        "  theme_mode    : {:?} -> {:?} (accent {})",
        config.theme.mode, resolved_theme, palette.accent_fg
    );
    println!("  sqlite_mode   : {}", db.journal_mode()?);
    println!("  schema_ver    : {}", db.schema_version()?);
    println!(
        "  core_builtins : {}",
        aios_builtin::planned_builtin_ids().join(", ")
    );
    println!(
        "  desktop_apps  : {} indexed ({} with resolved icons)",
        discovered_apps.len(),
        apps_with_icons
    );
    println!(
        "  wasm_plugins  : {} installed ({})",
        wasm_plugins.len(),
        paths.plugins_dir().display()
    );
    println!(
        "  script_cmds   : {} discovered ({})",
        script_cmds.len(),
        paths.scripts_dir().display()
    );
    Ok(())
}

fn dispatch_ipc(command: DaemonIpcCommand) -> Result<()> {
    let paths = AiosPaths::discover()?;
    let client = IpcClient::new(&paths.runtime_socket);
    match client.send(&command) {
        Ok(DaemonIpcResponse::Ok) => Ok(()),
        Ok(DaemonIpcResponse::Pong { version }) => {
            println!("aios daemon v{version} ready");
            Ok(())
        }
        Ok(DaemonIpcResponse::Status(status)) => {
            println!("AIOS Daemon Status");
            println!("  version    : {}", status.version);
            println!("  visible    : {}", status.visible);
            println!("  compositor : {}", status.compositor);
            println!("  theme      : {}", status.active_theme);
            Ok(())
        }
        Ok(DaemonIpcResponse::DmenuSelected { value, .. }) => {
            println!("{value}");
            Ok(())
        }
        Ok(DaemonIpcResponse::DmenuCancelled) => std::process::exit(1),
        Ok(DaemonIpcResponse::Error { message }) => {
            bail!("daemon returned error: {message}");
        }
        Err(err) => {
            bail!(
                "could not reach AIOS daemon at {} ({err}). Start it first with `aios daemon`.",
                paths.runtime_socket.display()
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_dmenu(
    prompt: Option<String>,
    lines: Option<usize>,
    icon: Option<String>,
    show_icons: bool,
    case_mode: DmenuCaseMode,
    allow_custom: bool,
    return_index: bool,
    standalone: bool,
    theme: Option<CliTheme>,
) -> Result<()> {
    let stdin_is_tty = std::io::stdin().is_terminal();
    if stdin_is_tty && !allow_custom {
        std::process::exit(1);
    }

    // Normalize local file paths (`~/` or relative paths against caller CWD) while leaving XDG
    // icon theme names as compact strings for 0ms lazy lookup inside the warm daemon `IconResolver`.
    let resolved_header_icon = icon.as_deref().and_then(normalize_dmenu_icon_spec);

    let mut initial_items = Vec::new();
    let mut initial_icons = Vec::new();
    let mut streaming = false;
    let mut stream_rx: Option<async_channel::Receiver<DmenuStreamMessage>> = None;

    if !stdin_is_tty {
        // Spawn a background reader immediately so slow pipelines (`find / | aios dmenu`)
        // open the AIOS window within milliseconds while items stream in live.
        let (tx, rx) = async_channel::bounded::<DmenuStreamMessage>(64);
        let (first_tx, first_rx) = std::sync::mpsc::sync_channel::<DmenuStreamMessage>(2);

        std::thread::Builder::new()
            .name("aios-dmenu-stdin".into())
            .spawn(move || {
                let stdin = std::io::stdin();
                let mut batch_items = Vec::with_capacity(256);
                let mut batch_icons = Vec::with_capacity(256);
                let mut batch_has_icons = false;
                let mut sent_first = false;
                let mut last_flush = Instant::now();

                let emit = |sent_first: &mut bool, msg: DmenuStreamMessage| -> bool {
                    if !*sent_first {
                        *sent_first = true;
                        first_tx.send(msg).is_ok()
                    } else {
                        tx.send_blocking(msg).is_ok()
                    }
                };

                for line in stdin.lock().lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if line.is_empty() {
                        continue;
                    }
                    let (label, row_icon) = parse_dmenu_line(&line);
                    if label.is_empty() {
                        continue;
                    }
                    let normalized_icon = row_icon.as_deref().and_then(normalize_dmenu_icon_spec);
                    if normalized_icon.is_some() {
                        batch_has_icons = true;
                    }
                    batch_items.push(label);
                    batch_icons.push(normalized_icon);

                    let threshold = if !sent_first { 16 } else { 256 };
                    let interval = if !sent_first {
                        Duration::from_millis(12)
                    } else {
                        Duration::from_millis(25)
                    };

                    if batch_items.len() >= threshold || last_flush.elapsed() >= interval {
                        let icons_out = if batch_has_icons {
                            std::mem::take(&mut batch_icons)
                        } else {
                            batch_icons.clear();
                            Vec::new()
                        };
                        batch_has_icons = false;
                        last_flush = Instant::now();
                        if !emit(
                            &mut sent_first,
                            DmenuStreamMessage::Batch {
                                items: std::mem::take(&mut batch_items),
                                item_icons: icons_out,
                            },
                        ) {
                            return;
                        }
                    }
                }

                if !batch_items.is_empty() {
                    let icons_out = if batch_has_icons {
                        std::mem::take(&mut batch_icons)
                    } else {
                        Vec::new()
                    };
                    if !emit(
                        &mut sent_first,
                        DmenuStreamMessage::Batch {
                            items: batch_items,
                            item_icons: icons_out,
                        },
                    ) {
                        return;
                    }
                }
                let _ = emit(&mut sent_first, DmenuStreamMessage::Eof);
            })
            .ok();

        // Tiny 5ms peek: catches immediate empty stdin (exit 1 without flashing a window)
        // and instant static lists (`echo -e "A\nB"`), while slow `find` opens immediately!
        match first_rx.recv_timeout(Duration::from_millis(5)) {
            Ok(DmenuStreamMessage::Eof) => {
                if !allow_custom {
                    std::process::exit(1);
                }
            }
            Ok(DmenuStreamMessage::Batch { items, item_icons }) => {
                initial_items = items;
                initial_icons = item_icons;
                match rx.try_recv() {
                    Ok(DmenuStreamMessage::Eof) => {
                        streaming = false;
                    }
                    Ok(DmenuStreamMessage::Batch {
                        items: more_items,
                        item_icons: more_icons,
                    }) => {
                        let prev_len = initial_items.len();
                        let more_has_icons = more_icons.iter().any(Option::is_some);
                        if more_has_icons || !initial_icons.is_empty() {
                            initial_icons.resize(prev_len, None);
                            let mut padded = more_icons;
                            padded.resize(more_items.len(), None);
                            initial_icons.extend(padded);
                        }
                        initial_items.extend(more_items);
                        streaming = true;
                        stream_rx = Some(rx);
                    }
                    Err(async_channel::TryRecvError::Empty) => {
                        streaming = true;
                        stream_rx = Some(rx);
                    }
                    Err(async_channel::TryRecvError::Closed) => {
                        streaming = false;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Producer (`find`, etc.) is still working: bridge `first_rx` + `rx` and open UI NOW!
                let (merged_tx, merged_rx) = async_channel::bounded::<DmenuStreamMessage>(64);
                std::thread::Builder::new()
                    .name("aios-dmenu-bridge".into())
                    .spawn(move || {
                        if let Ok(first) = first_rx.recv() {
                            let is_eof = matches!(first, DmenuStreamMessage::Eof);
                            if merged_tx.send_blocking(first).is_err() || is_eof {
                                return;
                            }
                        }
                        while let Ok(msg) = rx.recv_blocking() {
                            let is_eof = matches!(msg, DmenuStreamMessage::Eof);
                            if merged_tx.send_blocking(msg).is_err() || is_eof {
                                break;
                            }
                        }
                    })
                    .ok();
                streaming = true;
                stream_rx = Some(merged_rx);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if !allow_custom {
                    std::process::exit(1);
                }
            }
        }
    }

    let theme_str = theme.map(|t| match t {
        CliTheme::System => "system".to_string(),
        CliTheme::Dark => "dark".to_string(),
        CliTheme::Light => "light".to_string(),
    });

    let request = DmenuRequest {
        prompt,
        items: initial_items,
        lines,
        case_mode,
        allow_custom,
        return_index,
        theme: theme_str,
        icon: resolved_header_icon,
        show_icons: show_icons || !initial_icons.is_empty(),
        item_icons: initial_icons,
        streaming,
    };

    let paths = AiosPaths::discover()?;

    let outcome = if !standalone {
        let client = IpcClient::new(&paths.runtime_socket);
        match client.start_dmenu_stream(&request) {
            Ok(session) => {
                let (mut writer, reader) = session.split();
                if streaming && let Some(rx) = stream_rx.take() {
                    std::thread::Builder::new()
                        .name("aios-dmenu-ipc-tx".into())
                        .spawn(move || {
                            while let Ok(msg) = rx.recv_blocking() {
                                let is_eof = matches!(msg, DmenuStreamMessage::Eof);
                                match writer.send_stream(&msg) {
                                    Ok(true) if !is_eof => {}
                                    _ => break,
                                }
                            }
                        })
                        .ok();
                }
                match reader.wait_response() {
                    Ok(DaemonIpcResponse::DmenuSelected { value, index }) => {
                        DmenuOutcome::Selected { value, index }
                    }
                    Ok(DaemonIpcResponse::DmenuCancelled) => DmenuOutcome::Cancelled,
                    Ok(DaemonIpcResponse::Error { message }) => {
                        bail!("daemon dmenu error: {message}");
                    }
                    Ok(_) => DmenuOutcome::Cancelled,
                    Err(_) => DmenuOutcome::Cancelled,
                }
            }
            Err(_) => {
                // Daemon not running: seamlessly fall back to standalone GPUI window with live stream.
                let config = AiosConfig::load_or_default(&paths)?;
                run_dmenu_app_streaming(config, request, stream_rx)
            }
        }
    } else {
        let config = AiosConfig::load_or_default(&paths)?;
        run_dmenu_app_streaming(config, request, stream_rx)
    };

    match outcome {
        DmenuOutcome::Selected { value, index } => {
            if return_index {
                match index {
                    Some(i) => println!("{i}"),
                    None => println!("-1"),
                }
            } else {
                println!("{value}");
            }
            Ok(())
        }
        DmenuOutcome::Cancelled => std::process::exit(1),
    }
}

/// Parses a single `dmenu` stdin line, supporting optional rofi-style metadata:
/// `Label\0icon\x1f<icon-name-or-path>` (plus additional `\x1f`-delimited key/value pairs).
fn parse_dmenu_line(raw: &str) -> (String, Option<String>) {
    let Some((label, meta)) = raw.split_once('\0') else {
        return (raw.to_owned(), None);
    };
    let mut icon = None;
    let mut parts = meta.split('\x1f');
    while let (Some(key), Some(val)) = (parts.next(), parts.next()) {
        if key.trim().eq_ignore_ascii_case("icon") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                icon = Some(trimmed.to_owned());
            }
        }
    }
    (label.to_owned(), icon)
}

/// Normalizes a `--icon` or per-row `\0icon\x1f...` spec on the CLI side:
/// expands `~/` and canonicalizes relative file paths that exist on disk (so daemon CWD doesn't matter),
/// while passing XDG icon theme names (`camera-photo`) and short badges straight through with 0ms overhead.
fn normalize_dmenu_icon_spec(spec: &str) -> Option<String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }

    let looks_like_path = trimmed.starts_with("~/")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('/')
        || trimmed.contains('/');

    if looks_like_path {
        let expanded = if let Some(rest) = trimmed.strip_prefix("~/")
            && let Ok(home) = std::env::var("HOME")
        {
            PathBuf::from(home).join(rest)
        } else {
            PathBuf::from(trimmed)
        };
        if expanded.is_file() {
            let abs = std::fs::canonicalize(&expanded).unwrap_or(expanded);
            return Some(abs.to_string_lossy().into_owned());
        }
        return None;
    }

    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_plain_and_rofi_icon_dmenu_lines() {
        assert_eq!(parse_dmenu_line("Full"), ("Full".to_string(), None));
        assert_eq!(
            parse_dmenu_line("Full\0icon\x1fcamera-photo"),
            ("Full".to_string(), Some("camera-photo".to_string()))
        );
        assert_eq!(
            parse_dmenu_line("Area\0meta\x1fregion\x1ficon\x1f/tmp/area.svg"),
            ("Area".to_string(), Some("/tmp/area.svg".to_string()))
        );
    }

    #[test]
    fn resolves_local_svg_icon_and_badge_fallback() {
        let svg_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .join("plugins/com.aios.calculator/icon.svg");
        let resolved = normalize_dmenu_icon_spec(&svg_path.to_string_lossy());
        assert!(resolved.as_deref().is_some_and(|p| p.ends_with("icon.svg")));

        assert_eq!(
            normalize_dmenu_icon_spec("camera-photo"),
            Some("camera-photo".to_string())
        );
        assert_eq!(normalize_dmenu_icon_spec("QR"), Some("QR".to_string()));
        assert_eq!(normalize_dmenu_icon_spec("/nonexistent/icon.svg"), None);
    }
}
