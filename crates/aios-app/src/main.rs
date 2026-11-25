use aios_core::{AiosConfig, AiosPaths, Database, LogFormat, LogOptions, ThemeMode, init_logging};
use aios_daemon::{DaemonIpcCommand, DaemonIpcResponse};
use aios_ui::{palette_for, run_daemon_app, run_preview_app};
use aios_wayland::{IpcClient, detect_compositor, read_system_color_scheme};
use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliTheme {
    System,
    Dark,
    Light,
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
    /// Open an AIOS deep link URI (e.g. aios://demo/grid).
    Open { uri: String },
    /// Query live status from the running AIOS daemon.
    Status,
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
        Command::Status => dispatch_ipc(DaemonIpcCommand::Status),
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
