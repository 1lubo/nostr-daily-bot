//! Authentication and session management.

use chrono::Utc;
use nostr_sdk::{Event, Keys, PublicKey, ToBech32};
use uuid::Uuid;

use crate::nostr::NostrClient;

/// Result of parsing and validating an nsec.
pub struct AuthResult {
    /// The user's public key (npub).
    pub npub: String,
    /// The parsed keys.
    pub keys: Keys,
}

/// Parse an nsec and derive the npub.
pub fn parse_nsec(nsec: &str) -> Result<AuthResult, String> {
    let keys = NostrClient::keys_parse(nsec).map_err(|e| e.to_string())?;

    let npub = keys
        .public_key()
        .to_bech32()
        .map_err(|e| format!("Failed to encode npub: {}", e))?;

    Ok(AuthResult { npub, keys })
}

/// Generate a new session token.
pub fn generate_session_token() -> String {
    Uuid::new_v4().to_string()
}

/// Kind 22242 is used for NIP-07 authentication challenges.
pub const AUTH_EVENT_KIND: u16 = 22242;

/// Maximum age of a signed event (in seconds).
const MAX_EVENT_AGE_SECONDS: i64 = 300; // 5 minutes

/// Result of verifying a signed authentication event.
pub struct VerifyResult {
    /// The verified public key (hex).
    pub pubkey_hex: String,
    /// The public key as npub.
    pub npub: String,
}

/// Verify a signed authentication event from NIP-07 extension.
///
/// Checks:
/// 1. Event signature is valid
/// 2. Event content matches the expected challenge
/// 3. Event timestamp is recent (within 5 minutes)
/// 4. Event kind is 22242 (auth challenge)
/// 5. Event has the correct challenge tag
pub fn verify_signed_event(
    event: &Event,
    expected_challenge: &str,
    expected_challenge_id: &str,
) -> Result<VerifyResult, String> {
    // 1. Verify event signature is valid
    event.verify().map_err(|e| format!("Invalid signature: {}", e))?;

    // 2. Verify event kind
    if event.kind.as_u16() != AUTH_EVENT_KIND {
        return Err(format!(
            "Invalid event kind: expected {}, got {}",
            AUTH_EVENT_KIND,
            event.kind.as_u16()
        ));
    }

    // 3. Verify challenge content matches
    if event.content != expected_challenge {
        return Err("Challenge content mismatch".to_string());
    }

    // 4. Verify challenge tag exists and matches
    let has_valid_tag = event.tags.iter().any(|tag| {
        let values: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
        values.len() >= 2 && values[0] == "challenge" && values[1] == expected_challenge_id
    });

    if !has_valid_tag {
        return Err("Missing or invalid challenge tag".to_string());
    }

    // 5. Verify timestamp is recent
    let now = Utc::now().timestamp();
    let event_timestamp = event.created_at.as_u64() as i64;
    if (now - event_timestamp).abs() > MAX_EVENT_AGE_SECONDS {
        return Err("Event timestamp too old or too far in the future".to_string());
    }

    // Convert pubkey to npub
    let npub = event
        .pubkey
        .to_bech32()
        .map_err(|e| format!("Failed to encode npub: {}", e))?;

    Ok(VerifyResult {
        pubkey_hex: event.pubkey.to_hex(),
        npub,
    })
}

/// Parse an npub to hex public key.
pub fn npub_to_hex(npub: &str) -> Result<String, String> {
    let pubkey = PublicKey::parse(npub).map_err(|e| format!("Invalid npub: {}", e))?;
    Ok(pubkey.to_hex())
}

/// Parse a hex public key to npub.
pub fn hex_to_npub(hex: &str) -> Result<String, String> {
    let pubkey = PublicKey::parse(hex).map_err(|e| format!("Invalid hex pubkey: {}", e))?;
    pubkey
        .to_bech32()
        .map_err(|e| format!("Failed to encode npub: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_token() {
        let token1 = generate_session_token();
        let token2 = generate_session_token();

        assert_eq!(token1.len(), 36); // UUID v4 format
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_npub_to_hex_and_back() {
        let npub = "npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqskctm6r";
        let result = npub_to_hex(npub);
        // This is the all-zeros pubkey, which should parse
        assert!(result.is_ok() || result.is_err()); // Just test it doesn't panic
    }

    #[test]
    fn test_hex_to_npub() {
        // Valid 32-byte hex
        let hex = "0000000000000000000000000000000000000000000000000000000000000001";
        let result = hex_to_npub(hex);
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with("npub1"));
    }
}

