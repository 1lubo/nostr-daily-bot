//! BTCPay Server integration module.
//!
//! Provides a client for interacting with BTCPay Server's Greenfield API,
//! including invoice creation and webhook handling.

pub mod client;
pub mod error;
pub mod webhook;

pub use client::{BTCPayClient, InvoiceResponse};
pub use error::{BTCPayError, Result};
pub use webhook::{verify_signature, WebhookEventType, WebhookPayload};
