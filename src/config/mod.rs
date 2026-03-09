//! Configuration module for loading and validating settings.

mod loader;
mod types;
mod validation;

pub use types::Config;

use thiserror::Error;

/// Errors that can occur during configuration loading and validation.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    FileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse TOML: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Invalid cron expression '{expr}': {reason}")]
    InvalidCron { expr: String, reason: String },

    #[error("No relays configured - at least one relay URL is required")]
    NoRelays,

    #[error("No private key configured - set via config file or NOSTR_PRIVATE_KEY env var")]
    NoPrivateKey,

    #[error("No message templates configured")]
    NoTemplates,

    #[error("Invalid relay URL '{url}': must start with ws:// or wss://")]
    InvalidRelayUrl { url: String },
}

