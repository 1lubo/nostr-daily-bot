//! Authentication and session management.

use nostr_sdk::{Keys, ToBech32};
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
}

