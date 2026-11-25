//! Core built-in AIOS capabilities: Root Search Orchestrator and XDG Application Launcher.
//!
//! All other productivity tools (Calculator, Quicklinks, Window Management, System Commands,
//! Clipboard History, Files, Snippets, etc.) are implemented as External Plugins on top of
//! the Phase 4 Hybrid Plugin Runtime.

pub mod desktop;
pub mod icons;
pub mod root_search;

use aios_core::Result;
use aios_plugin_sdk::{CommandSpec, ViewNode};

pub use desktop::{
    AppSource, DesktopApp, launch_desktop_app, open_url_detached, parse_exec_command,
    resolve_terminal_emulator, scan_desktop_applications, scan_desktop_applications_in,
};
pub use icons::IconResolver;
pub use root_search::{
    ExternalRootEntry, RootActionOutcome, RootEntry, RootEntryKind, RootSearchOrchestrator,
};

/// Trait implemented by minimal in-process core plugins.
pub trait BuiltinPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn commands(&self) -> Vec<CommandSpec>;
    fn render_command(&self, command_id: &str, query: &str) -> Result<ViewNode>;
}

/// Core built-in identifiers compiled directly into AIOS.
#[must_use]
pub fn planned_builtin_ids() -> &'static [&'static str] {
    &["aios.root", "aios.applications"]
}
