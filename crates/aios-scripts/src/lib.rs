//! Standalone Script Commands runtime: metadata header discovery (`@aios.*`),
//! timeout-safe subprocess execution, inline summary caching, and dedicated
//! Script Commands Manager views, completely decoupled from WASM plugins.

pub mod host;
pub mod manager_view;
pub mod runner;

pub use host::{RootScriptEntry, ScriptActionResult, ScriptHost, ScriptRefreshSpec};
pub use manager_view::build_scripts_manager_view;
pub use runner::{
    MAX_CONSECUTIVE_REFRESH_FAILURES, MIN_REFRESH_INTERVAL_SECS, ScriptCommand, ScriptMode,
    ScriptRefreshStatus, ScriptRunOutcome, build_full_output_detail_view, discover_script_commands,
    execute_script_command, parse_script_metadata, script_output_frame_id,
};
