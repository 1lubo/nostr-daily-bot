//! User database operations.

use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};

use crate::models::{User, UserInput};

/// Get a user by npub.
pub async fn get_user(pool: &SqlitePool, npub: &str) -> Result<Option<User>> {
    let row = sqlx::query(
        "SELECT npub, display_name, cron, timezone, created_at, updated_at FROM users WHERE npub = ?"
    )
    .bind(npub)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch user")?;

    Ok(row.map(|r| User {
        npub: r.get("npub"),
        display_name: r.get("display_name"),
        cron: r.get("cron"),
        timezone: r.get("timezone"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }))
}

/// Create a new user or update if exists.
pub async fn upsert_user(pool: &SqlitePool, npub: &str, input: &UserInput) -> Result<User> {
    let cron = input.cron.as_deref().unwrap_or("0 0 9 * * *");
    let timezone = input.timezone.as_deref().unwrap_or("UTC");

    sqlx::query(
        r#"
        INSERT INTO users (npub, display_name, cron, timezone)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(npub) DO UPDATE SET
            display_name = COALESCE(excluded.display_name, users.display_name),
            cron = excluded.cron,
            timezone = excluded.timezone,
            updated_at = datetime('now')
        "#
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
pub async fn update_schedule(pool: &SqlitePool, npub: &str, cron: &str) -> Result<()> {
    sqlx::query("UPDATE users SET cron = ?, updated_at = datetime('now') WHERE npub = ?")
        .bind(cron)
        .bind(npub)
        .execute(pool)
        .await
        .context("Failed to update schedule")?;

    Ok(())
}

/// Delete a user and all their data.
pub async fn delete_user(pool: &SqlitePool, npub: &str) -> Result<()> {
    sqlx::query("DELETE FROM users WHERE npub = ?")
        .bind(npub)
        .execute(pool)
        .await
        .context("Failed to delete user")?;

    Ok(())
}

/// Check if a user exists.
pub async fn user_exists(pool: &SqlitePool, npub: &str) -> Result<bool> {
    let count: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE npub = ?")
        .bind(npub)
        .fetch_one(pool)
        .await
        .context("Failed to check user existence")?;

    Ok(count > 0)
}

