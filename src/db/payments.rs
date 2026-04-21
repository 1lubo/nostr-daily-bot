//! Payment database operations.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

/// Payment status values.
pub mod status {
    #[allow(dead_code)]
    pub const PENDING: &str = "pending";
    pub const PAID: &str = "paid";
    pub const EXPIRED: &str = "expired";
    pub const INVALID: &str = "invalid";
}

/// Payment type values.
pub mod payment_type {
    pub const TIP: &str = "tip";
    #[allow(dead_code)]
    pub const QUOTA_INCREASE: &str = "quota_increase";
    #[allow(dead_code)]
    pub const STORAGE: &str = "storage";
}

/// Database row for payments table.
#[derive(FromRow)]
struct PaymentRow {
    id: i64,
    btcpay_invoice_id: String,
    user_npub: Option<String>,
    payment_type: String,
    amount_sats: i64,
    message: Option<String>,
    status: String,
    payment_method: Option<String>,
    created_at: DateTime<Utc>,
    paid_at: Option<DateTime<Utc>>,
}

/// Payment record for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub id: i64,
    pub btcpay_invoice_id: String,
    pub user_npub: Option<String>,
    pub payment_type: String,
    pub amount_sats: i64,
    pub message: Option<String>,
    pub status: String,
    pub payment_method: Option<String>,
    pub created_at: String,
    pub paid_at: Option<String>,
}

impl From<PaymentRow> for Payment {
    fn from(row: PaymentRow) -> Self {
        Self {
            id: row.id,
            btcpay_invoice_id: row.btcpay_invoice_id,
            user_npub: row.user_npub,
            payment_type: row.payment_type,
            amount_sats: row.amount_sats,
            message: row.message,
            status: row.status,
            payment_method: row.payment_method,
            created_at: row.created_at.to_rfc3339(),
            paid_at: row.paid_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Input for creating a new payment.
#[derive(Debug)]
pub struct CreatePaymentInput {
    pub btcpay_invoice_id: String,
    pub user_npub: Option<String>,
    pub payment_type: String,
    pub amount_sats: i64,
    pub message: Option<String>,
}

/// Create a new pending payment.
pub async fn create_payment(pool: &PgPool, input: CreatePaymentInput) -> Result<Payment> {
    let row: PaymentRow = sqlx::query_as(
        r#"
        INSERT INTO payments (btcpay_invoice_id, user_npub, payment_type, amount_sats, message)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, btcpay_invoice_id, user_npub, payment_type, amount_sats, message, 
                  status, payment_method, created_at, paid_at
        "#,
    )
    .bind(&input.btcpay_invoice_id)
    .bind(&input.user_npub)
    .bind(&input.payment_type)
    .bind(input.amount_sats)
    .bind(&input.message)
    .fetch_one(pool)
    .await
    .context("Failed to create payment")?;

    Ok(row.into())
}

/// Get a payment by BTCPay invoice ID.
pub async fn get_payment_by_invoice_id(pool: &PgPool, invoice_id: &str) -> Result<Option<Payment>> {
    let row: Option<PaymentRow> = sqlx::query_as(
        r#"
        SELECT id, btcpay_invoice_id, user_npub, payment_type, amount_sats, message,
               status, payment_method, created_at, paid_at
        FROM payments
        WHERE btcpay_invoice_id = $1
        "#,
    )
    .bind(invoice_id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch payment by invoice ID")?;

    Ok(row.map(Payment::from))
}

/// Update payment status to paid.
pub async fn mark_payment_paid(
    pool: &PgPool,
    invoice_id: &str,
    payment_method: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE payments
        SET status = $1, payment_method = $2, paid_at = NOW()
        WHERE btcpay_invoice_id = $3
        "#,
    )
    .bind(status::PAID)
    .bind(payment_method)
    .bind(invoice_id)
    .execute(pool)
    .await
    .context("Failed to mark payment as paid")?;

    Ok(())
}

/// Update payment status to expired.
pub async fn mark_payment_expired(pool: &PgPool, invoice_id: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE payments
        SET status = $1
        WHERE btcpay_invoice_id = $2
        "#,
    )
    .bind(status::EXPIRED)
    .bind(invoice_id)
    .execute(pool)
    .await
    .context("Failed to mark payment as expired")?;

    Ok(())
}

/// Update payment status to invalid.
pub async fn mark_payment_invalid(pool: &PgPool, invoice_id: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE payments
        SET status = $1
        WHERE btcpay_invoice_id = $2
        "#,
    )
    .bind(status::INVALID)
    .bind(invoice_id)
    .execute(pool)
    .await
    .context("Failed to mark payment as invalid")?;

    Ok(())
}

/// List payments with pagination (for admin view).
pub async fn list_payments(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    status_filter: Option<&str>,
) -> Result<Vec<Payment>> {
    let rows: Vec<PaymentRow> = if let Some(status) = status_filter {
        sqlx::query_as(
            r#"
            SELECT id, btcpay_invoice_id, user_npub, payment_type, amount_sats, message,
                   status, payment_method, created_at, paid_at
            FROM payments
            WHERE status = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("Failed to list payments")?
    } else {
        sqlx::query_as(
            r#"
            SELECT id, btcpay_invoice_id, user_npub, payment_type, amount_sats, message,
                   status, payment_method, created_at, paid_at
            FROM payments
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("Failed to list payments")?
    };

    Ok(rows.into_iter().map(Payment::from).collect())
}

/// Get total payment count (for pagination).
pub async fn count_payments(pool: &PgPool, status_filter: Option<&str>) -> Result<i64> {
    let count: i64 = if let Some(status) = status_filter {
        sqlx::query_scalar("SELECT COUNT(*) FROM payments WHERE status = $1")
            .bind(status)
            .fetch_one(pool)
            .await
            .context("Failed to count payments")?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM payments")
            .fetch_one(pool)
            .await
            .context("Failed to count payments")?
    };

    Ok(count)
}

/// Get total tips received (sum of paid tip amounts).
pub async fn get_total_tips_sats(pool: &PgPool) -> Result<i64> {
    let total: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT SUM(amount_sats)
        FROM payments
        WHERE status = $1 AND payment_type = $2
        "#,
    )
    .bind(status::PAID)
    .bind(payment_type::TIP)
    .fetch_one(pool)
    .await
    .context("Failed to get total tips")?;

    Ok(total.unwrap_or(0))
}
