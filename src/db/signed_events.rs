//! Signed events database operations.

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::models::{EventCounts, SignedEvent, SignedEventStatus};

/// Store a batch of signed events.
pub async fn store_signed_events(
    pool: &SqlitePool,
    user_npub: &str,
    events: Vec<(String, String, String, String)>, // (event_json, event_id, content_preview, scheduled_for)
) -> Result<i32> {
    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    let mut count = 0;

    for (event_json, event_id, content_preview, scheduled_for) in events {
        // Skip if event_id already exists
        let exists: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM signed_events WHERE event_id = ?"
        )
        .bind(&event_id)
        .fetch_one(&mut *tx)
        .await
        .context("Failed to check event existence")?;

        if exists > 0 {
            continue;
        }

        sqlx::query(
            "INSERT INTO signed_events (user_npub, event_json, event_id, content_preview, scheduled_for, status) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(user_npub)
        .bind(&event_json)
        .bind(&event_id)
        .bind(&content_preview)
        .bind(&scheduled_for)
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
pub async fn get_pending_events(pool: &SqlitePool, user_npub: &str, limit: i32) -> Result<Vec<SignedEvent>> {
    let rows = sqlx::query(
        "SELECT id, user_npub, event_json, event_id, content_preview, scheduled_for, status, posted_at, error_message, created_at
         FROM signed_events
         WHERE user_npub = ? AND status = 'pending'
         ORDER BY scheduled_for ASC
         LIMIT ?"
    )
    .bind(user_npub)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to fetch pending events")?;

    Ok(rows.into_iter().map(row_to_signed_event).collect())
}

/// Get the next due signed event (scheduled_for <= now and status = pending).
pub async fn get_next_due(pool: &SqlitePool, user_npub: &str) -> Result<Option<SignedEvent>> {
    let now = Utc::now().to_rfc3339();

    let row = sqlx::query(
        "SELECT id, user_npub, event_json, event_id, content_preview, scheduled_for, status, posted_at, error_message, created_at
         FROM signed_events
         WHERE user_npub = ? AND status = 'pending' AND scheduled_for <= ?
         ORDER BY scheduled_for ASC
         LIMIT 1"
    )
    .bind(user_npub)
    .bind(&now)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch next due event")?;

    Ok(row.map(row_to_signed_event))
}

/// Get all due signed events across all users.
pub async fn get_all_due(pool: &SqlitePool) -> Result<Vec<SignedEvent>> {
    let now = Utc::now().to_rfc3339();

    let rows = sqlx::query(
        "SELECT id, user_npub, event_json, event_id, content_preview, scheduled_for, status, posted_at, error_message, created_at
         FROM signed_events
         WHERE status = 'pending' AND scheduled_for <= ?
         ORDER BY scheduled_for ASC"
    )
    .bind(&now)
    .fetch_all(pool)
    .await
    .context("Failed to fetch all due events")?;

    Ok(rows.into_iter().map(row_to_signed_event).collect())
}

/// Mark a signed event as posted.
pub async fn mark_posted(pool: &SqlitePool, id: i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    sqlx::query("UPDATE signed_events SET status = 'posted', posted_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to mark event as posted")?;

    Ok(())
}

/// Mark a signed event as failed.
pub async fn mark_failed(pool: &SqlitePool, id: i64, error: &str) -> Result<()> {
    sqlx::query("UPDATE signed_events SET status = 'failed', error_message = ? WHERE id = ?")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to mark event as failed")?;

    Ok(())
}

/// Get event counts for a user.
pub async fn get_event_counts(pool: &SqlitePool, user_npub: &str) -> Result<EventCounts> {
    let pending: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signed_events WHERE user_npub = ? AND status = 'pending'"
    )
    .bind(user_npub)
    .fetch_one(pool)
    .await?;

    let posted: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signed_events WHERE user_npub = ? AND status = 'posted'"
    )
    .bind(user_npub)
    .fetch_one(pool)
    .await?;

    let failed: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signed_events WHERE user_npub = ? AND status = 'failed'"
    )
    .bind(user_npub)
    .fetch_one(pool)
    .await?;

    Ok(EventCounts {
        pending,
        signed: pending, // signed but not yet posted = pending
        posted,
        failed,
    })
}

/// Get scheduled times that already have signed events.
pub async fn get_scheduled_times(pool: &SqlitePool, user_npub: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT scheduled_for FROM signed_events WHERE user_npub = ? AND status IN ('pending', 'posted')"
    )
    .bind(user_npub)
    .fetch_all(pool)
    .await
    .context("Failed to fetch scheduled times")?;

    Ok(rows.into_iter().map(|(s,)| s).collect())
}

/// Cancel all pending signed events for a user.
pub async fn cancel_pending_events(pool: &SqlitePool, user_npub: &str) -> Result<i32> {
    let result = sqlx::query(
        "UPDATE signed_events SET status = 'cancelled' WHERE user_npub = ? AND status = 'pending'"
    )
    .bind(user_npub)
    .execute(pool)
    .await
    .context("Failed to cancel pending events")?;

    Ok(result.rows_affected() as i32)
}

/// Helper to convert a row to SignedEvent.
fn row_to_signed_event(r: sqlx::sqlite::SqliteRow) -> SignedEvent {
    SignedEvent {
        id: r.get("id"),
        user_npub: r.get("user_npub"),
        event_json: r.get("event_json"),
        event_id: r.get("event_id"),
        content_preview: r.get("content_preview"),
        scheduled_for: r.get("scheduled_for"),
        status: r.get("status"),
        posted_at: r.get("posted_at"),
        error_message: r.get("error_message"),
        created_at: r.get("created_at"),
    }
}

