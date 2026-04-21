//! Database connection pool and initialization.

use std::env;

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;

/// Type alias for the PostgreSQL connection pool.
pub type DbPool = PgPool;

/// Get the database URL from environment.
pub fn database_url() -> Result<String> {
    env::var("DATABASE_URL").context(
        "DATABASE_URL environment variable not set. \
         Set it to a PostgreSQL connection string like: \
         postgres://user:password@host/database",
    )
}

/// Initialize the database connection pool and run migrations.
pub async fn init_db() -> Result<DbPool> {
    let db_url = database_url()?;

    // Mask password in log
    let masked_url = mask_db_url(&db_url);
    info!(url = %masked_url, "Connecting to PostgreSQL database");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .context("Failed to connect to PostgreSQL database")?;

    // Run migrations
    run_migrations(&pool).await?;

    info!("Database initialized successfully");
    Ok(pool)
}

/// Run database migrations.
async fn run_migrations(pool: &PgPool) -> Result<()> {
    info!("Running database migrations");

    // Run all migrations in order
    let migrations = [
        include_str!("../../migrations/001_initial.sql"),
        include_str!("../../migrations/002_presigning.sql"),
        include_str!("../../migrations/003_payments.sql"),
    ];

    for (i, migration_sql) in migrations.iter().enumerate() {
        sqlx::raw_sql(migration_sql)
            .execute(pool)
            .await
            .with_context(|| format!("Failed to run migration {}", i + 1))?;
    }

    info!("Migrations complete");
    Ok(())
}

/// Mask the password in a database URL for logging.
fn mask_db_url(url: &str) -> String {
    // Simple masking: replace password portion
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let prefix = &url[..colon_pos + 1];
            let suffix = &url[at_pos..];
            return format!("{}****{}", prefix, suffix);
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_db_url() {
        let url = "postgres://user:secret123@localhost/db";
        let masked = mask_db_url(url);
        assert_eq!(masked, "postgres://user:****@localhost/db");
    }

    #[test]
    fn test_mask_db_url_no_password() {
        let url = "postgres://localhost/db";
        let masked = mask_db_url(url);
        assert_eq!(masked, "postgres://localhost/db");
    }
}
