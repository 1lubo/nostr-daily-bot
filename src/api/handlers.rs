//! API request handlers.

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use nostr_sdk::ToBech32;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::auth::{generate_session_token, parse_nsec};
use crate::db::{history, quotes, users};
use crate::models::UserInput;
use crate::nostr::NostrClient;
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::state::{ActiveSession, SharedState};

// ─────────────────────────────────────────────────────────────────────────────
// Request/Response types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StartSessionRequest {
    pub nsec: String,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub npub: String,
    pub token: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AuthenticatedRequest<T> {
    pub token: String,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub npub: Option<String>,
    pub active: bool,
    pub session_started_at: Option<String>,
    pub relay_count: usize,
    pub quote_count: i32,
    pub post_count: i32,
    pub cron: String,
    pub server_url: String,
}

#[derive(Serialize)]
pub struct QuotesResponse {
    pub quotes: Vec<String>,
}

#[derive(Deserialize)]
pub struct UploadQuotesRequest {
    pub token: String,
    pub quotes: Vec<String>,
}

#[derive(Serialize)]
pub struct ScheduleResponse {
    pub cron: String,
}

#[derive(Deserialize)]
pub struct UpdateScheduleRequest {
    pub token: String,
    pub cron: String,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Serialize)]
pub struct PostResponse {
    pub message: String,
    pub event_id: Option<String>,
}

#[derive(Deserialize)]
pub struct PostNowRequest {
    pub token: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct StopSessionRequest {
    pub token: String,
}

#[derive(Serialize)]
pub struct HistoryResponse {
    pub posts: Vec<HistoryItem>,
}

#[derive(Serialize)]
pub struct HistoryItem {
    pub content: String,
    pub event_id: Option<String>,
    pub posted_at: String,
    pub is_scheduled: bool,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<MessageResponse>)>;

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<MessageResponse>) {
    (status, Json(MessageResponse { message: message.into() }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Session handlers
// ─────────────────────────────────────────────────────────────────────────────

pub async fn start_session(
    State(state): State<SharedState>,
    Json(req): Json<StartSessionRequest>,
) -> ApiResult<SessionResponse> {
    // Parse nsec and derive npub
    let auth = parse_nsec(&req.nsec)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("Invalid nsec: {}", e)))?;

    // Check if already has active session
    if state.has_session(&auth.npub).await {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Session already active for this user",
        ));
    }

    // Create or get user in database
    let user = users::upsert_user(&state.db, &auth.npub, &UserInput::default())
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    // Create Nostr client
    let nostr_client = NostrClient::with_keys(auth.keys)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create client: {}", e)))?;
    let nostr_client = Arc::new(nostr_client);

    // Connect to relays
    nostr_client
        .connect()
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to connect: {}", e)))?;

    // Generate session token
    let token = generate_session_token();

    // Create session
    let session = ActiveSession {
        npub: auth.npub.clone(),
        token: token.clone(),
        nostr_client: Arc::clone(&nostr_client),
        started_at: Utc::now(),
    };

    // Start scheduler for this user
    let scheduler = start_scheduler_for_user(&state, &auth.npub, Arc::clone(&nostr_client), &user.cron)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start scheduler: {}", e)))?;

    // Store session and scheduler
    state.sessions.write().await.insert(auth.npub.clone(), session);
    state.schedulers.write().await.insert(auth.npub.clone(), scheduler);

    info!(npub = %auth.npub, "Session started");
    Ok(Json(SessionResponse {
        npub: auth.npub,
        token,
        message: "Session started successfully".to_string(),
    }))
}

pub async fn stop_session(
    State(state): State<SharedState>,
    Json(req): Json<StopSessionRequest>,
) -> ApiResult<MessageResponse> {
    // Find user by token
    let npub = state
        .get_session_by_token(&req.token)
        .await
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?;

    // Stop scheduler
    if let Some(mut scheduler) = state.schedulers.write().await.remove(&npub) {
        let _ = scheduler.stop().await;
    }

    // Remove session
    if let Some(session) = state.sessions.write().await.remove(&npub) {
        session.nostr_client.shutdown().await;
    }

    info!(npub = %npub, "Session stopped");
    Ok(Json(MessageResponse {
        message: "Session stopped".to_string(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Status handler
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_status(
    State(state): State<SharedState>,
    Path(npub): Path<String>,
) -> ApiResult<StatusResponse> {
    // Get user from database
    let user = users::get_user(&state.db, &npub)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    let user = match user {
        Some(u) => u,
        None => {
            return Ok(Json(StatusResponse {
                npub: Some(npub),
                active: false,
                session_started_at: None,
                relay_count: 0,
                quote_count: 0,
                post_count: 0,
                cron: "0 0 9 * * *".to_string(),
                server_url: format!("http://localhost:{}", state.port),
            }));
        }
    };

    // Check if session is active
    let sessions = state.sessions.read().await;
    let session = sessions.get(&npub);

    let (active, session_started_at, relay_count) = match session {
        Some(s) => (
            true,
            Some(s.started_at.to_rfc3339()),
            s.nostr_client.connected_relay_count().await,
        ),
        None => (false, None, 0),
    };

    // Get counts from database
    let quote_count = quotes::get_quote_count(&state.db, &npub).await.unwrap_or(0);
    let post_count = history::get_post_count(&state.db, &npub).await.unwrap_or(0);

    Ok(Json(StatusResponse {
        npub: Some(npub),
        active,
        session_started_at,
        relay_count,
        quote_count,
        post_count,
        cron: user.cron,
        server_url: format!("http://localhost:{}", state.port),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Quotes handlers
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_quotes(
    State(state): State<SharedState>,
    Path(npub): Path<String>,
) -> ApiResult<QuotesResponse> {
    let db_quotes = quotes::get_quotes(&state.db, &npub)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    let quote_strings: Vec<String> = db_quotes.into_iter().map(|q| q.content).collect();
    Ok(Json(QuotesResponse { quotes: quote_strings }))
}

pub async fn upload_quotes(
    State(state): State<SharedState>,
    Json(req): Json<UploadQuotesRequest>,
) -> ApiResult<MessageResponse> {
    // Validate token and get npub
    let npub = state
        .get_session_by_token(&req.token)
        .await
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?;

    if req.quotes.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "Quotes list cannot be empty"));
    }

    // Save to database
    quotes::replace_quotes(&state.db, &npub, &req.quotes)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    info!(npub = %npub, count = req.quotes.len(), "Quotes updated");
    Ok(Json(MessageResponse {
        message: format!("Uploaded {} quotes", req.quotes.len()),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Schedule handlers
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_schedule(
    State(state): State<SharedState>,
    Path(npub): Path<String>,
) -> ApiResult<ScheduleResponse> {
    let user = users::get_user(&state.db, &npub)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "User not found"))?;

    Ok(Json(ScheduleResponse { cron: user.cron }))
}

pub async fn update_schedule(
    State(state): State<SharedState>,
    Json(req): Json<UpdateScheduleRequest>,
) -> ApiResult<MessageResponse> {
    // Validate token and get npub
    let npub = state
        .get_session_by_token(&req.token)
        .await
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?;

    // Validate cron expression
    cron::Schedule::from_str(&req.cron)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid cron expression"))?;

    // Update in database
    users::update_schedule(&state.db, &npub, &req.cron)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    // Restart scheduler if session is active
    if state.has_session(&npub).await {
        // Stop old scheduler
        if let Some(mut scheduler) = state.schedulers.write().await.remove(&npub) {
            let _ = scheduler.stop().await;
        }

        // Start new scheduler with new cron
        if let Some(client) = state.get_session(&npub).await {
            if let Ok(scheduler) = start_scheduler_for_user(&state, &npub, client, &req.cron).await {
                state.schedulers.write().await.insert(npub.clone(), scheduler);
            }
        }
    }

    info!(npub = %npub, cron = %req.cron, "Schedule updated");
    Ok(Json(MessageResponse {
        message: "Schedule updated".to_string(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Post action
// ─────────────────────────────────────────────────────────────────────────────

pub async fn post_now(
    State(state): State<SharedState>,
    Json(req): Json<PostNowRequest>,
) -> ApiResult<PostResponse> {
    let message = req.message.trim();
    if message.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "Message cannot be empty"));
    }

    // Validate token and get npub
    let npub = state
        .get_session_by_token(&req.token)
        .await
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?;

    // Get the session's nostr client
    let nostr_client = state
        .get_session(&npub)
        .await
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Session not found"))?;

    // Publish the note
    let event_id = nostr_client
        .publish_text_note(message)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to post: {}", e)))?;

    let event_id_str = event_id.to_bech32().unwrap_or_else(|_| event_id.to_hex());

    // Record in history
    let _ = history::record_post(&state.db, &npub, message, Some(&event_id_str), 1, false).await;

    info!(npub = %npub, event_id = %event_id_str, "Posted manually");
    Ok(Json(PostResponse {
        message: message.to_string(),
        event_id: Some(event_id_str),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// History handler
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_history(
    State(state): State<SharedState>,
    Path(npub): Path<String>,
) -> ApiResult<HistoryResponse> {
    let posts = history::get_history(&state.db, &npub, 50)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    let items: Vec<HistoryItem> = posts
        .into_iter()
        .map(|p| HistoryItem {
            content: p.content,
            event_id: p.event_id,
            posted_at: p.posted_at,
            is_scheduled: p.is_scheduled,
        })
        .collect();

    Ok(Json(HistoryResponse { posts: items }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: Start scheduler for a user
// ─────────────────────────────────────────────────────────────────────────────

async fn start_scheduler_for_user(
    state: &SharedState,
    npub: &str,
    nostr_client: Arc<NostrClient>,
    cron: &str,
) -> anyhow::Result<Scheduler> {
    let config = SchedulerConfig {
        cron_expression: cron.to_string(),
        timezone: "UTC".to_string(),
    };

    let mut scheduler = Scheduler::new(config).await?;

    let client = Arc::clone(&nostr_client);
    let db = state.db.clone();
    let user_npub = npub.to_string();

    scheduler
        .register_posting_job(Arc::new(move || {
            let client = Arc::clone(&client);
            let db = db.clone();
            let npub = user_npub.clone();

            Box::pin(async move {
                // Get quotes from database
                let user_quotes = match quotes::get_quotes(&db, &npub).await {
                    Ok(q) => q,
                    Err(e) => {
                        error!(npub = %npub, error = %e, "Failed to fetch quotes");
                        return;
                    }
                };

                if user_quotes.is_empty() {
                    error!(npub = %npub, "No quotes to post");
                    return;
                }

                // Simple rotation using post count
                let post_count = history::get_post_count(&db, &npub).await.unwrap_or(0) as usize;
                let idx = post_count % user_quotes.len();
                let message = &user_quotes[idx].content;

                match client.publish_text_note(message).await {
                    Ok(id) => {
                        let event_id_str = id.to_bech32().unwrap_or_else(|_| id.to_hex());
                        info!(npub = %npub, event_id = %event_id_str, "Scheduled post successful");
                        let _ = history::record_post(&db, &npub, message, Some(&event_id_str), 1, true).await;
                    }
                    Err(e) => {
                        error!(npub = %npub, error = %e, "Scheduled post failed");
                    }
                }
            })
        }))
        .await?;

    scheduler.start().await?;
    Ok(scheduler)
}
