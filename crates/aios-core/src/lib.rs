//! Core shared types, XDG path resolution, configuration, SQLite WAL storage,
//! structured logging, and declarative multi-view AST for AIOS.

#[cfg(feature = "host")]
pub mod config;
#[cfg(feature = "host")]
pub mod error;
#[cfg(feature = "host")]
pub mod logging;
#[cfg(feature = "host")]
pub mod paths;
#[cfg(feature = "host")]
pub mod storage;
pub mod view;

#[cfg(feature = "host")]
pub use config::{AiosConfig, PluginRegistryConfig, ResolvedTheme, ThemeMode};
#[cfg(feature = "host")]
pub use error::{AiosError, Result};
#[cfg(feature = "host")]
pub use logging::{LogFormat, LogOptions, init_logging};
#[cfg(feature = "host")]
pub use paths::AiosPaths;
#[cfg(feature = "host")]
pub use storage::{Database, LATEST_SCHEMA_VERSION, PluginStateRecord, StoredFrecency};
