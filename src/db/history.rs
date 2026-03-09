//! Post history database operations.

use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};

use crate::models::PostHistory;

/// Record a post in history.
pub async fn record_post(
    pool: &SqlitePool,
    user_npub: &str,
    content: &str,
    event_id: Option<&str>,
    relay_count: i32,
    is_scheduled: bool,
) -> Result<i64> {
    let is_scheduled_int: i32 = if is_scheduled { 1 } else { 0 };

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO post_history (user_npub, content, event_id, relay_count, is_scheduled) VALUES (?, ?, ?, ?, ?) RETURNING id"
    )
    .bind(user_npub)
    .bind(content)
    .bind(event_id)
    .bind(relay_count)
    .bind(is_scheduled_int)
    .fetch_one(pool)
    .await
    .context("Failed to record post")?;

    Ok(id)
}

/// Get recent post history for a user.
pub async fn get_history(
    pool: &SqlitePool,
    user_npub: &str,
    limit: i32,
) -> Result<Vec<PostHistory>> {
    let rows = sqlx::query(
        "SELECT id, user_npub, content, event_id, relay_count, is_scheduled, posted_at FROM post_history WHERE user_npub = ? ORDER BY posted_at DESC LIMIT ?"
    )
    .bind(user_npub)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to fetch post history")?;

    Ok(rows.into_iter().map(|r| {
        let is_sched: i32 = r.get("is_scheduled");
        PostHistory {
            id: r.get("id"),
            user_npub: r.get("user_npub"),
            content: r.get("content"),
            event_id: r.get("event_id"),
            relay_count: r.get("relay_count"),
            is_scheduled: is_sched == 1,
            posted_at: r.get("posted_at"),
        }
    }).collect())
}

/// Get total post count for a user.
pub async fn get_post_count(pool: &SqlitePool, user_npub: &str) -> Result<i32> {
    let count: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM post_history WHERE user_npub = ?")
        .bind(user_npub)
        .fetch_one(pool)
        .await
        .context("Failed to count posts")?;

    Ok(count)
}

/// Delete old history entries (keep last N).
pub async fn cleanup_history(pool: &SqlitePool, user_npub: &str, keep_count: i32) -> Result<i32> {
    let result = sqlx::query(
        "DELETE FROM post_history WHERE user_npub = ? AND id NOT IN (SELECT id FROM post_history WHERE user_npub = ? ORDER BY posted_at DESC LIMIT ?)"
    )
    .bind(user_npub)
    .bind(user_npub)
    .bind(keep_count)
    .execute(pool)
    .await
    .context("Failed to cleanup history")?;

    Ok(result.rows_affected() as i32)
}

