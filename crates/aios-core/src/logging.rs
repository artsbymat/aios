//! Structured `tracing` subscriber setup for AIOS binaries and tests.

use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFormat {
    #[default]
    Compact,
    Pretty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogOptions {
    pub default_filter: String,
    pub format: LogFormat,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            default_filter: "aios=info,warn".to_string(),
            format: LogFormat::Compact,
        }
    }
}

/// Initializes the global `tracing` subscriber safely (no-op if already initialized).
pub fn init_logging(options: &LogOptions) {
    let env_filter = EnvFilter::try_from_env("AIOS_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new(&options.default_filter));

    match options.format {
        LogFormat::Compact => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(true)
                .compact()
                .try_init();
        }
        LogFormat::Pretty => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(true)
                .pretty()
                .try_init();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_is_safe_to_call_repeatedly() {
        let opts = LogOptions::default();
        init_logging(&opts);
        init_logging(&opts);
    }
}
