//! Error types for the Nostr module.

use thiserror::Error;

/// Errors that can occur in the Nostr client.
#[derive(Debug, Error)]
pub enum NostrError {
    #[error("Invalid private key format: {0}")]
    InvalidKey(String),

    #[error("No relays connected - cannot publish")]
    NoRelaysConnected,

    #[error("Failed to publish event: {0}")]
    PublishFailed(String),

    #[error("SDK error: {0}")]
    Sdk(#[from] nostr_sdk::client::Error),
}

/// Result type for Nostr operations.
pub type Result<T> = std::result::Result<T, NostrError>;
