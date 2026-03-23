//! Signed events database operations.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use crate::models::{EventCounts, SignedEvent, SignedEventStatus};

#[derive(FromRow)]
struct SignedEventRow {
    id: i64,
    user_npub: String,
    event_json: String,
    event_id: String,
    content_preview: String,
    scheduled_for: DateTime<Utc>,
    status: String,
    posted_at: Option<DateTime<Utc>>,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
}

fn row_to_signed_event(r: SignedEventRow) -> SignedEvent {
    SignedEvent {
        id: r.id,
        user_npub: r.user_npub,
        event_json: r.event_json,
        event_id: r.event_id,
        content_preview: r.content_preview,
        scheduled_for: r.scheduled_for.to_rfc3339(),
        status: r.status,
        posted_at: r.posted_at.map(|dt| dt.to_rfc3339()),
        error_message: r.error_message,
        created_at: r.created_at.to_rfc3339(),
    }
}

/// Store a batch of signed events.
pub async fn store_signed_events(
    pool: &PgPool,
    user_npub: &str,
    events: Vec<(String, String, String, String)>, // (event_json, event_id, content_preview, scheduled_for)
) -> Result<i32> {
    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    let mut count = 0;

    for (event_json, event_id, content_preview, scheduled_for) in events {
        // Skip if event_id already exists
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM signed_events WHERE event_id = $1")
                .bind(&event_id)
                .fetch_one(&mut *tx)
                .await
                .context("Failed to check event existence")?;

        if exists > 0 {
            continue;
        }

        // Parse scheduled_for as timestamp
        let scheduled_dt = DateTime::parse_from_rfc3339(&scheduled_for)
            .map(|dt| dt.with_timezone(&Utc))
            .context("Invalid scheduled_for datetime")?;

        sqlx::query(
            "INSERT INTO signed_events (user_npub, event_json, event_id, content_preview, scheduled_for, status) VALUES ($1, $2, $3, $4, $5, $6)"
        )
        .bind(user_npub)
        .bind(&event_json)
        .bind(&event_id)
        .bind(&content_preview)
        .bind(scheduled_dt)
        .bind(SignedEventStatus::Pending.as_str())
        .execute(&mut *tx)
        .await
        .context("Failed to insert signed event")?;

        count += 1;
    }

    tx.commit().await.context("Failed to commit transaction")?;
    Ok(count)
}

/// Get pending signed events for a user.
pub async fn get_pending_events(
    pool: &PgPool,
    user_npub: &str,
    limit: i32,
) -> Result<Vec<SignedEvent>> {
    let rows: Vec<SignedEventRow> = sqlx::query_as(
        "SELECT id, user_npub, event_json, event_id, content_preview, scheduled_for, status, posted_at, error_message, created_at
         FROM signed_events
         WHERE user_npub = $1 AND status = 'pending'
         ORDER BY scheduled_for ASC
         LIMIT $2"
    )
    .bind(user_npub)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to fetch pending events")?;

    Ok(rows.into_iter().map(row_to_signed_event).collect())
}

/// Get the next due signed event (scheduled_for <= now and status = pending).
#[allow(dead_code)]
pub async fn get_next_due(pool: &PgPool, user_npub: &str) -> Result<Option<SignedEvent>> {
    let now = Utc::now();

    let row: Option<SignedEventRow> = sqlx::query_as(
        "SELECT id, user_npub, event_json, event_id, content_preview, scheduled_for, status, posted_at, error_message, created_at
         FROM signed_events
         WHERE user_npub = $1 AND status = 'pending' AND scheduled_for <= $2
         ORDER BY scheduled_for ASC
         LIMIT 1"
    )
    .bind(user_npub)
    .bind(now)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch next due event")?;

    Ok(row.map(row_to_signed_event))
}

/// Get all due signed events across all users.
pub async fn get_all_due(pool: &PgPool) -> Result<Vec<SignedEvent>> {
    let now = Utc::now();

    let rows: Vec<SignedEventRow> = sqlx::query_as(
        "SELECT id, user_npub, event_json, event_id, content_preview, scheduled_for, status, posted_at, error_message, created_at
         FROM signed_events
         WHERE status = 'pending' AND scheduled_for <= $1
         ORDER BY scheduled_for ASC"
    )
    .bind(now)
    .fetch_all(pool)
    .await
    .context("Failed to fetch all due events")?;

    Ok(rows.into_iter().map(row_to_signed_event).collect())
}

/// Mark a signed event as posted.
pub async fn mark_posted(pool: &PgPool, id: i64) -> Result<()> {
    let now = Utc::now();

    sqlx::query("UPDATE signed_events SET status = 'posted', posted_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to mark event as posted")?;

    Ok(())
}

/// Mark a signed event as failed.
pub async fn mark_failed(pool: &PgPool, id: i64, error: &str) -> Result<()> {
    sqlx::query("UPDATE signed_events SET status = 'failed', error_message = $1 WHERE id = $2")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to mark event as failed")?;

    Ok(())
}

/// Get event counts for a user.
pub async fn get_event_counts(pool: &PgPool, user_npub: &str) -> Result<EventCounts> {
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signed_events WHERE user_npub = $1 AND status = 'pending'",
    )
    .bind(user_npub)
    .fetch_one(pool)
    .await?;

    let posted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signed_events WHERE user_npub = $1 AND status = 'posted'",
    )
    .bind(user_npub)
    .fetch_one(pool)
    .await?;

    let failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signed_events WHERE user_npub = $1 AND status = 'failed'",
    )
    .bind(user_npub)
    .fetch_one(pool)
    .await?;

    Ok(EventCounts {
        pending: pending as i32,
        signed: pending as i32, // signed but not yet posted = pending
        posted: posted as i32,
        failed: failed as i32,
    })
}

/// Get scheduled times that already have signed events.
pub async fn get_scheduled_times(pool: &PgPool, user_npub: &str) -> Result<Vec<String>> {
    let rows: Vec<(DateTime<Utc>,)> = sqlx::query_as(
        "SELECT scheduled_for FROM signed_events WHERE user_npub = $1 AND status IN ('pending', 'posted')"
    )
    .bind(user_npub)
    .fetch_all(pool)
    .await
    .context("Failed to fetch scheduled times")?;

    Ok(rows.into_iter().map(|(dt,)| dt.to_rfc3339()).collect())
}

/// Cancel all pending signed events for a user.
#[allow(dead_code)]
pub async fn cancel_pending_events(pool: &PgPool, user_npub: &str) -> Result<i32> {
    let result = sqlx::query(
        "UPDATE signed_events SET status = 'cancelled' WHERE user_npub = $1 AND status = 'pending'",
    )
    .bind(user_npub)
    .execute(pool)
    .await
    .context("Failed to cancel pending events")?;

    Ok(result.rows_affected() as i32)
}
