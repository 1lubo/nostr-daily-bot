//! Database connection pool and initialization.

use std::path::PathBuf;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tracing::info;

/// Type alias for the SQLite connection pool.
pub type DbPool = SqlitePool;

/// Get the database file path.
pub fn db_path() -> Result<PathBuf> {
    let proj_dirs = directories::ProjectDirs::from("com", "nostr", "nostr-daily-bot")
        .context("Could not determine config directory")?;

    let data_dir = proj_dirs.data_dir();

    // Create directory if it doesn't exist
    if !data_dir.exists() {
        std::fs::create_dir_all(data_dir).context("Failed to create data directory")?;
    }

    Ok(data_dir.join("nostr_daily_bot.db"))
}

/// Initialize the database connection pool and run migrations.
pub async fn init_db() -> Result<DbPool> {
    let db_path = db_path()?;
    
    info!(path = %db_path.display(), "Initializing database");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("Failed to connect to database")?;

    // Run migrations
    run_migrations(&pool).await?;

    info!("Database initialized successfully");
    Ok(pool)
}

/// Run database migrations.
async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    info!("Running database migrations");

    // Read and execute migration file
    let migration_sql = include_str!("../../migrations/001_initial.sql");

    sqlx::raw_sql(migration_sql)
        .execute(pool)
        .await
        .context("Failed to run migrations")?;

    info!("Migrations complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_db() {
        // Use in-memory database for testing
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        let migration_sql = include_str!("../../migrations/001_initial.sql");
        sqlx::raw_sql(migration_sql)
            .execute(&pool)
            .await
            .unwrap();

        // Verify tables exist
        let result: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(result.0, 0);
    }
}

