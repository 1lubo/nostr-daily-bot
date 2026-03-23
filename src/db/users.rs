//! User database operations.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use crate::models::{User, UserInput};

#[derive(FromRow)]
struct UserRow {
    npub: String,
    display_name: Option<String>,
    cron: String,
    timezone: String,
    auth_mode: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Get a user by npub.
pub async fn get_user(pool: &PgPool, npub: &str) -> Result<Option<User>> {
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT npub, display_name, cron, timezone, auth_mode, created_at, updated_at FROM users WHERE npub = $1"
    )
    .bind(npub)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch user")?;

    Ok(row.map(|r| User {
        npub: r.npub,
        display_name: r.display_name,
        cron: r.cron,
        timezone: r.timezone,
        auth_mode: r.auth_mode,
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.to_rfc3339(),
    }))
}

/// Create a new user or update if exists.
pub async fn upsert_user(pool: &PgPool, npub: &str, input: &UserInput) -> Result<User> {
    let cron = input.cron.as_deref().unwrap_or("0 0 9 * * *");
    let timezone = input.timezone.as_deref().unwrap_or("UTC");

    sqlx::query(
        r#"
        INSERT INTO users (npub, display_name, cron, timezone)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT(npub) DO UPDATE SET
            display_name = COALESCE(EXCLUDED.display_name, users.display_name),
            cron = EXCLUDED.cron,
            timezone = EXCLUDED.timezone,
            updated_at = NOW()
        "#,
    )
    .bind(npub)
    .bind(&input.display_name)
    .bind(cron)
    .bind(timezone)
    .execute(pool)
    .await
    .context("Failed to upsert user")?;

    get_user(pool, npub)
        .await?
        .context("User not found after upsert")
}

/// Update user's schedule.
pub async fn update_schedule(pool: &PgPool, npub: &str, cron: &str) -> Result<()> {
    sqlx::query("UPDATE users SET cron = $1, updated_at = NOW() WHERE npub = $2")
        .bind(cron)
        .bind(npub)
        .execute(pool)
        .await
        .context("Failed to update schedule")?;

    Ok(())
}

/// Delete a user and all their data.
#[allow(dead_code)]
pub async fn delete_user(pool: &PgPool, npub: &str) -> Result<()> {
    sqlx::query("DELETE FROM users WHERE npub = $1")
        .bind(npub)
        .execute(pool)
        .await
        .context("Failed to delete user")?;

    Ok(())
}

/// Check if a user exists.
#[allow(dead_code)]
pub async fn user_exists(pool: &PgPool, npub: &str) -> Result<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE npub = $1")
        .bind(npub)
        .fetch_one(pool)
        .await
        .context("Failed to check user existence")?;

    Ok(count > 0)
}

/// Update user's auth mode.
pub async fn update_auth_mode(pool: &PgPool, npub: &str, auth_mode: &str) -> Result<()> {
    sqlx::query("UPDATE users SET auth_mode = $1, updated_at = NOW() WHERE npub = $2")
        .bind(auth_mode)
        .bind(npub)
        .execute(pool)
        .await
        .context("Failed to update auth mode")?;

    Ok(())
}
