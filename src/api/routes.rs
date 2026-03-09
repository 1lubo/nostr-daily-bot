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
        // Session management
        .route("/api/session/start", post(handlers::start_session))
        .route("/api/session/stop", post(handlers::stop_session))
        // Status
        .route("/api/status", get(handlers::get_status))
        // Quotes
        .route("/api/quotes", get(handlers::get_quotes))
        .route("/api/quotes/upload", post(handlers::upload_quotes))
        // Schedule
        .route("/api/schedule", get(handlers::get_schedule))
        .route("/api/schedule", put(handlers::update_schedule))
        // Actions
        .route("/api/post", post(handlers::post_now))
        .with_state(state)
}

