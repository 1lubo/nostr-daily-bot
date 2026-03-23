//! Nostr client module for relay connections and event publishing.

mod error;

use nostr_sdk::prelude::*;
use tracing::{debug, info, instrument, warn};

pub use error::{NostrError, Result};

/// Configuration for the Nostr client.
#[derive(Debug, Clone)]
pub struct NostrClientConfig {
    /// List of relay URLs to connect to.
    pub relays: Vec<String>,
    /// Fallback relays if primary relays fail.
    pub fallback_relays: Vec<String>,
}

impl Default for NostrClientConfig {
    fn default() -> Self {
        Self {
            relays: vec![
                "wss://relay.damus.io".to_string(),
                "wss://nos.lol".to_string(),
                "wss://relay.nostr.band".to_string(),
            ],
            fallback_relays: vec![
                "wss://nostr.wine".to_string(),
                "wss://relay.snort.social".to_string(),
            ],
        }
    }
}

/// Wrapper around nostr-sdk Client.
pub struct NostrClient {
    client: Client,
    config: NostrClientConfig,
}

impl NostrClient {
    /// Parse keys from either hex or bech32 format (auto-detect).
    pub fn keys_parse(private_key: &str) -> Result<Keys> {
        Keys::parse(private_key).map_err(|e| NostrError::InvalidKey(e.to_string()))
    }

    /// Create a new NostrClient with the given keys and configuration.
    pub async fn new(keys: Keys, config: NostrClientConfig) -> Result<Self> {
        let client = Client::builder().signer(keys).build();
        Ok(Self { client, config })
    }

    /// Create with default configuration.
    pub async fn with_keys(keys: Keys) -> Result<Self> {
        Self::new(keys, NostrClientConfig::default()).await
    }

    /// Connect to all configured relays.
    #[instrument(skip(self), fields(relay_count = self.config.relays.len()))]
    pub async fn connect(&self) -> Result<()> {
        info!("Starting relay connections");

        for url in &self.config.relays {
            if let Err(e) = self.client.add_relay(url).await {
                warn!(relay_url = %url, error = %e, "Failed to add relay");
            }
        }

        self.client.connect().await;

        let connected = self.connected_relay_count().await;
        if connected == 0 {
            info!("No primary relays connected, trying fallbacks...");
            self.connect_fallback_relays().await?;
        }

        let final_count = self.connected_relay_count().await;
        if final_count == 0 {
            return Err(NostrError::NoRelaysConnected);
        }

        info!(connected_relays = final_count, "Connection complete");
        Ok(())
    }

    async fn connect_fallback_relays(&self) -> Result<()> {
        for url in &self.config.fallback_relays {
            match self.client.add_relay(url).await {
                Ok(_) => debug!(relay_url = %url, "Added fallback relay"),
                Err(e) => warn!(relay_url = %url, error = %e, "Failed to add fallback"),
            }
        }
        self.client.connect().await;
        Ok(())
    }

    /// Get count of connected relays.
    pub async fn connected_relay_count(&self) -> usize {
        self.client.relays().await.len()
    }

    /// Shutdown the client.
    pub async fn shutdown(&self) {
        info!("Shutting down Nostr client");
        self.client.disconnect().await;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Event Publishing
    // ─────────────────────────────────────────────────────────────────────────────

    /// Publish a text note (kind 1 event).
    #[instrument(skip(self, content), fields(content_len = content.len()))]
    pub async fn publish_text_note(&self, content: &str) -> Result<EventId> {
        info!("Publishing text note");

        let builder = EventBuilder::text_note(content);
        let output = self
            .client
            .send_event_builder(builder)
            .await
            .map_err(NostrError::Sdk)?;

        let event_id = *output.id();

        info!(
            event_id = %event_id.to_bech32().unwrap_or_else(|_| event_id.to_hex()),
            success_count = output.success.len(),
            failed_count = output.failed.len(),
            "Text note published"
        );

        if output.success.is_empty() {
            return Err(NostrError::PublishFailed(
                "Event not published to any relay".to_string(),
            ));
        }

        Ok(event_id)
    }
}
