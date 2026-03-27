//! API request handlers.

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use nostr_sdk::{Event, PublicKey, ToBech32};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::auth::{generate_session_token, parse_nsec, verify_signed_event};
use crate::db::{challenges, history, quotes, signed_events, users};
use crate::models::UserInput;
use crate::nostr::NostrClient;
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::state::{ActiveSession, PresignSession, SharedState};

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

#[allow(dead_code)]
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

// ─────────────────────────────────────────────────────────────────────────────
// NIP-07 Auth types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChallengeRequest {
    pub npub: String,
}

#[derive(Serialize)]
pub struct ChallengeResponse {
    pub challenge_id: String,
    pub challenge: String,
    pub expires_in: i64,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub challenge_id: String,
    pub signed_event: Event,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    pub npub: String,
    pub token: String,
    pub auth_mode: String,
    pub message: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pre-signing types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PendingEventsRequest {
    pub token: String,
    #[serde(default = "default_days_ahead")]
    pub days_ahead: i32,
}

fn default_days_ahead() -> i32 {
    7
}

#[derive(Serialize)]
pub struct PendingEventsResponse {
    pub events_to_sign: Vec<EventToSign>,
    pub total_pending: i32,
    pub next_unsigned: Option<String>,
}

#[derive(Serialize)]
pub struct EventToSign {
    pub scheduled_for: String,
    pub content: String,
    pub unsigned_event: UnsignedEventJson,
}

#[derive(Serialize)]
pub struct UnsignedEventJson {
    pub kind: i32,
    pub created_at: i64,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    pub pubkey: String,
}

#[derive(Deserialize)]
pub struct SignedEventInput {
    pub scheduled_for: String,
    pub event: Event,
}

#[derive(Deserialize)]
pub struct StoreSignedEventsRequest {
    pub token: String,
    pub signed_events: Vec<SignedEventInput>,
}

#[derive(Serialize)]
pub struct StoreSignedEventsResponse {
    pub stored: i32,
    pub message: String,
}

#[derive(Serialize)]
pub struct EventStatusResponse {
    pub pending: i32,
    pub signed: i32,
    pub posted: i32,
    pub failed: i32,
    pub next_post: Option<String>,
}

#[derive(Serialize)]
pub struct CronPostResponse {
    pub processed: i32,
    pub posted: i32,
    pub failed: i32,
}

#[derive(Serialize)]
pub struct DebugStatusResponse {
    pub current_time: String,
    pub counts: DebugCounts,
    pub pending_events: Vec<DebugEvent>,
    pub recent_posted: Vec<DebugEvent>,
    pub recent_failed: Vec<DebugEvent>,
}

#[derive(Serialize)]
pub struct DebugCounts {
    pub pending: i32,
    pub posted: i32,
    pub failed: i32,
}

#[derive(Serialize)]
pub struct DebugEvent {
    pub id: i64,
    pub user_npub: String,
    pub scheduled_for: String,
    pub status: String,
    pub content_preview: String,
    pub is_due: bool,
    pub error_message: Option<String>,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<MessageResponse>)>;

fn api_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<MessageResponse>) {
    (
        status,
        Json(MessageResponse {
            message: message.into(),
        }),
    )
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
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    // Create Nostr client
    let nostr_client = NostrClient::with_keys(auth.keys).await.map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create client: {}", e),
        )
    })?;
    let nostr_client = Arc::new(nostr_client);

    // Connect to relays
    nostr_client.connect().await.map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to connect: {}", e),
        )
    })?;

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
    let scheduler =
        start_scheduler_for_user(&state, &auth.npub, Arc::clone(&nostr_client), &user.cron)
            .await
            .map_err(|e| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to start scheduler: {}", e),
                )
            })?;

    // Store session and scheduler
    state
        .sessions
        .write()
        .await
        .insert(auth.npub.clone(), session);
    state
        .schedulers
        .write()
        .await
        .insert(auth.npub.clone(), scheduler);

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
// NIP-07 Auth handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Request a challenge for NIP-07 authentication.
pub async fn auth_challenge(
    State(state): State<SharedState>,
    Json(req): Json<ChallengeRequest>,
) -> ApiResult<ChallengeResponse> {
    // Accept either npub or hex pubkey and normalize to npub
    let npub = if req.npub.starts_with("npub1") {
        // Validate it's a proper npub
        PublicKey::parse(&req.npub)
            .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid npub format"))?
            .to_bech32()
            .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid npub format"))?
    } else {
        // Assume it's a hex pubkey and convert to npub
        PublicKey::parse(&req.npub)
            .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid pubkey format"))?
            .to_bech32()
            .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Failed to convert pubkey to npub"))?
    };

    // Create challenge in database
    let challenge = challenges::create_challenge(&state.db, &npub)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create challenge: {}", e),
            )
        })?;

    info!(npub = %npub, challenge_id = %challenge.id, "Auth challenge created");

    Ok(Json(ChallengeResponse {
        challenge_id: challenge.id,
        challenge: challenge.challenge,
        expires_in: 300, // 5 minutes
    }))
}

/// Verify a signed challenge and create a session.
pub async fn auth_verify(
    State(state): State<SharedState>,
    Json(req): Json<VerifyRequest>,
) -> ApiResult<VerifyResponse> {
    // Get and verify the challenge
    let challenge = challenges::verify_challenge(
        &state.db,
        &req.challenge_id,
        &req.signed_event.pubkey.to_bech32().unwrap_or_default(),
    )
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?
    .ok_or_else(|| {
        api_error(
            StatusCode::BAD_REQUEST,
            "Invalid, expired, or already used challenge",
        )
    })?;

    // Verify the signed event
    let verify_result = verify_signed_event(&req.signed_event, &challenge.challenge, &challenge.id)
        .map_err(|e| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("Signature verification failed: {}", e),
            )
        })?;

    // Mark challenge as used
    challenges::mark_challenge_used(&state.db, &challenge.id)
        .await
        .map_err(|e| {
            warn!(error = %e, "Failed to mark challenge as used");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to complete authentication",
            )
        })?;

    // For presign sessions, we allow replacing existing sessions
    // (unlike nsec sessions which hold server-side keys)
    // Remove any existing presign session for this user
    state
        .presign_sessions
        .write()
        .await
        .remove(&verify_result.npub);

    // But block if there's an active nsec session (which has server-side state)
    if state
        .sessions
        .read()
        .await
        .contains_key(&verify_result.npub)
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "An nsec session is already active for this user. Stop it first.",
        ));
    }

    // Create or update user in database with presign auth mode
    let user_input = UserInput::default();
    users::upsert_user(&state.db, &verify_result.npub, &user_input)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    // Update auth_mode to presign
    users::update_auth_mode(&state.db, &verify_result.npub, "presign")
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    // Generate session token
    let token = generate_session_token();

    // Create presign session (no NostrClient needed)
    let session = PresignSession {
        npub: verify_result.npub.clone(),
        token: token.clone(),
        started_at: Utc::now(),
    };

    // Store presign session
    state
        .presign_sessions
        .write()
        .await
        .insert(verify_result.npub.clone(), session);

    info!(npub = %verify_result.npub, "NIP-07 session started");

    Ok(Json(VerifyResponse {
        npub: verify_result.npub,
        token,
        auth_mode: "presign".to_string(),
        message: "Authenticated successfully".to_string(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Pre-signing handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Get pending events that need to be signed.
pub async fn get_pending_events(
    State(state): State<SharedState>,
    Query(req): Query<PendingEventsRequest>,
) -> ApiResult<PendingEventsResponse> {
    // Validate token and get npub (presign sessions only)
    let npub = state
        .get_presign_session_by_token(&req.token)
        .await
        .ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "Invalid session token or not a presign session",
            )
        })?;

    // Get user to check auth mode and get schedule
    let user = users::get_user(&state.db, &npub)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "User not found"))?;

    if user.auth_mode != "presign" {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "User is not in presign mode",
        ));
    }

    // Get user's quotes
    let user_quotes = quotes::get_quotes(&state.db, &npub).await.map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    if user_quotes.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "No quotes configured. Add quotes first.",
        ));
    }

    // Parse cron schedule
    let schedule = cron::Schedule::from_str(&user.cron)
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Invalid cron schedule"))?;

    // Calculate posting times for the next N days
    let now = Utc::now();
    let days_ahead = req.days_ahead.clamp(1, 30); // Limit to 1-30 days
    let end_date = now + chrono::Duration::days(days_ahead as i64);

    let posting_times: Vec<DateTime<Utc>> = schedule
        .upcoming(Utc)
        .take_while(|t| *t < end_date)
        .collect();

    // Get already-signed event times
    let existing_times = signed_events::get_scheduled_times(&state.db, &npub)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    // Get post count for quote tracking (to know which quotes have been used)
    let post_count = history::get_post_count(&state.db, &npub).await.unwrap_or(0) as usize;
    let existing_count = existing_times.len();

    // Convert hex pubkey
    let pubkey = PublicKey::parse(&npub).map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Invalid npub: {}", e),
        )
    })?;
    let pubkey_hex = pubkey.to_hex();

    // Calculate how many quotes have already been used (posted + signed)
    let used_count = post_count + existing_count;

    // Only generate events for remaining unused quotes (no repetition)
    let remaining_quotes = if used_count >= user_quotes.len() {
        0 // All quotes have been used
    } else {
        user_quotes.len() - used_count
    };

    // Generate unsigned events for times that don't have signed events
    // Limited to the number of remaining unused quotes
    let mut events_to_sign = Vec::new();
    let mut quote_offset = 0;

    for time in posting_times.iter() {
        if quote_offset >= remaining_quotes {
            break; // No more unused quotes
        }

        let time_str = time.to_rfc3339();
        if !existing_times.contains(&time_str) {
            let quote_idx = used_count + quote_offset;
            let content = user_quotes[quote_idx].content.clone();

            events_to_sign.push(EventToSign {
                scheduled_for: time_str,
                content: content.clone(),
                unsigned_event: UnsignedEventJson {
                    kind: 1,
                    created_at: time.timestamp(),
                    content,
                    tags: vec![],
                    pubkey: pubkey_hex.clone(),
                },
            });

            quote_offset += 1;
        }
    }

    let total_pending = events_to_sign.len() as i32;
    let next_unsigned = events_to_sign.first().map(|e| e.scheduled_for.clone());

    info!(npub = %npub, pending = total_pending, "Generated pending events for signing");

    Ok(Json(PendingEventsResponse {
        events_to_sign,
        total_pending,
        next_unsigned,
    }))
}

/// Store signed events from client.
pub async fn store_signed_events(
    State(state): State<SharedState>,
    Json(req): Json<StoreSignedEventsRequest>,
) -> ApiResult<StoreSignedEventsResponse> {
    // Validate token and get npub
    let npub = state
        .get_presign_session_by_token(&req.token)
        .await
        .ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "Invalid session token or not a presign session",
            )
        })?;

    if req.signed_events.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "No signed events provided",
        ));
    }

    // Validate and prepare events for storage
    let mut events_to_store = Vec::new();
    for signed_input in &req.signed_events {
        // Verify the event signature
        signed_input.event.verify().map_err(|e| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid event signature: {}", e),
            )
        })?;

        // Verify the event is from the correct user
        let event_npub = signed_input.event.pubkey.to_bech32().map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to encode pubkey: {}", e),
            )
        })?;

        if event_npub != npub {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "Event pubkey does not match session",
            ));
        }

        // Extract content preview (first 100 chars)
        let content_preview = if signed_input.event.content.len() > 100 {
            format!("{}...", &signed_input.event.content[..97])
        } else {
            signed_input.event.content.clone()
        };

        events_to_store.push((
            serde_json::to_string(&signed_input.event).map_err(|e| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to serialize event: {}", e),
                )
            })?,
            signed_input.event.id.to_hex(),
            content_preview,
            signed_input.scheduled_for.clone(),
        ));
    }

    // Store events in database
    let stored = signed_events::store_signed_events(&state.db, &npub, events_to_store)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    info!(npub = %npub, stored = stored, "Stored signed events");

    Ok(Json(StoreSignedEventsResponse {
        stored,
        message: format!("Stored {} signed events", stored),
    }))
}

/// Get event status/counts for a user.
pub async fn get_event_status(
    State(state): State<SharedState>,
    Query(req): Query<PendingEventsRequest>,
) -> ApiResult<EventStatusResponse> {
    // Validate token and get npub
    let npub = state
        .get_presign_session_by_token(&req.token)
        .await
        .ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "Invalid session token or not a presign session",
            )
        })?;

    // Get event counts
    let counts = signed_events::get_event_counts(&state.db, &npub)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    // Get next pending event
    let pending_events = signed_events::get_pending_events(&state.db, &npub, 1)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    let next_post = pending_events.first().map(|e| e.scheduled_for.clone());

    Ok(Json(EventStatusResponse {
        pending: counts.pending,
        signed: counts.signed,
        posted: counts.posted,
        failed: counts.failed,
        next_post,
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
    let user = users::get_user(&state.db, &npub).await.map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

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
    let db_quotes = quotes::get_quotes(&state.db, &npub).await.map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let quote_strings: Vec<String> = db_quotes.into_iter().map(|q| q.content).collect();
    Ok(Json(QuotesResponse {
        quotes: quote_strings,
    }))
}

pub async fn upload_quotes(
    State(state): State<SharedState>,
    Json(req): Json<UploadQuotesRequest>,
) -> ApiResult<MessageResponse> {
    // Validate token and get npub (check both nsec and presign sessions)
    let npub = state
        .get_any_session_by_token(&req.token)
        .await
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?;

    if req.quotes.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Quotes list cannot be empty",
        ));
    }

    // Save to database
    quotes::replace_quotes(&state.db, &npub, &req.quotes)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

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
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "User not found"))?;

    Ok(Json(ScheduleResponse { cron: user.cron }))
}

pub async fn update_schedule(
    State(state): State<SharedState>,
    Json(req): Json<UpdateScheduleRequest>,
) -> ApiResult<MessageResponse> {
    // Validate token and get npub (check both nsec and presign sessions)
    let npub = state
        .get_any_session_by_token(&req.token)
        .await
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?;

    // Validate cron expression
    cron::Schedule::from_str(&req.cron)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid cron expression"))?;

    // Update in database
    users::update_schedule(&state.db, &npub, &req.cron)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    // Restart scheduler if session is active
    if state.has_session(&npub).await {
        // Stop old scheduler
        if let Some(mut scheduler) = state.schedulers.write().await.remove(&npub) {
            let _ = scheduler.stop().await;
        }

        // Start new scheduler with new cron
        if let Some(client) = state.get_session(&npub).await {
            if let Ok(scheduler) = start_scheduler_for_user(&state, &npub, client, &req.cron).await
            {
                state
                    .schedulers
                    .write()
                    .await
                    .insert(npub.clone(), scheduler);
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
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Message cannot be empty",
        ));
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
    let event_id = nostr_client.publish_text_note(message).await.map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to post: {}", e),
        )
    })?;

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
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

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
                        let _ =
                            history::record_post(&db, &npub, message, Some(&event_id_str), 1, true)
                                .await;
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

// ─────────────────────────────────────────────────────────────────────────────
// Cron webhook handler (for external cron services)
// ─────────────────────────────────────────────────────────────────────────────

use crate::scheduler::presign::post_due_events;

/// Webhook endpoint for external cron services to trigger posting of due events.
/// This allows the app to work on shared/serverless infrastructure where the
/// background scheduler may not be running continuously.
///
/// Call this endpoint every 1-5 minutes from an external cron service like:
/// - cron-job.org (free)
/// - EasyCron
/// - GitHub Actions
/// - UptimeRobot (free, can ping URLs)
pub async fn cron_post_due(State(state): State<SharedState>) -> ApiResult<CronPostResponse> {
    let now = Utc::now();
    info!(now = %now, "Cron webhook triggered - checking for due events");

    // First, let's check what pending events exist in the database
    match signed_events::get_event_counts_all(&state.db).await {
        Ok(counts) => {
            info!(
                total_pending = counts.pending,
                total_posted = counts.posted,
                total_failed = counts.failed,
                "Current signed_events status"
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get event counts for logging");
        }
    }

    let result = post_due_events(&state.db).await;

    match result {
        Ok((posted, failed)) => {
            let processed = posted + failed;
            info!(
                processed = processed,
                posted = posted,
                failed = failed,
                now = %now,
                "Cron: finished processing"
            );
            Ok(Json(CronPostResponse {
                processed,
                posted,
                failed,
            }))
        }
        Err(e) => {
            error!(error = %e, now = %now, "Cron: failed to process due events");
            Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to process events: {}", e),
            ))
        }
    }
}

/// Debug endpoint to check database state for signed events.
pub async fn debug_status(State(state): State<SharedState>) -> ApiResult<DebugStatusResponse> {
    let now = Utc::now();

    // Get counts
    let counts = signed_events::get_event_counts_all(&state.db)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get counts: {}", e),
            )
        })?;

    // Get pending events
    let pending_rows = signed_events::get_all_pending(&state.db, 20)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get pending: {}", e),
            )
        })?;

    let pending_events: Vec<DebugEvent> = pending_rows
        .into_iter()
        .map(|e| {
            let scheduled_dt = DateTime::parse_from_rfc3339(&e.scheduled_for).ok();
            let is_due = scheduled_dt.map(|dt| dt <= now).unwrap_or(false);
            DebugEvent {
                id: e.id,
                user_npub: format!("{}...", &e.user_npub[..20.min(e.user_npub.len())]),
                scheduled_for: e.scheduled_for,
                status: e.status,
                content_preview: e.content_preview,
                is_due,
                error_message: e.error_message,
            }
        })
        .collect();

    // Get recent posted events
    let posted_rows = signed_events::get_recent_by_status(&state.db, "posted", 5)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get posted: {}", e),
            )
        })?;

    let recent_posted: Vec<DebugEvent> = posted_rows
        .into_iter()
        .map(|e| DebugEvent {
            id: e.id,
            user_npub: format!("{}...", &e.user_npub[..20.min(e.user_npub.len())]),
            scheduled_for: e.scheduled_for,
            status: e.status,
            content_preview: e.content_preview,
            is_due: false,
            error_message: e.error_message,
        })
        .collect();

    // Get recent failed events
    let failed_rows = signed_events::get_recent_by_status(&state.db, "failed", 5)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get failed: {}", e),
            )
        })?;

    let recent_failed: Vec<DebugEvent> = failed_rows
        .into_iter()
        .map(|e| DebugEvent {
            id: e.id,
            user_npub: format!("{}...", &e.user_npub[..20.min(e.user_npub.len())]),
            scheduled_for: e.scheduled_for,
            status: e.status,
            content_preview: e.content_preview,
            is_due: false,
            error_message: e.error_message,
        })
        .collect();

    Ok(Json(DebugStatusResponse {
        current_time: now.to_rfc3339(),
        counts: DebugCounts {
            pending: counts.pending,
            posted: counts.posted,
            failed: counts.failed,
        },
        pending_events,
        recent_posted,
        recent_failed,
    }))
}
