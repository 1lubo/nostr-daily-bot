//! Error types for the scheduler module.

use thiserror::Error;
use tokio_cron_scheduler::JobSchedulerError;

/// Errors that can occur in the scheduler.
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Failed to create scheduler: {0}")]
    Creation(#[from] JobSchedulerError),

    #[error("Invalid cron expression '{expr}': {reason}")]
    InvalidCron { expr: String, reason: String },

    #[error("Failed to add job: {0}")]
    JobAddition(String),

    #[error("Failed to start scheduler: {0}")]
    Start(String),

    #[error("Failed to shutdown scheduler: {0}")]
    Shutdown(String),

    #[error("Scheduler not initialized")]
    NotInitialized,
}

/// Result type for scheduler operations.
pub type Result<T> = std::result::Result<T, SchedulerError>;
