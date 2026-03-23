//! Post history database operations.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use crate::models::PostHistory;

#[derive(FromRow)]
struct PostHistoryRow {
    id: i64,
    user_npub: String,
    content: String,
    event_id: Option<String>,
    relay_count: i32,
    is_scheduled: bool,
    posted_at: DateTime<Utc>,
}

/// Record a post in history.
pub async fn record_post(
    pool: &PgPool,
    user_npub: &str,
    content: &str,
    event_id: Option<&str>,
    relay_count: i32,
    is_scheduled: bool,
) -> Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO post_history (user_npub, content, event_id, relay_count, is_scheduled) VALUES ($1, $2, $3, $4, $5) RETURNING id"
    )
    .bind(user_npub)
    .bind(content)
    .bind(event_id)
    .bind(relay_count)
    .bind(is_scheduled)
    .fetch_one(pool)
    .await
    .context("Failed to record post")?;

    Ok(id)
}

/// Get recent post history for a user.
pub async fn get_history(
    pool: &PgPool,
    user_npub: &str,
    limit: i32,
) -> Result<Vec<PostHistory>> {
    let rows: Vec<PostHistoryRow> = sqlx::query_as(
        "SELECT id, user_npub, content, event_id, relay_count, is_scheduled, posted_at FROM post_history WHERE user_npub = $1 ORDER BY posted_at DESC LIMIT $2"
    )
    .bind(user_npub)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to fetch post history")?;

    Ok(rows.into_iter().map(|r| PostHistory {
        id: r.id,
        user_npub: r.user_npub,
        content: r.content,
        event_id: r.event_id,
        relay_count: r.relay_count,
        is_scheduled: r.is_scheduled,
        posted_at: r.posted_at.to_rfc3339(),
    }).collect())
}

/// Get total post count for a user.
pub async fn get_post_count(pool: &PgPool, user_npub: &str) -> Result<i32> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM post_history WHERE user_npub = $1")
        .bind(user_npub)
        .fetch_one(pool)
        .await
        .context("Failed to count posts")?;

    Ok(count as i32)
}

/// Delete old history entries (keep last N).
pub async fn cleanup_history(pool: &PgPool, user_npub: &str, keep_count: i32) -> Result<i32> {
    let keep_count_i64 = keep_count as i64;
    let result = sqlx::query(
        "DELETE FROM post_history WHERE user_npub = $1 AND id NOT IN (SELECT id FROM post_history WHERE user_npub = $1 ORDER BY posted_at DESC LIMIT $2)"
    )
    .bind(user_npub)
    .bind(keep_count_i64)
    .execute(pool)
    .await
    .context("Failed to cleanup history")?;

    Ok(result.rows_affected() as i32)
}

