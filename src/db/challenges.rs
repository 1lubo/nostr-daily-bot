//! Auth challenge database operations.

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::AuthChallenge;

/// Challenge validity duration in seconds.
const CHALLENGE_EXPIRY_SECONDS: i64 = 300; // 5 minutes

/// Create a new auth challenge for a user.
pub async fn create_challenge(pool: &SqlitePool, npub: &str) -> Result<AuthChallenge> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires_at = now + Duration::seconds(CHALLENGE_EXPIRY_SECONDS);
    
    // Challenge format: app-name:id:timestamp
    let challenge = format!("nostr-daily-bot:{}:{}", id, now.timestamp());
    
    let created_at = now.to_rfc3339();
    let expires_at_str = expires_at.to_rfc3339();

    sqlx::query(
        "INSERT INTO auth_challenges (id, npub, challenge, created_at, expires_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(npub)
    .bind(&challenge)
    .bind(&created_at)
    .bind(&expires_at_str)
    .execute(pool)
    .await
    .context("Failed to create challenge")?;

    Ok(AuthChallenge {
        id,
        npub: npub.to_string(),
        challenge,
        created_at,
        expires_at: expires_at_str,
        used: false,
    })
}

/// Get a challenge by ID.
pub async fn get_challenge(pool: &SqlitePool, id: &str) -> Result<Option<AuthChallenge>> {
    let row = sqlx::query(
        "SELECT id, npub, challenge, created_at, expires_at, used FROM auth_challenges WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch challenge")?;

    Ok(row.map(|r| {
        let used_int: i32 = r.get("used");
        AuthChallenge {
            id: r.get("id"),
            npub: r.get("npub"),
            challenge: r.get("challenge"),
            created_at: r.get("created_at"),
            expires_at: r.get("expires_at"),
            used: used_int == 1,
        }
    }))
}

/// Verify a challenge is valid (exists, not expired, not used, matches npub).
pub async fn verify_challenge(pool: &SqlitePool, id: &str, npub: &str) -> Result<Option<AuthChallenge>> {
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
pub async fn mark_challenge_used(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("UPDATE auth_challenges SET used = 1 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to mark challenge used")?;

    Ok(())
}

/// Clean up expired challenges.
pub async fn cleanup_expired_challenges(pool: &SqlitePool) -> Result<i32> {
    let now = Utc::now().to_rfc3339();
    
    let result = sqlx::query("DELETE FROM auth_challenges WHERE expires_at < ?")
        .bind(&now)
        .execute(pool)
        .await
        .context("Failed to cleanup expired challenges")?;

    Ok(result.rows_affected() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(include_str!("../../migrations/001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/002_presigning.sql"))
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_create_and_verify_challenge() {
        let pool = setup_test_db().await;
        
        let challenge = create_challenge(&pool, "npub1test").await.unwrap();
        assert!(!challenge.used);
        
        let verified = verify_challenge(&pool, &challenge.id, "npub1test").await.unwrap();
        assert!(verified.is_some());
        
        // Wrong npub should fail
        let wrong = verify_challenge(&pool, &challenge.id, "npub1wrong").await.unwrap();
        assert!(wrong.is_none());
    }

    #[tokio::test]
    async fn test_challenge_cannot_be_reused() {
        let pool = setup_test_db().await;
        
        let challenge = create_challenge(&pool, "npub1test").await.unwrap();
        mark_challenge_used(&pool, &challenge.id).await.unwrap();
        
        let verified = verify_challenge(&pool, &challenge.id, "npub1test").await.unwrap();
        assert!(verified.is_none());
    }
}

