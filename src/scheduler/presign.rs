//! Background scheduler for pre-signed events.
//!
//! This scheduler runs independently of user sessions and posts pre-signed events
//! when their scheduled time arrives.

use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use nostr_sdk::prelude::*;
use sqlx::PgPool;
use tracing::{debug, error, info, warn};

use crate::db::{history, signed_events};

/// Default relays for publishing pre-signed events.
const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.nostr.band",
    "wss://nostr.wine",
    "wss://relay.snort.social",
];

/// Run the background scheduler for pre-signed events.
/// This function runs forever, checking for due events every minute.
pub async fn run_presign_scheduler(db: PgPool) {
    info!("Starting pre-signed events background scheduler");

    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;
        let _ = post_due_events(&db).await;
    }
}

/// Post all due pre-signed events. Returns (posted_count, failed_count).
/// This can be called from the background scheduler or from an external cron webhook.
pub async fn post_due_events(db: &PgPool) -> Result<(i32, i32)> {
    let now = Utc::now();
    debug!(now = %now, "Checking for due pre-signed events");

    let due_events = signed_events::get_all_due(db).await?;

    if due_events.is_empty() {
        debug!(now = %now, "No due events found");
        return Ok((0, 0));
    }

    info!(
        count = due_events.len(),
        now = %now,
        "Found due pre-signed events to post"
    );

    // Log details of each due event
    for event in &due_events {
        info!(
            event_id = event.id,
            scheduled_for = %event.scheduled_for,
            npub = %event.user_npub,
            status = %event.status,
            content_preview = %event.content_preview,
            "Due event details"
        );
    }

    let mut posted = 0;
    let mut failed = 0;

    for event in due_events {
        if post_presigned_event(db, event).await {
            posted += 1;
        } else {
            failed += 1;
        }
    }

    info!(posted = posted, failed = failed, "Finished posting due events");
    Ok((posted, failed))
}

/// Post a single pre-signed event to relays. Returns true if successful.
async fn post_presigned_event(db: &PgPool, event: crate::models::SignedEvent) -> bool {
    let event_id = event.id;
    let user_npub = event.user_npub.clone();

    // Parse the stored event JSON
    let nostr_event: Event = match serde_json::from_str(&event.event_json) {
        Ok(e) => e,
        Err(e) => {
            error!(
                event_id = event_id,
                npub = %user_npub,
                error = %e,
                "Failed to parse stored event JSON"
            );
            let _ = signed_events::mark_failed(db, event_id, &format!("Invalid event JSON: {}", e))
                .await;
            return false;
        }
    };

    // Create a temporary client for publishing (no signing needed)
    let client = Client::default();

    // Add relays
    for relay in DEFAULT_RELAYS {
        if let Err(e) = client.add_relay(*relay).await {
            warn!(relay = %relay, error = %e, "Failed to add relay");
        }
    }

    // Connect to relays
    client.connect().await;

    // Give relays a moment to connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send the pre-signed event
    let success = match client.send_event(nostr_event.clone()).await {
        Ok(output) => {
            let nostr_event_id = output.id().to_hex();
            info!(
                db_event_id = event_id,
                nostr_event_id = %nostr_event_id,
                npub = %user_npub,
                success_count = output.success.len(),
                failed_count = output.failed.len(),
                "Posted pre-signed event"
            );

            // Mark as posted
            if let Err(e) = signed_events::mark_posted(db, event_id).await {
                error!(event_id = event_id, error = %e, "Failed to mark event as posted");
            }

            // Record in history
            let content_preview = if nostr_event.content.len() > 100 {
                format!("{}...", &nostr_event.content[..97])
            } else {
                nostr_event.content.clone()
            };

            if let Err(e) = history::record_post(
                db,
                &user_npub,
                &content_preview,
                Some(&nostr_event_id),
                output.success.len() as i32,
                true, // is_scheduled
            )
            .await
            {
                warn!(error = %e, "Failed to record post in history");
            }
            true
        }
        Err(e) => {
            error!(
                event_id = event_id,
                npub = %user_npub,
                error = %e,
                "Failed to post pre-signed event"
            );
            let _ = signed_events::mark_failed(db, event_id, &e.to_string()).await;
            false
        }
    };

    // Disconnect
    client.disconnect().await;
    success
}
