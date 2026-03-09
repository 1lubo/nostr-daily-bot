//! API request handlers.

use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use nostr_sdk::ToBech32;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::nostr::NostrClient;
use crate::persistence::{save_quotes, save_schedule, PersistedSchedule};
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
pub struct StatusResponse {
    pub active: bool,
    pub session_started_at: Option<String>,
    pub relay_count: usize,
    pub quote_count: usize,
    pub cron: String,
    pub next_post: Option<String>,
    pub server_url: String,
}

#[derive(Serialize)]
pub struct QuotesResponse {
    pub quotes: Vec<String>,
}

#[derive(Deserialize)]
pub struct UploadQuotesRequest {
    pub quotes: Vec<String>,
}

#[derive(Serialize)]
pub struct ScheduleResponse {
    pub cron: String,
}

#[derive(Deserialize)]
pub struct UpdateScheduleRequest {
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
) -> ApiResult<MessageResponse> {
    if state.is_session_active().await {
        return Err(api_error(StatusCode::BAD_REQUEST, "Session already active. Stop it first."));
    }

    let keys = NostrClient::keys_parse(&req.nsec)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("Invalid nsec: {}", e)))?;

    let nostr_client = NostrClient::with_keys(keys.clone())
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create client: {}", e)))?;
    let nostr_client = Arc::new(nostr_client);

    nostr_client.connect().await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to connect: {}", e)))?;

    let session = ActiveSession {
        keys,
        nostr_client: Arc::clone(&nostr_client),
        started_at: Utc::now(),
    };

    let schedule = state.schedule.read().await.clone();
    let scheduler = start_scheduler(&state, Arc::clone(&nostr_client), &schedule.cron)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start scheduler: {}", e)))?;

    *state.scheduler.write().await = Some(scheduler);
    *state.session.write().await = Some(session);

    info!("Session started");
    Ok(Json(MessageResponse { message: "Session started successfully".to_string() }))
}

pub async fn stop_session(State(state): State<SharedState>) -> ApiResult<MessageResponse> {
    if let Some(mut scheduler) = state.scheduler.write().await.take() {
        let _ = scheduler.stop().await;
    }

    if let Some(session) = state.session.write().await.take() {
        session.nostr_client.shutdown().await;
    }

    state.schedule.write().await.next_post = None;

    info!("Session stopped");
    Ok(Json(MessageResponse { message: "Session stopped".to_string() }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Status handler
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_status(State(state): State<SharedState>) -> Json<StatusResponse> {
    let session = state.session.read().await;
    let schedule = state.schedule.read().await;
    let quotes = state.quotes.read().await;

    let (active, session_started_at, relay_count) = match &*session {
        Some(s) => (true, Some(s.started_at.to_rfc3339()), s.nostr_client.connected_relay_count().await),
        None => (false, None, 0),
    };

    Json(StatusResponse {
        active,
        session_started_at,
        relay_count,
        quote_count: quotes.len(),
        cron: schedule.cron.clone(),
        next_post: schedule.next_post.map(|t| t.to_rfc3339()),
        server_url: format!("http://localhost:{}", state.port),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Quotes handlers
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_quotes(State(state): State<SharedState>) -> Json<QuotesResponse> {
    let quotes = state.quotes.read().await.clone();
    Json(QuotesResponse { quotes })
}

pub async fn upload_quotes(
    State(state): State<SharedState>,
    Json(req): Json<UploadQuotesRequest>,
) -> ApiResult<MessageResponse> {
    if req.quotes.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "Quotes list cannot be empty"));
    }

    save_quotes(&req.quotes)
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save: {}", e)))?;

    *state.quotes.write().await = req.quotes.clone();

    info!(count = req.quotes.len(), "Quotes updated");
    Ok(Json(MessageResponse { message: format!("Uploaded {} quotes", req.quotes.len()) }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Schedule handlers
// ─────────────────────────────────────────────────────────────────────────────

pub async fn get_schedule(State(state): State<SharedState>) -> Json<ScheduleResponse> {
    let schedule = state.schedule.read().await;
    Json(ScheduleResponse { cron: schedule.cron.clone() })
}

pub async fn update_schedule(
    State(state): State<SharedState>,
    Json(req): Json<UpdateScheduleRequest>,
) -> ApiResult<MessageResponse> {
    cron::Schedule::from_str(&req.cron)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid cron expression"))?;

    save_schedule(&PersistedSchedule { cron: req.cron.clone() })
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save: {}", e)))?;

    state.schedule.write().await.cron = req.cron.clone();

    // Restart scheduler if session active
    if state.is_session_active().await {
        if let Some(mut scheduler) = state.scheduler.write().await.take() {
            let _ = scheduler.stop().await;
        }

        let session = state.session.read().await;
        if let Some(ref s) = *session {
            if let Ok(scheduler) = start_scheduler(&state, Arc::clone(&s.nostr_client), &req.cron).await {
                *state.scheduler.write().await = Some(scheduler);
            }
        }
    }

    info!(cron = %req.cron, "Schedule updated");
    Ok(Json(MessageResponse { message: "Schedule updated".to_string() }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Post action
// ─────────────────────────────────────────────────────────────────────────────

pub async fn post_now(State(state): State<SharedState>) -> ApiResult<PostResponse> {
    let session = state.session.read().await;
    let session = session.as_ref()
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "No active session"))?;

    let quotes = state.quotes.read().await;
    if quotes.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "No quotes configured"));
    }

    // Get next quote (simple rotation using static counter)
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let idx = COUNTER.fetch_add(1, Ordering::SeqCst) % quotes.len();
    let message = &quotes[idx];

    let event_id = session.nostr_client.publish_text_note(message).await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to post: {}", e)))?;

    let event_id_str = event_id.to_bech32().unwrap_or_else(|_| event_id.to_hex());

    info!(event_id = %event_id_str, "Posted manually");
    Ok(Json(PostResponse {
        message: message.clone(),
        event_id: Some(event_id_str),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: Start scheduler
// ─────────────────────────────────────────────────────────────────────────────

async fn start_scheduler(
    state: &SharedState,
    nostr_client: Arc<NostrClient>,
    cron: &str,
) -> anyhow::Result<Scheduler> {
    let config = SchedulerConfig {
        cron_expression: cron.to_string(),
        timezone: "UTC".to_string(),
    };

    let mut scheduler = Scheduler::new(config).await?;

    let client = Arc::clone(&nostr_client);
    let state_clone = Arc::clone(state);
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    scheduler.register_posting_job(Arc::new(move || {
        let client = Arc::clone(&client);
        let state = Arc::clone(&state_clone);

        Box::pin(async move {
            let quotes = state.quotes.read().await;
            if quotes.is_empty() {
                error!("No quotes to post");
                return;
            }

            let idx = COUNTER.fetch_add(1, Ordering::SeqCst) % quotes.len();
            let message = quotes[idx].clone();
            drop(quotes); // Release lock before async call

            match client.publish_text_note(&message).await {
                Ok(id) => info!(event_id = %id.to_hex(), "Scheduled post successful"),
                Err(e) => error!(error = %e, "Scheduled post failed"),
            }
        })
    })).await?;

    scheduler.start().await?;
    Ok(scheduler)
}
