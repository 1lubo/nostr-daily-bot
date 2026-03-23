//! Quote database operations.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use crate::models::Quote;

#[derive(FromRow)]
struct QuoteRow {
    id: i64,
    user_npub: String,
    content: String,
    sort_order: i32,
    created_at: DateTime<Utc>,
}

/// Get all quotes for a user.
pub async fn get_quotes(pool: &PgPool, user_npub: &str) -> Result<Vec<Quote>> {
    let rows: Vec<QuoteRow> = sqlx::query_as(
        "SELECT id, user_npub, content, sort_order, created_at FROM quotes WHERE user_npub = $1 ORDER BY sort_order ASC, id ASC"
    )
    .bind(user_npub)
    .fetch_all(pool)
    .await
    .context("Failed to fetch quotes")?;

    Ok(rows.into_iter().map(|r| Quote {
        id: r.id,
        user_npub: r.user_npub,
        content: r.content,
        sort_order: r.sort_order,
        created_at: r.created_at.to_rfc3339(),
    }).collect())
}

/// Get quote count for a user.
pub async fn get_quote_count(pool: &PgPool, user_npub: &str) -> Result<i32> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quotes WHERE user_npub = $1")
        .bind(user_npub)
        .fetch_one(pool)
        .await
        .context("Failed to count quotes")?;

    Ok(count as i32)
}

/// Replace all quotes for a user.
pub async fn replace_quotes(pool: &PgPool, user_npub: &str, quotes: &[String]) -> Result<()> {
    let mut tx = pool.begin().await.context("Failed to start transaction")?;

    // Delete existing quotes
    sqlx::query("DELETE FROM quotes WHERE user_npub = $1")
        .bind(user_npub)
        .execute(&mut *tx)
        .await
        .context("Failed to delete existing quotes")?;

    // Insert new quotes
    for (i, content) in quotes.iter().enumerate() {
        let sort_order = i as i32;
        sqlx::query("INSERT INTO quotes (user_npub, content, sort_order) VALUES ($1, $2, $3)")
            .bind(user_npub)
            .bind(content)
            .bind(sort_order)
            .execute(&mut *tx)
            .await
            .context("Failed to insert quote")?;
    }

    tx.commit().await.context("Failed to commit transaction")?;
    Ok(())
}

/// Add a single quote for a user.
pub async fn add_quote(pool: &PgPool, user_npub: &str, content: &str) -> Result<Quote> {
    // Get the next sort order
    let max_order: Option<i32> = sqlx::query_scalar("SELECT MAX(sort_order) FROM quotes WHERE user_npub = $1")
        .bind(user_npub)
        .fetch_one(pool)
        .await
        .context("Failed to get max sort order")?;

    let sort_order = max_order.unwrap_or(-1) + 1;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO quotes (user_npub, content, sort_order) VALUES ($1, $2, $3) RETURNING id"
    )
    .bind(user_npub)
    .bind(content)
    .bind(sort_order)
    .fetch_one(pool)
    .await
    .context("Failed to insert quote")?;

    Ok(Quote {
        id,
        user_npub: user_npub.to_string(),
        content: content.to_string(),
        sort_order,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Delete a quote by ID (must belong to user).
pub async fn delete_quote(pool: &PgPool, user_npub: &str, quote_id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM quotes WHERE id = $1 AND user_npub = $2")
        .bind(quote_id)
        .bind(user_npub)
        .execute(pool)
        .await
        .context("Failed to delete quote")?;

    Ok(result.rows_affected() > 0)
}

