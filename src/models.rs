//! Data models for the application.

use serde::{Deserialize, Serialize};

/// User record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub npub: String,
    pub display_name: Option<String>,
    pub cron: String,
    pub timezone: String,
    pub auth_mode: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Quote record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub id: i64,
    pub user_npub: String,
    pub content: String,
    pub sort_order: i32,
    pub created_at: String,
}

/// Post history record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostHistory {
    pub id: i64,
    pub user_npub: String,
    pub content: String,
    pub event_id: Option<String>,
    pub relay_count: i32,
    pub is_scheduled: bool,
    pub posted_at: String,
}

/// User creation/update input.
#[derive(Debug, Clone, Deserialize)]
pub struct UserInput {
    pub display_name: Option<String>,
    pub cron: Option<String>,
    pub timezone: Option<String>,
}

impl Default for UserInput {
    fn default() -> Self {
        Self {
            display_name: None,
            cron: Some("0 0 9 * * *".to_string()),
            timezone: Some("UTC".to_string()),
        }
    }
}

/// Auth challenge for NIP-07 login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthChallenge {
    pub id: String,
    pub npub: String,
    pub challenge: String,
    pub created_at: String,
    pub expires_at: String,
    pub used: bool,
}

/// Signed event for scheduled posting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEvent {
    pub id: i64,
    pub user_npub: String,
    pub event_json: String,
    pub event_id: String,
    pub content_preview: String,
    pub scheduled_for: String,
    pub status: String,
    pub posted_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

/// Status of a signed event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedEventStatus {
    Pending,
    Posted,
    Failed,
    Cancelled,
}

impl SignedEventStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Posted => "posted",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Unsigned event to be signed by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedEvent {
    pub kind: i32,
    pub created_at: i64,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    pub pubkey: String,
}

/// Event counts for status display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventCounts {
    pub pending: i32,
    pub signed: i32,
    pub posted: i32,
    pub failed: i32,
}
