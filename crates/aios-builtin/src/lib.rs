//! Native built-in AIOS plugins (applications, calculator, clipboard, window management, files).

use aios_core::Result;
use aios_plugin_sdk::{CommandSpec, ViewNode};

/// Trait implemented by first-party native Rust plugins.
pub trait BuiltinPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn commands(&self) -> Vec<CommandSpec>;
    fn render_command(&self, command_id: &str, query: &str) -> Result<ViewNode>;
}

/// Planned built-in plugin identifiers shipped in v1 local-first releases.
pub fn planned_builtin_ids() -> &'static [&'static str] {
    &[
        "aios.applications",
        "aios.calculator",
        "aios.quicklinks",
        "aios.window-management",
        "aios.system",
        "aios.clipboard",
        "aios.files",
        "aios.snippets",
        "aios.emoji",
        "aios.notes",
        "aios.processes",
        "aios.color-picker",
        "aios.focus",
    ]
}
