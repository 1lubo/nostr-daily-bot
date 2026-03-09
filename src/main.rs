//! Nostr Daily Bot - Posts scheduled messages to Nostr relays
//!
//! A learning project for Rust backend development.

mod config;
mod nostr;
mod observability;
mod scheduler;

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use crate::config::Config;
use crate::nostr::NostrClient;
use crate::observability::{init_logging, spans, ObservabilityConfig};
use crate::scheduler::{run_until_shutdown, Scheduler, SchedulerConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize observability FIRST (before any logging)
    let log_config = ObservabilityConfig::from_env();
    init_logging(log_config);

    // 2. Enter startup span for tracing
    let _startup = spans::startup_span().entered();
    info!("Nostr Daily Bot v{} starting", env!("CARGO_PKG_VERSION"));

    // 3. Load and validate configuration
    let config = Config::load("config.toml").context("Failed to load configuration")?;
    info!(
        relay_count = config.relays.urls.len(),
        template_count = config.messages.templates.len(),
        "Configuration loaded"
    );

    // 4. Initialize Nostr client
    let keys = NostrClient::keys_parse(&config.get_private_key()?)
        .context("Invalid private key format")?;
    let nostr_client = Arc::new(NostrClient::with_keys(keys).await?);

    // 5. Connect to relays
    nostr_client
        .connect()
        .await
        .context("Failed to connect to relays")?;
    info!(
        connected = nostr_client.connected_relay_count().await,
        "Connected to relays"
    );

    // 6. Setup scheduler with posting job
    let scheduler = setup_scheduler(&config, Arc::clone(&nostr_client)).await?;

    // 7. Run until shutdown signal (SIGTERM/SIGINT)
    info!("Bot running. Press Ctrl+C to stop.");
    run_until_shutdown(scheduler).await?;

    // 8. Cleanup
    nostr_client.shutdown().await;
    info!("Shutdown complete");

    Ok(())
}

/// Setup the scheduler with the posting job
async fn setup_scheduler(config: &Config, nostr_client: Arc<NostrClient>) -> Result<Scheduler> {
    let scheduler_config = SchedulerConfig {
        cron_expression: config.schedule.cron.clone(),
        timezone: config.schedule.timezone.clone(),
    };

    let mut scheduler = Scheduler::new(scheduler_config)
        .await
        .context("Failed to create scheduler")?;

    // Clone for the closure
    let client = Arc::clone(&nostr_client);
    let templates = Arc::new(config.messages.templates.clone());
    let template_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Register the posting job
    scheduler
        .register_posting_job(Arc::new(move || {
            let client = Arc::clone(&client);
            let templates = Arc::clone(&templates);
            let index = Arc::clone(&template_index);

            Box::pin(async move {
                let operation_id = spans::generate_operation_id();

                // Get next message (sequential rotation)
                let idx = index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let message = &templates[idx % templates.len()];

                info!(
                    operation_id = %operation_id,
                    message_index = idx % templates.len(),
                    "Posting scheduled message"
                );

                match client.publish_text_note(message).await {
                    Ok(event_id) => {
                        info!(operation_id = %operation_id, %event_id, "Posted successfully");
                    }
                    Err(e) => {
                        tracing::error!(operation_id = %operation_id, error = %e, "Failed to post message");
                    }
                }
            })
        }))
        .await
        .context("Failed to register posting job")?;

    Ok(scheduler)
}

