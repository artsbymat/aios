//! Core shared types, XDG path resolution, configuration, SQLite WAL storage,
//! structured logging, and declarative multi-view AST for AIOS.

pub mod config;
pub mod error;
pub mod logging;
pub mod paths;
pub mod storage;
pub mod view;

pub use config::{AiosConfig, ResolvedTheme, ThemeMode};
pub use error::{AiosError, Result};
pub use logging::{LogFormat, LogOptions, init_logging};
pub use paths::AiosPaths;
pub use storage::{Database, LATEST_SCHEMA_VERSION};
