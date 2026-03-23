//! Auth challenge database operations.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::models::AuthChallenge;

/// Challenge validity duration in seconds.
const CHALLENGE_EXPIRY_SECONDS: i64 = 300; // 5 minutes

#[derive(FromRow)]
struct ChallengeRow {
    id: String,
    npub: String,
    challenge: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    used: bool,
}

/// Create a new auth challenge for a user.
pub async fn create_challenge(pool: &PgPool, npub: &str) -> Result<AuthChallenge> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires_at = now + Duration::seconds(CHALLENGE_EXPIRY_SECONDS);

    // Challenge format: app-name:id:timestamp
    let challenge = format!("nostr-daily-bot:{}:{}", id, now.timestamp());

    sqlx::query(
        "INSERT INTO auth_challenges (id, npub, challenge, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(&id)
    .bind(npub)
    .bind(&challenge)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await
    .context("Failed to create challenge")?;

    Ok(AuthChallenge {
        id,
        npub: npub.to_string(),
        challenge,
        created_at: now.to_rfc3339(),
        expires_at: expires_at.to_rfc3339(),
        used: false,
    })
}

/// Get a challenge by ID.
pub async fn get_challenge(pool: &PgPool, id: &str) -> Result<Option<AuthChallenge>> {
    let row: Option<ChallengeRow> = sqlx::query_as(
        "SELECT id, npub, challenge, created_at, expires_at, used FROM auth_challenges WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch challenge")?;

    Ok(row.map(|r| AuthChallenge {
        id: r.id,
        npub: r.npub,
        challenge: r.challenge,
        created_at: r.created_at.to_rfc3339(),
        expires_at: r.expires_at.to_rfc3339(),
        used: r.used,
    }))
}

/// Verify a challenge is valid (exists, not expired, not used, matches npub).
pub async fn verify_challenge(
    pool: &PgPool,
    id: &str,
    npub: &str,
) -> Result<Option<AuthChallenge>> {
    let challenge = match get_challenge(pool, id).await? {
        Some(c) => c,
        None => return Ok(None),
    };

    // Check if already used
    if challenge.used {
        return Ok(None);
    }

    // Check if expired
    let expires_at = chrono::DateTime::parse_from_rfc3339(&challenge.expires_at)
        .context("Invalid expires_at format")?;
    if Utc::now() > expires_at {
        return Ok(None);
    }

    // Check if npub matches
    if challenge.npub != npub {
        return Ok(None);
    }

    Ok(Some(challenge))
}

/// Mark a challenge as used.
pub async fn mark_challenge_used(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query("UPDATE auth_challenges SET used = TRUE WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to mark challenge used")?;

    Ok(())
}

/// Clean up expired challenges.
#[allow(dead_code)]
pub async fn cleanup_expired_challenges(pool: &PgPool) -> Result<i32> {
    let now = Utc::now();

    let result = sqlx::query("DELETE FROM auth_challenges WHERE expires_at < $1")
        .bind(now)
        .execute(pool)
        .await
        .context("Failed to cleanup expired challenges")?;

    Ok(result.rows_affected() as i32)
}

// Tests removed - PostgreSQL tests require a running database
// Consider adding integration tests with testcontainers
