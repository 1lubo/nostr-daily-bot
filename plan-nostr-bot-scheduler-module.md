# Implementation Plan: Scheduler Module for Rust Nostr Bot

## 1. Overview

### Description
A scheduler module for the Rust Nostr bot that handles daily message posting based on cron expressions. The module wraps `tokio-cron-scheduler` to provide a clean interface for scheduling async jobs with proper lifecycle management and graceful shutdown support.

### Goals and Success Criteria
- ✅ Schedule jobs using cron expressions from configuration
- ✅ Support async job callbacks that can call the Nostr client
- ✅ Graceful shutdown on SIGTERM/SIGINT signals
- ✅ Handle job execution failures without crashing (log and continue)
- ✅ Clean start/stop lifecycle methods
- ✅ Thread-safe shared state using Arc for Nostr client access

### Scope Boundaries
- **Included**: Scheduler struct, cron job registration, lifecycle management, signal handling
- **Excluded**: Message selection logic, Nostr client implementation (uses existing module)

---

## 2. Prerequisites

### Dependencies (Cargo.toml)
```toml
[dependencies]
tokio-cron-scheduler = "0.15"   # Async cron job scheduling
tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal"] }
thiserror = "2"                  # Error handling
tracing = "0.1"                  # Logging
uuid = { version = "1", features = ["v4"] }  # Job IDs
chrono = "0.4"                   # Time handling (already in tokio-cron-scheduler)
chrono-tz = "0.10"               # Timezone support
```

### Project Structure
```
src/
├── main.rs
├── config/           # Existing config module
├── nostr/            # Existing Nostr client module
└── scheduler/
    ├── mod.rs        # Module exports and Scheduler struct
    └── error.rs      # Custom error types
```

---

## 3. Implementation Steps

### Step 1: Define Error Types (`src/scheduler/error.rs`)

**Description**: Create custom error types for scheduler operations.

**Key Patterns**:
- `thiserror` for deriving `std::error::Error`
- Wrapping `JobSchedulerError` from the crate

```rust
use thiserror::Error;
use tokio_cron_scheduler::JobSchedulerError;

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

pub type Result<T> = std::result::Result<T, SchedulerError>;
```

---

### Step 2: Define the Scheduler Struct (`src/scheduler/mod.rs`)

**Description**: Create the main scheduler wrapper with shared state support.

**Key Patterns**:
- `Arc<T>` for thread-safe shared ownership
- Generic callback type for flexibility
- `Option<JobScheduler>` for lifecycle management

```rust
pub mod error;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub use error::{Result, SchedulerError};

/// Type alias for the async posting callback
/// The callback receives nothing and returns a pinned future
pub type PostingCallback = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync
>;

/// Configuration for the scheduler
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Cron expression for scheduling (e.g., "0 0 9 * * *" for 9 AM daily)
    pub cron_expression: String,
    /// Timezone for cron interpretation (e.g., "UTC", "Europe/London")
    pub timezone: String,
}

/// The Scheduler manages scheduled job execution
pub struct Scheduler {
    /// The underlying job scheduler
    inner: Option<JobScheduler>,
    /// Configuration
    config: SchedulerConfig,
    /// Job ID for the posting job (for removal if needed)
    posting_job_id: Option<Uuid>,
}
```

---

### Step 3: Implement Scheduler Initialization

**Description**: Initialize the scheduler with proper error handling.

```rust
impl Scheduler {
    /// Create a new Scheduler with the given configuration
    pub async fn new(config: SchedulerConfig) -> Result<Self> {
        info!("Creating scheduler with cron: {}", config.cron_expression);
        
        let scheduler = JobScheduler::new()
            .await
            .map_err(SchedulerError::Creation)?;

        Ok(Self {
            inner: Some(scheduler),
            config,
            posting_job_id: None,
        })
    }

    /// Get reference to the inner scheduler
    fn scheduler(&self) -> Result<&JobScheduler> {
        self.inner.as_ref().ok_or(SchedulerError::NotInitialized)
    }

    /// Get mutable reference to the inner scheduler
    fn scheduler_mut(&mut self) -> Result<&mut JobScheduler> {
        self.inner.as_mut().ok_or(SchedulerError::NotInitialized)
    }
}
```

---

### Step 4: Implement Job Registration with Callback

**Description**: Register the posting job with an async callback that handles errors gracefully.

**Key Patterns**:
- `Arc` cloning for closure capture
- `Box::pin` for async closures
- Error handling inside the job (log and continue)

```rust
impl Scheduler {
    /// Register the daily posting job with an async callback
    ///
    /// The callback should handle posting to Nostr. Any errors in the callback
    /// are caught and logged - they won't crash the scheduler.
    ///
    /// # Arguments
    /// * `callback` - Async function that performs the Nostr posting
    ///
    /// # Example
    /// ```rust
    /// let nostr_client = Arc::new(nostr_client);
    /// let client_clone = Arc::clone(&nostr_client);
    /// 
    /// scheduler.register_posting_job(Arc::new(move || {
    ///     let client = Arc::clone(&client_clone);
    ///     Box::pin(async move {
    ///         if let Err(e) = client.publish_text_note("Hello!").await {
    ///             error!("Failed to post: {}", e);
    ///         }
    ///     })
    /// })).await?;
    /// ```
    pub async fn register_posting_job(&mut self, callback: PostingCallback) -> Result<Uuid> {
        let cron = &self.config.cron_expression;
        let tz_str = &self.config.timezone;
        
        info!("Registering posting job with cron: {} (timezone: {})", cron, tz_str);
        
        // Parse timezone
        let timezone: chrono_tz::Tz = tz_str.parse()
            .map_err(|_| SchedulerError::InvalidCron {
                expr: tz_str.clone(),
                reason: "Invalid timezone".to_string(),
            })?;

        // Create the async job with error handling wrapper
        let job = Job::new_async_tz(cron, timezone, move |uuid, _lock| {
            let cb = Arc::clone(&callback);
            Box::pin(async move {
                info!("Executing scheduled posting job (id: {})", uuid);
                
                // Execute callback with panic protection
                let result = std::panic::AssertUnwindSafe(cb())
                    .catch_unwind()
                    .await;
                
                match result {
                    Ok(()) => info!("Posting job completed successfully"),
                    Err(_) => error!("Posting job panicked - scheduler continues"),
                }
            })
        }).map_err(|e| SchedulerError::InvalidCron {
            expr: cron.clone(),
            reason: e.to_string(),
        })?;

        let job_id = job.guid();
        
        self.scheduler()?
            .add(job)
            .await
            .map_err(|e| SchedulerError::JobAddition(e.to_string()))?;

        self.posting_job_id = Some(job_id);
        info!("Posting job registered with ID: {}", job_id);
        
        Ok(job_id)
    }
}
```

---

### Step 5: Implement Start/Stop Lifecycle Methods

**Description**: Methods to start and gracefully stop the scheduler.

**Key Patterns**:
- `scheduler.start()` spawns background task
- `scheduler.shutdown()` for graceful stop
- Shutdown handler for cleanup

```rust
impl Scheduler {
    /// Start the scheduler
    ///
    /// This spawns a background task that checks for due jobs every 500ms.
    /// Call this after registering all jobs.
    pub async fn start(&self) -> Result<()> {
        info!("Starting scheduler...");

        self.scheduler()?
            .start()
            .await
            .map_err(|e| SchedulerError::Start(e.to_string()))?;

        info!("Scheduler started successfully");
        Ok(())
    }

    /// Stop the scheduler gracefully
    ///
    /// This will:
    /// 1. Stop accepting new job executions
    /// 2. Wait for currently running jobs to complete
    /// 3. Clean up resources
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping scheduler...");

        if let Some(mut scheduler) = self.inner.take() {
            scheduler
                .shutdown()
                .await
                .map_err(|e| SchedulerError::Shutdown(e.to_string()))?;
        }

        info!("Scheduler stopped");
        Ok(())
    }

    /// Check if the scheduler is running
    pub fn is_running(&self) -> bool {
        self.inner.is_some()
    }

    /// Get the posting job ID if registered
    pub fn posting_job_id(&self) -> Option<Uuid> {
        self.posting_job_id
    }

    /// Remove the posting job (if you need to re-register with different callback)
    pub async fn remove_posting_job(&mut self) -> Result<()> {
        if let Some(job_id) = self.posting_job_id.take() {
            self.scheduler()?
                .remove(&job_id)
                .await
                .map_err(|e| SchedulerError::JobAddition(e.to_string()))?;
            info!("Removed posting job: {}", job_id);
        }
        Ok(())
    }
}
```

---

### Step 6: Implement Graceful Shutdown with Signal Handling

**Description**: Create a helper function that integrates with tokio's signal handling for graceful shutdown.

**Key Patterns**:
- `tokio::signal::ctrl_c()` for SIGINT
- `tokio::signal::unix::signal(SignalKind::terminate())` for SIGTERM
- `tokio::select!` for racing between signals

```rust
use tokio::signal;

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

/// Run the scheduler until a shutdown signal is received
///
/// This function:
/// 1. Starts the scheduler
/// 2. Waits for SIGTERM or SIGINT (Ctrl+C)
/// 3. Gracefully shuts down the scheduler
///
/// # Example
/// ```rust
/// let scheduler = Scheduler::new(config).await?;
/// scheduler.register_posting_job(callback).await?;
/// scheduler.run_until_shutdown().await?;
/// ```
pub async fn run_until_shutdown(mut scheduler: Scheduler) -> Result<()> {
    // Start the scheduler
    scheduler.start().await?;

    info!("Scheduler running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    wait_for_shutdown_signal().await;

    info!("Shutdown signal received, stopping scheduler...");

    // Graceful shutdown
    scheduler.stop().await?;

    info!("Scheduler shutdown complete");
    Ok(())
}

/// Wait for either SIGTERM or SIGINT
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = signal(SignalKind::terminate())
            .expect("Failed to install SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt())
            .expect("Failed to install SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT (Ctrl+C)");
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On Windows, just handle Ctrl+C
        signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
        info!("Received Ctrl+C");
    }
}
```

---

### Step 7: Alternative Pattern - Scheduler with Embedded Signal Handling

**Description**: An alternative design where the scheduler manages its own shutdown via `tokio::select!`.

```rust
impl Scheduler {
    /// Run the scheduler with integrated shutdown handling
    ///
    /// This is a blocking call that runs until a shutdown signal is received.
    /// Useful for simple applications where the scheduler is the main loop.
    pub async fn run_with_shutdown(mut self) -> Result<()> {
        self.start().await?;

        info!("Scheduler running, waiting for shutdown signal...");

        // Use tokio::select! to race between the scheduler running and shutdown
        tokio::select! {
            // The scheduler runs indefinitely, so this branch won't complete
            // unless there's an internal error
            _ = async {
                // Keep the scheduler alive
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                }
            } => {
                warn!("Scheduler loop exited unexpectedly");
            }

            // Wait for shutdown signal
            _ = wait_for_shutdown_signal() => {
                info!("Initiating graceful shutdown...");
            }
        }

        self.stop().await?;
        Ok(())
    }
}
```

---

### Step 8: Module Exports (`src/scheduler/mod.rs`)

**Description**: Clean module organization with re-exports.

```rust
pub mod error;

// ... (implementation above)

// Re-exports for convenience
pub use error::{Result, SchedulerError};
pub use tokio_cron_scheduler::JobSchedulerError;
```

---

## 4. File Changes Summary

### Files to Create

| File | Purpose |
|------|---------|
| `src/scheduler/mod.rs` | Scheduler struct, lifecycle methods, signal handling |
| `src/scheduler/error.rs` | Custom error types |

### Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Add dependencies: `tokio-cron-scheduler`, `chrono-tz` |
| `src/main.rs` | Add `mod scheduler;` and integration code |

---

## 5. Complete Usage Example

```rust
// src/main.rs
mod config;
mod nostr;
mod scheduler;

use std::sync::Arc;
use tracing::{error, info};

use config::Config;
use nostr::NostrClient;
use scheduler::{Scheduler, SchedulerConfig, run_until_shutdown};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = Config::load("config.toml")?;

    // Initialize Nostr client
    let keys = NostrClient::keys_parse(&config.get_private_key()?)?;
    let nostr_client = Arc::new(NostrClient::with_keys(keys).await?);
    nostr_client.connect().await?;

    info!("Connected to {} relay(s)", nostr_client.connected_relay_count().await);

    // Create scheduler
    let scheduler_config = SchedulerConfig {
        cron_expression: config.schedule.cron.clone(),
        timezone: config.schedule.timezone.clone(),
    };
    let mut scheduler = Scheduler::new(scheduler_config).await?;

    // Clone client for the closure
    let client_for_job = Arc::clone(&nostr_client);

    // Message templates from config
    let templates = Arc::new(config.messages.templates.clone());
    let template_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Register the posting job
    scheduler.register_posting_job(Arc::new(move || {
        let client = Arc::clone(&client_for_job);
        let templates = Arc::clone(&templates);
        let index = Arc::clone(&template_index);

        Box::pin(async move {
            // Get next message (sequential rotation)
            let idx = index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let message = &templates[idx % templates.len()];

            info!("Posting scheduled message: {}", message);

            match client.publish_text_note(message).await {
                Ok(event_id) => {
                    info!("Posted successfully! Event ID: {}", event_id);
                }
                Err(e) => {
                    error!("Failed to post message: {}", e);
                    // Don't panic - the scheduler will continue
                }
            }
        })
    })).await?;

    // Run until shutdown signal
    run_until_shutdown(scheduler).await?;

    // Cleanup
    nostr_client.shutdown().await;
    info!("Application shutdown complete");

    Ok(())
}
```

---

## 6. Testing Strategy

### Unit Tests (`src/scheduler/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn test_scheduler_creation() {
        let config = SchedulerConfig {
            cron_expression: "0 * * * * *".to_string(), // Every minute
            timezone: "UTC".to_string(),
        };

        let scheduler = Scheduler::new(config).await;
        assert!(scheduler.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_cron() {
        let config = SchedulerConfig {
            cron_expression: "invalid cron".to_string(),
            timezone: "UTC".to_string(),
        };

        let mut scheduler = Scheduler::new(config).await.unwrap();

        let result = scheduler.register_posting_job(Arc::new(|| {
            Box::pin(async {})
        })).await;

        assert!(matches!(result, Err(SchedulerError::InvalidCron { .. })));
    }

    #[tokio::test]
    async fn test_invalid_timezone() {
        let config = SchedulerConfig {
            cron_expression: "0 * * * * *".to_string(),
            timezone: "Invalid/Timezone".to_string(),
        };

        let mut scheduler = Scheduler::new(config).await.unwrap();

        let result = scheduler.register_posting_job(Arc::new(|| {
            Box::pin(async {})
        })).await;

        assert!(matches!(result, Err(SchedulerError::InvalidCron { .. })));
    }

    #[tokio::test]
    async fn test_job_execution() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let config = SchedulerConfig {
            cron_expression: "* * * * * *".to_string(), // Every second
            timezone: "UTC".to_string(),
        };

        let mut scheduler = Scheduler::new(config).await.unwrap();

        scheduler.register_posting_job(Arc::new(move || {
            let c = Arc::clone(&counter_clone);
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
            })
        })).await.unwrap();

        scheduler.start().await.unwrap();

        // Wait for at least one execution
        tokio::time::sleep(Duration::from_secs(2)).await;

        scheduler.stop().await.unwrap();

        assert!(counter.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn test_job_error_handling() {
        // Test that panics in jobs don't crash the scheduler
        let config = SchedulerConfig {
            cron_expression: "* * * * * *".to_string(),
            timezone: "UTC".to_string(),
        };

        let mut scheduler = Scheduler::new(config).await.unwrap();

        scheduler.register_posting_job(Arc::new(|| {
            Box::pin(async {
                panic!("Simulated job failure");
            })
        })).await.unwrap();

        scheduler.start().await.unwrap();

        // Scheduler should still be running after panic
        tokio::time::sleep(Duration::from_secs(2)).await;

        assert!(scheduler.is_running());

        scheduler.stop().await.unwrap();
    }
}
```

### Integration Test with Mock Nostr Client

```rust
#[tokio::test]
async fn test_scheduler_with_nostr_client() {
    use std::sync::Mutex;

    // Mock Nostr client that records posted messages
    struct MockNostrClient {
        posted_messages: Mutex<Vec<String>>,
    }

    impl MockNostrClient {
        fn new() -> Self {
            Self { posted_messages: Mutex::new(vec![]) }
        }

        async fn publish_text_note(&self, content: &str) -> Result<(), &'static str> {
            self.posted_messages.lock().unwrap().push(content.to_string());
            Ok(())
        }
    }

    let client = Arc::new(MockNostrClient::new());
    let client_clone = Arc::clone(&client);

    let config = SchedulerConfig {
        cron_expression: "* * * * * *".to_string(),
        timezone: "UTC".to_string(),
    };

    let mut scheduler = Scheduler::new(config).await.unwrap();

    scheduler.register_posting_job(Arc::new(move || {
        let c = Arc::clone(&client_clone);
        Box::pin(async move {
            let _ = c.publish_text_note("Test message").await;
        })
    })).await.unwrap();

    scheduler.start().await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    scheduler.stop().await.unwrap();

    let messages = client.posted_messages.lock().unwrap();
    assert!(!messages.is_empty());
}
```

---

## 7. Rollback Plan

### How to Revert
1. Remove the `src/scheduler/` directory
2. Remove `mod scheduler;` from `main.rs`
3. Remove `tokio-cron-scheduler` and `chrono-tz` from `Cargo.toml`
4. Run `cargo build` to verify clean state

### No Data Migrations Required
This is a pure code addition with no persistent state.

---

## 8. Estimated Effort

| Component | Time Estimate | Complexity |
|-----------|---------------|------------|
| Error types | 15 min | Low |
| Scheduler struct | 30 min | Low |
| Job registration with callback | 45 min | Medium |
| Start/stop lifecycle | 30 min | Low |
| Signal handling integration | 45 min | Medium |
| Unit tests | 1 hour | Medium |
| Integration with main.rs | 30 min | Low |
| **Total** | **~4 hours** | **Medium** |

---

## 9. Key Async Rust Patterns Summary

### Pattern 1: Arc for Shared State
```rust
// Share Nostr client between main thread and scheduler job
let client = Arc::new(NostrClient::new().await?);
let client_for_job = Arc::clone(&client);
```

### Pattern 2: Box::pin for Async Closures
```rust
// tokio-cron-scheduler requires Pin<Box<dyn Future<Output = ()> + Send>>
Arc::new(move || {
    let client = Arc::clone(&client_clone);
    Box::pin(async move {
        client.publish_text_note("Hello").await;
    })
})
```

### Pattern 3: tokio::select! for Racing Futures
```rust
tokio::select! {
    _ = some_long_running_task() => { /* task completed */ }
    _ = shutdown_signal() => { /* shutdown requested */ }
}
```

### Pattern 4: Catching Panics in Async Code
```rust
use std::panic::AssertUnwindSafe;
use futures::FutureExt;

let result = AssertUnwindSafe(async_task())
    .catch_unwind()
    .await;
```

### Pattern 5: Atomic Operations for Simple Shared State
```rust
use std::sync::atomic::{AtomicUsize, Ordering};

let counter = Arc::new(AtomicUsize::new(0));
let idx = counter.fetch_add(1, Ordering::SeqCst);
```

---

## 10. Cron Expression Reference

The `tokio-cron-scheduler` uses 7-field cron expressions:

```
┌───────────── second (0-59)
│ ┌───────────── minute (0-59)
│ │ ┌───────────── hour (0-23)
│ │ │ ┌───────────── day of month (1-31)
│ │ │ │ ┌───────────── month (1-12 or Jan-Dec)
│ │ │ │ │ ┌───────────── day of week (0-6 or Sun-Sat)
│ │ │ │ │ │ ┌───────────── year (optional)
│ │ │ │ │ │ │
* * * * * * *
```

**Examples**:
- `0 0 9 * * *` - Every day at 9:00 AM
- `0 0 */6 * * *` - Every 6 hours
- `0 30 8 * * Mon-Fri` - 8:30 AM on weekdays
- `0 0 12 1 * *` - Noon on the 1st of every month

---

## 11. Next Steps After Implementation

1. **Add job status monitoring** - Track success/failure counts
2. **Add retry logic** - Retry failed postings with exponential backoff
3. **Add webhook notifications** - Notify on job failures
4. **Add job persistence** - Resume jobs after restart (using tokio-cron-scheduler's persistence features)
5. **Add multiple job types** - Different schedules for different message types
```

