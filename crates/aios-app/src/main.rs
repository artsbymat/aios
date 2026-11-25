use aios_core::{AiosConfig, AiosPaths, Database, LogFormat, LogOptions, init_logging};
use aios_daemon::DaemonIpcCommand;
use aios_ui::palette_for;
use aios_wayland::detect_compositor;
use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Inspect resolved XDG paths, configuration, SQLite WAL status, and detected compositor.
    Doctor,
    /// Start the AIOS background daemon (full Wayland/GPUI loop arrives in Phase 2).
    Daemon,
    /// Toggle the main AIOS launcher overlay.
    Toggle,
    /// Show the main AIOS launcher overlay.
    Show,
    /// Hide the main AIOS launcher overlay.
    Hide,
    /// Open an AIOS deep link URI (e.g. aios://extensions/...).
    Open { uri: String },
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
        Command::Daemon => {
            tracing::info!(
                "aios daemon bootstrap ready; GPUI layer-shell loop scheduled for Phase 2"
            );
            Ok(())
        }
        Command::Toggle => dispatch_ipc_stub(DaemonIpcCommand::Toggle),
        Command::Show => dispatch_ipc_stub(DaemonIpcCommand::Show),
        Command::Hide => dispatch_ipc_stub(DaemonIpcCommand::Hide),
        Command::Open { uri } => dispatch_ipc_stub(DaemonIpcCommand::Open { uri }),
    }
}

fn run_doctor() -> Result<()> {
    let paths = AiosPaths::discover()?;
    paths.ensure_dirs()?;
    let config = AiosConfig::load_or_default(&paths)?;
    let db = Database::open(&paths.database_file())?;
    let compositor = detect_compositor();
    let resolved_theme = config.theme.mode.resolve(true);
    let palette = palette_for(resolved_theme);

    tracing::info!(
        compositor = ?compositor,
        schema_version = db.schema_version()?,
        journal_mode = %db.journal_mode()?,
        "aios doctor check complete"
    );

    println!("AIOS Doctor");
    println!("  compositor    : {compositor:?}");
    println!("  config_file   : {}", paths.config_file().display());
    println!("  database_file : {}", paths.database_file().display());
    println!("  cache_dir     : {}", paths.cache_dir.display());
    println!("  runtime_sock  : {}", paths.runtime_socket.display());
    println!(
        "  theme_mode    : {:?} (accent {})",
        config.theme.mode, palette.accent_fg
    );
    println!("  sqlite_mode   : {}", db.journal_mode()?);
    println!("  schema_ver    : {}", db.schema_version()?);
    println!(
        "  builtins      : {}",
        aios_builtin::planned_builtin_ids().len()
    );
    Ok(())
}

fn dispatch_ipc_stub(command: DaemonIpcCommand) -> Result<()> {
    let paths = AiosPaths::discover()?;
    tracing::info!(
        socket = %paths.runtime_socket.display(),
        ?command,
        "IPC socket server will be wired in Phase 2"
    );
    Ok(())
}
