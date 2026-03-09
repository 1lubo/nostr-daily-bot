//! Application state management.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use nostr_sdk::Keys;
use tokio::sync::RwLock;

use crate::nostr::NostrClient;
use crate::scheduler::Scheduler;

/// Shared application state.
pub type SharedState = Arc<AppState>;

/// Main application state, shared across all handlers.
pub struct AppState {
    /// Active session (if user has entered nsec).
    pub session: RwLock<Option<ActiveSession>>,
    /// List of quotes to post.
    pub quotes: RwLock<Vec<String>>,
    /// Current schedule configuration.
    pub schedule: RwLock<ScheduleState>,
    /// Active scheduler (if session is running).
    pub scheduler: RwLock<Option<Scheduler>>,
    /// Port the server is running on.
    pub port: u16,
}

/// Active session state (when user has entered nsec).
pub struct ActiveSession {
    /// Nostr keys derived from nsec.
    pub keys: Keys,
    /// Connected Nostr client.
    pub nostr_client: Arc<NostrClient>,
    /// When the session started.
    pub started_at: DateTime<Utc>,
}

/// Schedule configuration state.
#[derive(Clone)]
pub struct ScheduleState {
    /// Cron expression.
    pub cron: String,
    /// Next scheduled post time (if scheduler is running).
    pub next_post: Option<DateTime<Utc>>,
}

impl Default for ScheduleState {
    fn default() -> Self {
        Self {
            cron: "0 0 9 * * *".to_string(), // Daily at 9 AM UTC
            next_post: None,
        }
    }
}

impl AppState {
    /// Create new app state with defaults.
    pub fn new(port: u16) -> Self {
        Self {
            session: RwLock::new(None),
            quotes: RwLock::new(Vec::new()),
            schedule: RwLock::new(ScheduleState::default()),
            scheduler: RwLock::new(None),
            port,
        }
    }

    /// Check if a session is currently active.
    pub async fn is_session_active(&self) -> bool {
        self.session.read().await.is_some()
    }

    /// Get the number of connected relays (if session active).
    pub async fn connected_relay_count(&self) -> usize {
        if let Some(ref session) = *self.session.read().await {
            session.nostr_client.connected_relay_count().await
        } else {
            0
        }
    }
}

