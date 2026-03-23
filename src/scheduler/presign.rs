//! Background scheduler for pre-signed events.
//!
//! This scheduler runs independently of user sessions and posts pre-signed events
//! when their scheduled time arrives.

use std::time::Duration;

use nostr_sdk::prelude::*;
use sqlx::PgPool;
use tracing::{error, info, warn};

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

        // Find all due signed events
        match signed_events::get_all_due(&db).await {
            Ok(due_events) => {
                if !due_events.is_empty() {
                    info!(count = due_events.len(), "Found due pre-signed events");
                }

                for event in due_events {
                    post_presigned_event(&db, event).await;
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to fetch due events");
            }
        }
    }
}

/// Post a single pre-signed event to relays.
async fn post_presigned_event(db: &PgPool, event: crate::models::SignedEvent) {
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
            let _ = signed_events::mark_failed(db, event_id, &format!("Invalid event JSON: {}", e)).await;
            return;
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
    match client.send_event(nostr_event.clone()).await {
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
            ).await {
                warn!(error = %e, "Failed to record post in history");
            }
        }
        Err(e) => {
            error!(
                event_id = event_id,
                npub = %user_npub,
                error = %e,
                "Failed to post pre-signed event"
            );
            let _ = signed_events::mark_failed(db, event_id, &e.to_string()).await;
        }
    }

    // Disconnect
    client.disconnect().await;
}

