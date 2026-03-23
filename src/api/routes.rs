//! API route definitions.

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::state::SharedState;

use super::handlers;

/// Create the API router with all endpoints.
pub fn create_router(state: SharedState) -> Router {
    Router::new()
        // NIP-07 Authentication
        .route("/api/auth/challenge", post(handlers::auth_challenge))
        .route("/api/auth/verify", post(handlers::auth_verify))
        // Session management (nsec mode)
        .route("/api/session/start", post(handlers::start_session))
        .route("/api/session/stop", post(handlers::stop_session))
        // Pre-signing endpoints (presign mode)
        .route("/api/events/pending", get(handlers::get_pending_events))
        .route("/api/events/sign", post(handlers::store_signed_events))
        .route("/api/events/status", get(handlers::get_event_status))
        // User-specific endpoints (by npub)
        .route("/api/users/{npub}/status", get(handlers::get_status))
        .route("/api/users/{npub}/quotes", get(handlers::get_quotes))
        .route("/api/users/{npub}/schedule", get(handlers::get_schedule))
        .route("/api/users/{npub}/history", get(handlers::get_history))
        // Authenticated actions (token in body)
        .route("/api/quotes", post(handlers::upload_quotes))
        .route("/api/schedule", put(handlers::update_schedule))
        .route("/api/post", post(handlers::post_now))
        .with_state(state)
}

