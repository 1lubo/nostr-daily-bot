//! BTCPay Server configuration.

use std::env;

use tracing::info;

/// Configuration for BTCPay Server integration.
#[derive(Debug, Clone)]
pub struct BTCPayConfig {
    /// Base URL of the BTCPay Server instance (e.g., "https://mainnet.demo.btcpayserver.org")
    pub base_url: String,
    /// API key with invoice permissions
    pub api_key: String,
    /// Store ID
    pub store_id: String,
    /// Webhook secret for verifying incoming webhooks
    pub webhook_secret: String,
    /// Default tip amount in satoshis
    pub default_tip_sats: u64,
}

impl BTCPayConfig {
    /// Load configuration from environment variables.
    ///
    /// Required env vars:
    /// - `BTCPAY_BASE_URL`
    /// - `BTCPAY_API_KEY`
    /// - `BTCPAY_STORE_ID`
    /// - `BTCPAY_WEBHOOK_SECRET`
    ///
    /// Optional:
    /// - `BTCPAY_DEFAULT_TIP_SATS` (default: 5000)
    ///
    /// Returns `None` if any required variable is missing (tipping disabled).
    pub fn from_env() -> Option<Self> {
        let base_url = env::var("BTCPAY_BASE_URL").ok()?;
        let api_key = env::var("BTCPAY_API_KEY").ok()?;
        let store_id = env::var("BTCPAY_STORE_ID").ok()?;
        let webhook_secret = env::var("BTCPAY_WEBHOOK_SECRET").ok()?;

        let default_tip_sats = env::var("BTCPAY_DEFAULT_TIP_SATS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000);

        info!(
            base_url = %base_url,
            store_id = %store_id,
            default_tip_sats = default_tip_sats,
            "BTCPay configuration loaded"
        );

        Some(Self {
            base_url,
            api_key,
            store_id,
            webhook_secret,
            default_tip_sats,
        })
    }

    /// Check if the configuration appears valid.
    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.is_empty() {
            return Err("BTCPAY_BASE_URL cannot be empty".to_string());
        }
        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            return Err("BTCPAY_BASE_URL must start with http:// or https://".to_string());
        }
        if self.api_key.is_empty() {
            return Err("BTCPAY_API_KEY cannot be empty".to_string());
        }
        if self.store_id.is_empty() {
            return Err("BTCPAY_STORE_ID cannot be empty".to_string());
        }
        if self.webhook_secret.is_empty() {
            return Err("BTCPAY_WEBHOOK_SECRET cannot be empty".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let config = BTCPayConfig {
            base_url: "https://btcpay.example.com".to_string(),
            api_key: "test-api-key".to_string(),
            store_id: "test-store-id".to_string(),
            webhook_secret: "test-secret".to_string(),
            default_tip_sats: 5000,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_url() {
        let config = BTCPayConfig {
            base_url: "not-a-url".to_string(),
            api_key: "test-api-key".to_string(),
            store_id: "test-store-id".to_string(),
            webhook_secret: "test-secret".to_string(),
            default_tip_sats: 5000,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_empty_api_key() {
        let config = BTCPayConfig {
            base_url: "https://btcpay.example.com".to_string(),
            api_key: "".to_string(),
            store_id: "test-store-id".to_string(),
            webhook_secret: "test-secret".to_string(),
            default_tip_sats: 5000,
        };
        assert!(config.validate().is_err());
    }
}
