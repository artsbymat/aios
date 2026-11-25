//! Unified error types for `aios-core` and downstream AIOS crates.

use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, AiosError>;

#[derive(Debug, Error)]
pub enum AiosError {
    #[error("missing home directory environment variable (HOME)")]
    MissingHomeDir,

    #[error("filesystem error at {path}: {source}")]
    IoPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse configuration file {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize configuration: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("database migration {version} ({name}) failed: {source}")]
    Migration {
        version: u32,
        name: &'static str,
        #[source]
        source: rusqlite::Error,
    },

    #[error("invalid configuration value: {0}")]
    InvalidConfig(String),
}
