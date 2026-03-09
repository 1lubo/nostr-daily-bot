//! Data models for the application.

use serde::{Deserialize, Serialize};

/// User record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub npub: String,
    pub display_name: Option<String>,
    pub cron: String,
    pub timezone: String,
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

