//! Scheduler module for cron-based job execution.

mod error;
pub mod presign;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::FutureExt;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};
use uuid::Uuid;

pub use error::{Result, SchedulerError};

/// Type alias for the async posting callback.
pub type PostingCallback = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Configuration for the scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Cron expression for scheduling.
    pub cron_expression: String,
    /// Timezone for cron interpretation.
    pub timezone: String,
}

/// The Scheduler manages scheduled job execution.
pub struct Scheduler {
    inner: Option<JobScheduler>,
    config: SchedulerConfig,
}

impl Scheduler {
    /// Create a new Scheduler with the given configuration.
    pub async fn new(config: SchedulerConfig) -> Result<Self> {
        info!(cron = %config.cron_expression, tz = %config.timezone, "Creating scheduler");

        let scheduler = JobScheduler::new()
            .await
            .map_err(SchedulerError::Creation)?;

        Ok(Self {
            inner: Some(scheduler),
            config,
        })
    }

    fn scheduler(&self) -> Result<&JobScheduler> {
        self.inner.as_ref().ok_or(SchedulerError::NotInitialized)
    }

    /// Register the daily posting job with an async callback.
    pub async fn register_posting_job(&mut self, callback: PostingCallback) -> Result<Uuid> {
        let cron = &self.config.cron_expression;
        let tz_str = &self.config.timezone;

        info!(cron = %cron, timezone = %tz_str, "Registering posting job");

        // Parse timezone
        let timezone: chrono_tz::Tz = tz_str.parse().map_err(|_| SchedulerError::InvalidCron {
            expr: tz_str.clone(),
            reason: "Invalid timezone".to_string(),
        })?;

        // Create the async job
        let job = Job::new_async_tz(cron, timezone, move |uuid, _lock| {
            let cb = Arc::clone(&callback);
            Box::pin(async move {
                info!(job_id = %uuid, "Executing scheduled job");

                // Execute callback with panic protection
                let result = std::panic::AssertUnwindSafe(cb()).catch_unwind().await;

                match result {
                    Ok(()) => info!(job_id = %uuid, "Job completed successfully"),
                    Err(_) => error!(job_id = %uuid, "Job panicked - scheduler continues"),
                }
            })
        })
        .map_err(|e| SchedulerError::InvalidCron {
            expr: cron.clone(),
            reason: e.to_string(),
        })?;

        let job_id = job.guid();

        self.scheduler()?
            .add(job)
            .await
            .map_err(|e| SchedulerError::JobAddition(e.to_string()))?;

        info!(job_id = %job_id, "Posting job registered");
        Ok(job_id)
    }

    /// Start the scheduler.
    pub async fn start(&self) -> Result<()> {
        info!("Starting scheduler");

        self.scheduler()?
            .start()
            .await
            .map_err(|e| SchedulerError::Start(e.to_string()))?;

        info!("Scheduler started");
        Ok(())
    }

    /// Stop the scheduler gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping scheduler");

        if let Some(mut scheduler) = self.inner.take() {
            scheduler
                .shutdown()
                .await
                .map_err(|e| SchedulerError::Shutdown(e.to_string()))?;
        }

        info!("Scheduler stopped");
        Ok(())
    }
}
