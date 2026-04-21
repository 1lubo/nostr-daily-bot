//! Tipping API handlers.

use std::env;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::btcpay::{verify_signature, WebhookEventType, WebhookPayload};
use crate::db::payments::{
    self, create_payment, get_payment_by_invoice_id, mark_payment_expired, mark_payment_invalid,
    mark_payment_paid, CreatePaymentInput, Payment,
};
use crate::state::SharedState;

// ─────────────────────────────────────────────────────────────────────────────
// Request/Response types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTipRequest {
    /// Amount in satoshis
    pub amount_sats: u64,
    /// Optional message from the tipper
    #[serde(default)]
    pub message: Option<String>,
    /// Optional session token to link tip to user
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Serialize)]
pub struct CreateTipResponse {
    /// BTCPay invoice ID
    pub invoice_id: String,
    /// URL to BTCPay checkout page (for redirect mode)
    pub checkout_url: String,
    /// Amount in satoshis
    pub amount_sats: u64,
    /// Payment status
    pub status: String,
}

#[derive(Serialize)]
pub struct TipStatusResponse {
    /// BTCPay invoice ID
    pub invoice_id: String,
    /// Payment status
    pub status: String,
    /// Amount in satoshis
    pub amount_sats: i64,
    /// Payment method used (if paid)
    pub payment_method: Option<String>,
    /// When paid (if paid)
    pub paid_at: Option<String>,
}



#[derive(Serialize)]
pub struct TipConfigResponse {
    /// Whether tipping is enabled
    pub enabled: bool,
    /// BTCPay base URL (for modal script)
    pub btcpay_url: Option<String>,
    /// Default tip amount in sats
    pub default_amount_sats: u64,
}

#[derive(Deserialize)]
pub struct AdminPaymentsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub status: Option<String>,
}

fn default_limit() -> i64 {
    50
}

#[derive(Serialize)]
pub struct AdminPaymentsResponse {
    pub payments: Vec<Payment>,
    pub total: i64,
    pub total_tips_sats: i64,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<MessageResponse>)>;

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<MessageResponse>) {
    (
        status,
        Json(MessageResponse {
            message: message.into(),
        }),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Get tipping configuration (whether enabled, default amount, etc.)
pub async fn get_tip_config(State(state): State<SharedState>) -> Json<TipConfigResponse> {
    let (enabled, btcpay_url, default_amount_sats) = if let Some(ref btcpay) = state.btcpay {
        (true, Some(btcpay.base_url().to_string()), btcpay.default_tip_sats())
    } else {
        (false, None, 5000)
    };

    Json(TipConfigResponse {
        enabled,
        btcpay_url,
        default_amount_sats,
    })
}

/// Create a new tip (BTCPay invoice).
pub async fn create_tip(
    State(state): State<SharedState>,
    Json(req): Json<CreateTipRequest>,
) -> ApiResult<CreateTipResponse> {
    // Check if BTCPay is configured
    let btcpay = state.btcpay.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Tipping is not configured")
    })?;

    // Validate amount
    if req.amount_sats < 100 {
        return Err(api_error(StatusCode::BAD_REQUEST, "Minimum tip is 100 sats"));
    }
    if req.amount_sats > 10_000_000 {
        return Err(api_error(StatusCode::BAD_REQUEST, "Maximum tip is 10,000,000 sats"));
    }

    // Get user npub if token provided
    let user_npub = if let Some(token) = &req.token {
        state.get_any_session_by_token(token).await
    } else {
        None
    };

    // Generate order ID
    let order_id = format!("tip-{}", uuid::Uuid::new_v4());

    // Build redirect URL
    let redirect_url = env::var("PUBLIC_URL")
        .unwrap_or_else(|_| format!("http://localhost:{}", state.port));
    let redirect_url = format!("{}/tip/success", redirect_url);

    // Create BTCPay invoice
    let invoice = btcpay
        .create_invoice(
            req.amount_sats,
            Some(order_id),
            Some("Tip for Nostr Daily Bot".to_string()),
            Some(redirect_url),
        )
        .await
        .map_err(|e| {
            error!("Failed to create BTCPay invoice: {}", e);
            api_error(StatusCode::BAD_GATEWAY, "Failed to create payment invoice")
        })?;

    // Store payment in database
    let input = CreatePaymentInput {
        btcpay_invoice_id: invoice.id.clone(),
        user_npub,
        payment_type: payments::payment_type::TIP.to_string(),
        amount_sats: req.amount_sats as i64,
        message: req.message,
    };

    create_payment(&state.db, input).await.map_err(|e| {
        error!("Failed to store payment: {}", e);
        api_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to store payment")
    })?;

    info!(invoice_id = %invoice.id, amount_sats = req.amount_sats, "Tip invoice created");

    Ok(Json(CreateTipResponse {
        invoice_id: invoice.id,
        checkout_url: invoice.checkout_link,
        amount_sats: req.amount_sats,
        status: "pending".to_string(),
    }))
}

/// Handle BTCPay webhook.
pub async fn tip_webhook(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<MessageResponse> {
    // Check if BTCPay is configured
    let btcpay = state.btcpay.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Tipping is not configured")
    })?;

    // Get signature header
    let signature = headers
        .get("BTCPay-Sig")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Missing BTCPay-Sig header"))?;

    // Verify signature
    verify_signature(&body, signature, btcpay.webhook_secret()).map_err(|_| {
        warn!("Invalid webhook signature");
        api_error(StatusCode::UNAUTHORIZED, "Invalid signature")
    })?;

    // Parse payload
    let payload: WebhookPayload = serde_json::from_slice(&body).map_err(|e| {
        error!("Failed to parse webhook payload: {}", e);
        api_error(StatusCode::BAD_REQUEST, "Invalid payload")
    })?;

    info!(
        invoice_id = %payload.invoice_id,
        event_type = ?payload.event_type,
        "Received BTCPay webhook"
    );

    // Process event
    match payload.event_type {
        WebhookEventType::InvoiceSettled => {
            // TODO: Extract payment method from payload metadata if available
            mark_payment_paid(&state.db, &payload.invoice_id, Some("lightning"))
                .await
                .map_err(|e| {
                    error!("Failed to mark payment as paid: {}", e);
                    api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                })?;
            info!(invoice_id = %payload.invoice_id, "Payment marked as paid");
        }
        WebhookEventType::InvoiceExpired => {
            mark_payment_expired(&state.db, &payload.invoice_id)
                .await
                .map_err(|e| {
                    error!("Failed to mark payment as expired: {}", e);
                    api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                })?;
            info!(invoice_id = %payload.invoice_id, "Payment marked as expired");
        }
        WebhookEventType::InvoiceInvalid => {
            mark_payment_invalid(&state.db, &payload.invoice_id)
                .await
                .map_err(|e| {
                    error!("Failed to mark payment as invalid: {}", e);
                    api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                })?;
            info!(invoice_id = %payload.invoice_id, "Payment marked as invalid");
        }
        _ => {
            // Ignore other event types
            info!(event_type = ?payload.event_type, "Ignoring webhook event type");
        }
    }

    Ok(Json(MessageResponse {
        message: "OK".to_string(),
    }))
}

/// Get tip/payment status.
pub async fn get_tip_status(
    State(state): State<SharedState>,
    Path(invoice_id): Path<String>,
) -> ApiResult<TipStatusResponse> {
    let payment = get_payment_by_invoice_id(&state.db, &invoice_id)
        .await
        .map_err(|e| {
            error!("Failed to get payment: {}", e);
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Payment not found"))?;

    Ok(Json(TipStatusResponse {
        invoice_id: payment.btcpay_invoice_id,
        status: payment.status,
        amount_sats: payment.amount_sats,
        payment_method: payment.payment_method,
        paid_at: payment.paid_at,
    }))
}

/// Admin: List all payments.
pub async fn admin_payments(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<AdminPaymentsQuery>,
) -> ApiResult<AdminPaymentsResponse> {
    // Check admin token
    let admin_token = env::var("ADMIN_TOKEN").ok();
    let provided_token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    match (admin_token, provided_token) {
        (Some(expected), Some(provided)) if expected == provided => {}
        (None, _) => {
            return Err(api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Admin endpoint not configured",
            ));
        }
        _ => {
            return Err(api_error(StatusCode::UNAUTHORIZED, "Invalid admin token"));
        }
    }

    // Get payments
    let payments = payments::list_payments(
        &state.db,
        query.limit.min(100),
        query.offset,
        query.status.as_deref(),
    )
    .await
    .map_err(|e| {
        error!("Failed to list payments: {}", e);
        api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
    })?;

    let total = payments::count_payments(&state.db, query.status.as_deref())
        .await
        .map_err(|e| {
            error!("Failed to count payments: {}", e);
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;

    let total_tips_sats = payments::get_total_tips_sats(&state.db).await.map_err(|e| {
        error!("Failed to get total tips: {}", e);
        api_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
    })?;

    Ok(Json(AdminPaymentsResponse {
        payments,
        total,
        total_tips_sats,
    }))
}
