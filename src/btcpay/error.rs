//! BTCPay error types.

use std::fmt;

/// Errors that can occur when interacting with BTCPay Server.
#[derive(Debug)]
pub enum BTCPayError {
    /// HTTP request failed
    Network(reqwest::Error),
    /// API returned an error response
    Api { status: u16, message: String },
    /// Failed to parse response
    InvalidResponse(String),
    /// Webhook signature verification failed
    WebhookVerificationFailed,
    /// Configuration error
    Config(String),
}

impl fmt::Display for BTCPayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BTCPayError::Network(e) => write!(f, "Network error: {}", e),
            BTCPayError::Api { status, message } => {
                write!(f, "BTCPay API error ({}): {}", status, message)
            }
            BTCPayError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            BTCPayError::WebhookVerificationFailed => {
                write!(f, "Webhook signature verification failed")
            }
            BTCPayError::Config(msg) => write!(f, "Configuration error: {}", msg),
        }
    }
}

impl std::error::Error for BTCPayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BTCPayError::Network(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for BTCPayError {
    fn from(err: reqwest::Error) -> Self {
        BTCPayError::Network(err)
    }
}

/// Result type for BTCPay operations.
pub type Result<T> = std::result::Result<T, BTCPayError>;
