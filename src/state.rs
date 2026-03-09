//! Application state management.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::db::DbPool;
use crate::nostr::NostrClient;
use crate::scheduler::Scheduler;

/// Shared application state.
pub type SharedState = Arc<AppState>;

/// Main application state, shared across all handlers.
pub struct AppState {
    /// Database connection pool.
    pub db: DbPool,
    /// Active sessions by npub.
    pub sessions: RwLock<HashMap<String, ActiveSession>>,
    /// Active schedulers by npub.
    pub schedulers: RwLock<HashMap<String, Scheduler>>,
    /// Port the server is running on.
    pub port: u16,
}

/// Active session state (when user has entered nsec).
pub struct ActiveSession {
    /// User's npub (public key).
    pub npub: String,
    /// Session token for authentication.
    pub token: String,
    /// Connected Nostr client.
    pub nostr_client: Arc<NostrClient>,
    /// When the session started.
    pub started_at: DateTime<Utc>,
}

impl AppState {
    /// Create new app state.
    pub fn new(db: DbPool, port: u16) -> Self {
        Self {
            db,
            sessions: RwLock::new(HashMap::new()),
            schedulers: RwLock::new(HashMap::new()),
            port,
        }
    }

    /// Check if a session exists for the given npub.
    pub async fn has_session(&self, npub: &str) -> bool {
        self.sessions.read().await.contains_key(npub)
    }

    /// Get a session by token.
    pub async fn get_session_by_token(&self, token: &str) -> Option<String> {
        let sessions = self.sessions.read().await;
        for (npub, session) in sessions.iter() {
            if session.token == token {
                return Some(npub.clone());
            }
        }
        None
    }

    /// Get session for a user.
    pub async fn get_session(&self, npub: &str) -> Option<Arc<NostrClient>> {
        self.sessions
            .read()
            .await
            .get(npub)
            .map(|s| Arc::clone(&s.nostr_client))
    }

    /// Get the number of active sessions.
    pub async fn active_session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

