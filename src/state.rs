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
    /// Active sessions by npub (nsec mode - has NostrClient).
    pub sessions: RwLock<HashMap<String, ActiveSession>>,
    /// Active presign sessions by npub (NIP-07 mode - no NostrClient).
    pub presign_sessions: RwLock<HashMap<String, PresignSession>>,
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

/// Pre-sign session state (NIP-07 authenticated, no server-side key).
pub struct PresignSession {
    /// User's npub (public key).
    pub npub: String,
    /// Session token for authentication.
    pub token: String,
    /// When the session started.
    pub started_at: DateTime<Utc>,
}

impl AppState {
    /// Create new app state.
    pub fn new(db: DbPool, port: u16) -> Self {
        Self {
            db,
            sessions: RwLock::new(HashMap::new()),
            presign_sessions: RwLock::new(HashMap::new()),
            schedulers: RwLock::new(HashMap::new()),
            port,
        }
    }

    /// Check if any session (nsec or presign) exists for the given npub.
    pub async fn has_session(&self, npub: &str) -> bool {
        self.sessions.read().await.contains_key(npub)
            || self.presign_sessions.read().await.contains_key(npub)
    }

    /// Check if a presign session exists for the given npub.
    pub async fn has_presign_session(&self, npub: &str) -> bool {
        self.presign_sessions.read().await.contains_key(npub)
    }

    /// Get npub by token (checks both session types).
    pub async fn get_session_by_token(&self, token: &str) -> Option<String> {
        // Check nsec sessions first
        let sessions = self.sessions.read().await;
        for (npub, session) in sessions.iter() {
            if session.token == token {
                return Some(npub.clone());
            }
        }
        drop(sessions);

        // Check presign sessions
        let presign_sessions = self.presign_sessions.read().await;
        for (npub, session) in presign_sessions.iter() {
            if session.token == token {
                return Some(npub.clone());
            }
        }
        None
    }

    /// Get npub by token for presign sessions only.
    pub async fn get_presign_session_by_token(&self, token: &str) -> Option<String> {
        let sessions = self.presign_sessions.read().await;
        for (npub, session) in sessions.iter() {
            if session.token == token {
                return Some(npub.clone());
            }
        }
        None
    }

    /// Get NostrClient for an nsec session.
    pub async fn get_session(&self, npub: &str) -> Option<Arc<NostrClient>> {
        self.sessions
            .read()
            .await
            .get(npub)
            .map(|s| Arc::clone(&s.nostr_client))
    }

    /// Get the number of active sessions (both types).
    pub async fn active_session_count(&self) -> usize {
        self.sessions.read().await.len() + self.presign_sessions.read().await.len()
    }
}

