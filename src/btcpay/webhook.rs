//! BTCPay webhook handling and signature verification.

use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

use super::error::{BTCPayError, Result};

type HmacSha256 = Hmac<Sha256>;

/// Verify a BTCPay webhook signature.
///
/// BTCPay sends a `BTCPay-Sig` header containing the HMAC-SHA256 signature
/// of the request body, using the webhook secret as the key.
///
/// The header format is: `sha256=<hex-encoded-signature>`
pub fn verify_signature(payload: &[u8], signature_header: &str, secret: &str) -> Result<()> {
    // Parse the signature header (format: "sha256=<hex>")
    let signature_hex = signature_header
        .strip_prefix("sha256=")
        .ok_or(BTCPayError::WebhookVerificationFailed)?;

    // Decode the hex signature
    let expected_signature =
        hex::decode(signature_hex).map_err(|_| BTCPayError::WebhookVerificationFailed)?;

    // Compute HMAC-SHA256 of the payload
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| BTCPayError::WebhookVerificationFailed)?;
    mac.update(payload);

    // Verify the signature (constant-time comparison)
    mac.verify_slice(&expected_signature)
        .map_err(|_| BTCPayError::WebhookVerificationFailed)
}

/// BTCPay webhook event types we care about.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum WebhookEventType {
    /// Invoice has been fully paid and confirmed
    InvoiceSettled,
    /// Invoice has expired without payment
    InvoiceExpired,
    /// Invoice payment is invalid (e.g., underpaid)
    InvoiceInvalid,
    /// Invoice is processing (payment seen, awaiting confirmation)
    InvoiceProcessing,
    /// Invoice created
    InvoiceCreated,
    /// Unknown event type
    #[serde(other)]
    Unknown,
}

/// Webhook payload from BTCPay Server.
/// Fields are parsed from JSON - some may not be directly used but are needed for deserialization.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct WebhookPayload {
    /// The invoice ID
    pub invoice_id: String,
    /// Event type
    #[serde(rename = "type")]
    pub event_type: WebhookEventType,
    /// Store ID
    pub store_id: String,
    /// Additional metadata (if any)
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_signature_valid() {
        let secret = "test-secret";
        let payload = b"test payload";

        // Compute expected signature
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        let signature = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={}", signature);

        assert!(verify_signature(payload, &header, secret).is_ok());
    }

    #[test]
    fn test_verify_signature_invalid() {
        let secret = "test-secret";
        let payload = b"test payload";
        let header = "sha256=invalid";

        assert!(verify_signature(payload, header, secret).is_err());
    }

    #[test]
    fn test_verify_signature_wrong_secret() {
        let secret = "test-secret";
        let wrong_secret = "wrong-secret";
        let payload = b"test payload";

        // Compute signature with wrong secret
        let mut mac = HmacSha256::new_from_slice(wrong_secret.as_bytes()).unwrap();
        mac.update(payload);
        let signature = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={}", signature);

        assert!(verify_signature(payload, &header, secret).is_err());
    }

    #[test]
    fn test_deserialize_webhook_payload() {
        let json = r#"{
            "invoiceId": "test-invoice-123",
            "type": "InvoiceSettled",
            "storeId": "test-store",
            "metadata": {}
        }"#;

        let payload: WebhookPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.invoice_id, "test-invoice-123");
        assert_eq!(payload.event_type, WebhookEventType::InvoiceSettled);
        assert_eq!(payload.store_id, "test-store");
    }

    #[test]
    fn test_deserialize_unknown_event_type() {
        let json = r#"{
            "invoiceId": "test-invoice-123",
            "type": "SomeNewEventType",
            "storeId": "test-store",
            "metadata": {}
        }"#;

        let payload: WebhookPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.event_type, WebhookEventType::Unknown);
    }
}
