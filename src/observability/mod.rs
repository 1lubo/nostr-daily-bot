//! Observability module for logging and tracing.

use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable, colorized output for development.
    #[default]
    Pretty,
    /// JSON structured output for production.
    Json,
}

impl LogFormat {
    /// Parse from string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            _ => LogFormat::Pretty,
        }
    }
}

/// Configuration for the observability system.
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Output format: Pretty (dev) or JSON (prod).
    pub format: LogFormat,
    /// Default log level.
    pub default_level: Level,
    /// Whether to log span events.
    pub log_span_events: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            default_level: Level::INFO,
            log_span_events: false,
        }
    }
}

impl ObservabilityConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Self {
        let format = std::env::var("LOG_FORMAT")
            .map(|s| LogFormat::from_str(&s))
            .unwrap_or_default();

        Self {
            format,
            ..Default::default()
        }
    }
}

/// Initialize the global tracing subscriber.
pub fn init_logging(config: ObservabilityConfig) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(config.default_level.to_string()));

    let span_events = if config.log_span_events {
        FmtSpan::NEW | FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

    match config.format {
        LogFormat::Pretty => {
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(span_events)
                .with_target(true)
                .with_thread_ids(false)
                .with_ansi(true)
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }
        LogFormat::Json => {
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(span_events)
                .with_target(true)
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }
    }
}

