# Nostr Client Module Implementation Plan

## 1. Overview

### Description
A Rust wrapper module around the `nostr-sdk` crate that provides a clean, ergonomic interface for:
- Connecting to multiple Nostr relays
- Managing cryptographic keypairs (loading from hex/bech32 or generating new)
- Publishing text notes (kind 1 events)
- Handling connection failures with retry logic and fallback relays

### Goals and Success Criteria
- [ ] Wrapper struct that encapsulates `nostr_sdk::Client`
- [ ] Support for loading keys from hex or bech32 private key formats
- [ ] Support for generating new random keypairs
- [ ] Connect to multiple relays with automatic reconnection
- [ ] Graceful error handling for connection failures
- [ ] Async operations using tokio runtime
- [ ] Custom error types with proper error propagation

### Scope Boundaries
- **Included**: Key management, relay connections, publishing text notes (kind 1)
- **Excluded**: Subscriptions, event fetching, NIP-46 remote signing, advanced gossip features

---

## 2. Prerequisites

### Dependencies (Cargo.toml)
```toml
[dependencies]
nostr-sdk = "0.43"    # High-level SDK (includes nostr crate)
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
thiserror = "2"       # Error derive macro
tracing = "0.1"       # Logging
```

### Environment Requirements
- Rust 1.70+ (async/await, edition 2021)
- Tokio runtime for async operations

---

## 3. Implementation Steps

### Step 1: Define Module Structure and Error Types

**Files to create:** `src/nostr/mod.rs`, `src/nostr/error.rs`

Create a dedicated error enum for the Nostr module:

```rust
// src/nostr/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NostrError {
    #[error("Invalid private key format: {0}")]
    InvalidKey(String),
    
    #[error("Failed to connect to relay '{url}': {source}")]
    ConnectionFailed {
        url: String,
        #[source]
        source: nostr_sdk::client::Error,
    },
    
    #[error("No relays connected - cannot publish")]
    NoRelaysConnected,
    
    #[error("Failed to publish event: {0}")]
    PublishFailed(String),
    
    #[error("Signer not configured")]
    SignerNotConfigured,
    
    #[error("SDK error: {0}")]
    Sdk(#[from] nostr_sdk::client::Error),
    
    #[error("Nostr protocol error: {0}")]
    Protocol(#[from] nostr::prelude::Error),
}

pub type Result<T> = std::result::Result<T, NostrError>;
```

---

### Step 2: Define the NostrClient Wrapper Struct

**Files to modify:** `src/nostr/mod.rs`

```rust
// src/nostr/mod.rs
pub mod error;

use std::sync::Arc;
use std::time::Duration;

use nostr_sdk::prelude::*;
use tracing::{debug, error, info, warn};

pub use self::error::{NostrError, Result};

/// Configuration for the Nostr client
#[derive(Debug, Clone)]
pub struct NostrClientConfig {
    /// List of relay URLs to connect to
    pub relays: Vec<String>,
    /// Fallback relays if primary relays fail
    pub fallback_relays: Vec<String>,
    /// Connection timeout per relay
    pub connect_timeout: Duration,
    /// Whether to wait for connections before returning
    pub wait_for_connection: bool,
    /// Maximum time to wait for relay connections
    pub connection_wait_timeout: Duration,
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
            connect_timeout: Duration::from_secs(15),
            wait_for_connection: true,
            connection_wait_timeout: Duration::from_secs(30),
        }
    }
}

/// Wrapper around nostr-sdk Client
pub struct NostrClient {
    client: Client,
    config: NostrClientConfig,
    keys: Keys,
}
```

---

### Step 3: Implement Key Management

**Key operations:**
- Parse from hex string
- Parse from bech32 (nsec) string  
- Generate new random keypair

```rust
impl NostrClient {
    /// Create keys from a hex-encoded private key
    pub fn keys_from_hex(hex_private_key: &str) -> Result<Keys> {
        let secret_key = SecretKey::from_hex(hex_private_key)
            .map_err(|e| NostrError::InvalidKey(e.to_string()))?;
        Ok(Keys::new(secret_key))
    }
    
    /// Create keys from a bech32-encoded private key (nsec1...)
    pub fn keys_from_bech32(bech32_private_key: &str) -> Result<Keys> {
        let secret_key = SecretKey::from_bech32(bech32_private_key)
            .map_err(|e| NostrError::InvalidKey(e.to_string()))?;
        Ok(Keys::new(secret_key))
    }
    
    /// Parse keys from either hex or bech32 format (auto-detect)
    pub fn keys_parse(private_key: &str) -> Result<Keys> {
        Keys::parse(private_key)
            .map_err(|e| NostrError::InvalidKey(e.to_string()))
    }
    
    /// Generate a new random keypair
    pub fn keys_generate() -> Keys {
        Keys::generate()
    }
}
```

---

### Step 4: Implement Client Initialization with Builder Pattern

```rust
impl NostrClient {
    /// Create a new NostrClient with the given keys and configuration
    pub async fn new(keys: Keys, config: NostrClientConfig) -> Result<Self> {
        // Build the client with the signer (keys)
        let client = Client::builder()
            .signer(keys.clone())
            .connect_timeout(config.connect_timeout)
            .automatic_authentication(true) // NIP-42 support
            .build();

        Ok(Self {
            client,
            config,
            keys,
        })
    }

    /// Create with default configuration
    pub async fn with_keys(keys: Keys) -> Result<Self> {
        Self::new(keys, NostrClientConfig::default()).await
    }

    /// Get a reference to the underlying nostr-sdk Client
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Get the public key of the configured signer
    pub fn public_key(&self) -> PublicKey {
        self.keys.public_key()
    }

    /// Get the public key in bech32 format (npub1...)
    pub fn npub(&self) -> std::result::Result<String, nostr::prelude::Error> {
        self.keys.public_key().to_bech32()
    }
}
```

---

### Step 5: Implement Connection Management with Retry Logic

```rust
impl NostrClient {
    /// Connect to all configured relays
    pub async fn connect(&self) -> Result<()> {
        // Add primary relays
        for url in &self.config.relays {
            if let Err(e) = self.client.add_relay(url).await {
                warn!("Failed to add relay {}: {}", url, e);
            }
        }

        // Connect to relays
        if self.config.wait_for_connection {
            self.client
                .connect()
                .and_wait(self.config.connection_wait_timeout)
                .await;
        } else {
            self.client.connect().await;
        }

        // Check if any relays connected
        let connected_count = self.connected_relay_count().await;

        if connected_count == 0 {
            info!("No primary relays connected, trying fallback relays...");
            self.connect_fallback_relays().await?;
        }

        let final_count = self.connected_relay_count().await;
        if final_count == 0 {
            return Err(NostrError::NoRelaysConnected);
        }

        info!("Connected to {} relay(s)", final_count);
        Ok(())
    }

    /// Connect to fallback relays
    async fn connect_fallback_relays(&self) -> Result<()> {
        for url in &self.config.fallback_relays {
            match self.client.add_relay(url).await {
                Ok(_) => {
                    debug!("Added fallback relay: {}", url);
                }
                Err(e) => {
                    warn!("Failed to add fallback relay {}: {}", url, e);
                }
            }
        }

        // Wait for fallback connections
        self.client
            .connect()
            .and_wait(self.config.connection_wait_timeout)
            .await;

        Ok(())
    }

    /// Add a single relay and optionally connect immediately
    pub async fn add_relay(&self, url: &str) -> Result<bool> {
        self.client
            .add_relay(url)
            .and_connect()
            .await
            .map_err(|e| NostrError::ConnectionFailed {
                url: url.to_string(),
                source: e,
            })
    }

    /// Try to connect to a relay with timeout
    pub async fn try_connect_relay(&self, url: &str, timeout: Duration) -> Result<()> {
        self.client
            .try_connect_relay(url, timeout)
            .await
            .map_err(|e| NostrError::ConnectionFailed {
                url: url.to_string(),
                source: e,
            })
    }

    /// Get count of connected relays
    pub async fn connected_relay_count(&self) -> usize {
        let relays = self.client.relays().await;
        relays
            .values()
            .filter(|r| r.status().is_connected())
            .count()
    }

    /// Get list of connected relay URLs
    pub async fn connected_relays(&self) -> Vec<String> {
        let relays = self.client.relays().await;
        relays
            .iter()
            .filter(|(_, r)| r.status().is_connected())
            .map(|(url, _)| url.to_string())
            .collect()
    }

    /// Disconnect from all relays
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }

    /// Shutdown the client completely
    pub async fn shutdown(&self) {
        self.client.shutdown().await;
    }
}
```

---

### Step 6: Implement Event Publishing

```rust
impl NostrClient {
    /// Publish a text note (kind 1 event)
    pub async fn publish_text_note(&self, content: &str) -> Result<EventId> {
        let builder = EventBuilder::text_note(content);
        self.publish_event_builder(builder).await
    }

    /// Publish a text note with tags
    pub async fn publish_text_note_with_tags(
        &self,
        content: &str,
        tags: Vec<Tag>,
    ) -> Result<EventId> {
        let builder = EventBuilder::text_note(content).tags(tags);
        self.publish_event_builder(builder).await
    }

    /// Publish an event from an EventBuilder
    pub async fn publish_event_builder(&self, builder: EventBuilder) -> Result<EventId> {
        let output = self
            .client
            .send_event_builder(builder)
            .await
            .map_err(NostrError::Sdk)?;

        let event_id = output.id();

        // Log success/failure info
        if !output.success.is_empty() {
            info!(
                "Event {} published to {} relay(s)",
                event_id.to_bech32().unwrap_or_else(|_| event_id.to_hex()),
                output.success.len()
            );
        }

        if !output.failed.is_empty() {
            for (url, error) in &output.failed {
                warn!("Failed to publish to {}: {:?}", url, error);
            }
        }

        // Check if event was published to at least one relay
        if output.success.is_empty() {
            return Err(NostrError::PublishFailed(
                "Event not published to any relay".to_string(),
            ));
        }

        Ok(event_id.clone())
    }

    /// Publish a pre-signed event
    pub async fn publish_event(&self, event: &Event) -> Result<EventId> {
        let output = self
            .client
            .send_event(event)
            .await
            .map_err(NostrError::Sdk)?;

        if output.success.is_empty() {
            return Err(NostrError::PublishFailed(
                "Event not published to any relay".to_string(),
            ));
        }

        Ok(output.id().clone())
    }
}
```

---

## 4. File Changes Summary

### Files to Create

| File | Description |
|------|-------------|
| `src/nostr/mod.rs` | Main module with `NostrClient` struct and implementation |
| `src/nostr/error.rs` | Custom error types for the Nostr module |

### Files to Modify

| File | Change |
|------|--------|
| `src/lib.rs` or `src/main.rs` | Add `mod nostr;` declaration |
| `Cargo.toml` | Add dependencies: `nostr-sdk`, `thiserror`, `tracing` |

---

## 5. Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keys_from_hex() {
        let hex_key = "6b911fd37cdf5c81d4c0adb1ab7fa822ed253ab0ad9aa18d77257c88b29b718e";
        let keys = NostrClient::keys_from_hex(hex_key).unwrap();
        assert!(keys.public_key().to_hex().len() == 64);
    }

    #[test]
    fn test_keys_from_bech32() {
        let nsec = "nsec1j4c6269y9w0q2er2xjw8sv2ehyrtfxq3jwgdlxj6qfn8z4gjsq5qfvfk99";
        let keys = NostrClient::keys_from_bech32(nsec).unwrap();
        assert!(keys.public_key().to_hex().len() == 64);
    }

    #[test]
    fn test_keys_generate() {
        let keys = NostrClient::keys_generate();
        assert!(keys.public_key().to_hex().len() == 64);
        assert!(keys.secret_key().to_hex().len() == 64);
    }

    #[test]
    fn test_invalid_key() {
        let result = NostrClient::keys_from_hex("invalid");
        assert!(result.is_err());
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_connect_and_publish() {
    // Use test relay or mock
    let keys = NostrClient::keys_generate();
    let config = NostrClientConfig {
        relays: vec!["wss://relay.damus.io".to_string()],
        fallback_relays: vec![],
        connect_timeout: Duration::from_secs(10),
        wait_for_connection: true,
        connection_wait_timeout: Duration::from_secs(15),
    };

    let client = NostrClient::new(keys, config).await.unwrap();
    client.connect().await.unwrap();

    assert!(client.connected_relay_count().await > 0);

    let event_id = client
        .publish_text_note("Test note from integration test")
        .await
        .unwrap();

    assert!(event_id.to_hex().len() == 64);

    client.shutdown().await;
}
```

### Manual Testing Steps

1. Generate new keypair and verify public key format
2. Load existing private key (hex and bech32)
3. Connect to multiple relays and verify connection status
4. Publish a text note and verify it appears on relay
5. Test fallback relay logic by using invalid primary relays
6. Test graceful shutdown

---

## 6. Rollback Plan

1. **Code Rollback**: Revert the module addition via git
   ```bash
   git checkout HEAD~1 -- src/nostr/
   ```

2. **Dependencies**: Remove nostr-sdk from Cargo.toml if needed
   ```bash
   cargo remove nostr-sdk thiserror
   ```

3. **No Data Migration**: This module doesn't persist data, so no data rollback needed

---

## 7. Estimated Effort

| Task | Estimate |
|------|----------|
| Error types and module structure | 30 min |
| Key management implementation | 30 min |
| Client initialization with builder | 45 min |
| Connection management with retry | 1.5 hours |
| Event publishing | 45 min |
| Unit tests | 1 hour |
| Integration tests | 1 hour |
| Documentation | 30 min |
| **Total** | **~6 hours** |

### Complexity Assessment: **Medium**

- Uses established crate (`nostr-sdk`) with good documentation
- Async Rust patterns required (tokio)
- Error handling requires careful design
- Connection retry logic adds complexity

---

## 8. Usage Example

```rust
use crate::nostr::{NostrClient, NostrClientConfig, Result};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Option 1: Load from environment/config
    let private_key = std::env::var("NOSTR_PRIVATE_KEY")
        .expect("NOSTR_PRIVATE_KEY must be set");
    let keys = NostrClient::keys_parse(&private_key)?;

    // Option 2: Generate new keys
    // let keys = NostrClient::keys_generate();
    // println!("Generated npub: {}", keys.public_key().to_bech32()?);
    // println!("Generated nsec: {}", keys.secret_key().to_bech32()?);

    // Configure client
    let config = NostrClientConfig {
        relays: vec![
            "wss://relay.damus.io".to_string(),
            "wss://nos.lol".to_string(),
        ],
        fallback_relays: vec![
            "wss://relay.nostr.band".to_string(),
        ],
        connect_timeout: Duration::from_secs(15),
        wait_for_connection: true,
        connection_wait_timeout: Duration::from_secs(30),
    };

    // Create and connect
    let client = NostrClient::new(keys, config).await?;
    client.connect().await?;

    println!("Connected to {} relay(s)", client.connected_relay_count().await);

    // Publish a text note
    let event_id = client.publish_text_note("Hello, Nostr!").await?;
    println!("Published event: {}", event_id.to_bech32()?);

    // Cleanup
    client.shutdown().await;

    Ok(())
}
```

---

## 9. API Reference Summary

### Key Types from nostr-sdk

| Type | Description |
|------|-------------|
| `Keys` | Keypair (public + secret key) |
| `SecretKey` | Private/secret key |
| `PublicKey` | Public key |
| `Client` | High-level Nostr client |
| `EventBuilder` | Builder for creating events |
| `Event` | Signed Nostr event |
| `EventId` | Unique event identifier |
| `Tag` | Event tag (e.g., `["p", "pubkey"]`) |
| `Kind` | Event kind (e.g., `Kind::TextNote` = 1) |

### Key Methods from nostr-sdk

```rust
// Keys
Keys::generate() -> Keys
Keys::parse(s: &str) -> Result<Keys>
Keys::new(secret_key: SecretKey) -> Keys
SecretKey::from_hex(hex: &str) -> Result<SecretKey>
SecretKey::from_bech32(bech32: &str) -> Result<SecretKey>
keys.public_key() -> PublicKey
keys.secret_key() -> SecretKey

// Client
Client::builder() -> ClientBuilder
ClientBuilder::signer(signer) -> ClientBuilder
ClientBuilder::connect_timeout(duration) -> ClientBuilder
ClientBuilder::build() -> Client

// Relay Management
client.add_relay(url).await -> Result<bool>
client.add_relay(url).and_connect().await -> Result<bool>
client.connect().await
client.connect().and_wait(timeout).await
client.try_connect_relay(url, timeout).await -> Result<()>
client.disconnect().await
client.shutdown().await
client.relays().await -> HashMap<RelayUrl, Relay>

// Event Publishing
EventBuilder::text_note(content) -> EventBuilder
client.send_event_builder(builder).await -> Result<Output<EventId>>
client.send_event(event).await -> Result<Output<EventId>>
```

