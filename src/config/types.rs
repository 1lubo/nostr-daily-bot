//! Configuration struct definitions.

use serde::Deserialize;

/// Root configuration structure.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub relays: RelayConfig,
    pub schedule: ScheduleConfig,
    pub identity: IdentityConfig,
    pub messages: MessageConfig,
}

/// Relay connection settings.
#[derive(Debug, Deserialize)]
pub struct RelayConfig {
    /// List of relay WebSocket URLs (required, at least one).
    pub urls: Vec<String>,
}

/// Posting schedule settings.
#[derive(Debug, Deserialize)]
pub struct ScheduleConfig {
    /// Cron expression for posting schedule.
    pub cron: String,

    /// Timezone for cron interpretation (default: UTC).
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

/// Identity/key settings.
#[derive(Debug, Deserialize)]
pub struct IdentityConfig {
    /// Path to file containing private key (nsec or hex).
    pub private_key_file: Option<String>,

    /// Inline private key (not recommended for production).
    pub private_key: Option<String>,
}

/// Message template settings.
#[derive(Debug, Deserialize)]
pub struct MessageConfig {
    /// List of message templates to rotate through.
    pub templates: Vec<String>,

    /// Rotation strategy: "sequential" or "random".
    #[serde(default = "default_rotation")]
    pub rotation: String,
}

fn default_rotation() -> String {
    "sequential".to_string()
}

