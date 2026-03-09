//! Nostr Daily Bot - Posts scheduled messages to Nostr relays
//!
//! A learning project for Rust backend development.

mod api;
mod cli;
mod config;
mod nostr;
mod observability;
mod persistence;
mod scheduler;
mod state;
mod web;

use std::sync::Arc;

use anyhow::Result;
use axum::{routing::get, Router};
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;

use crate::cli::{Cli, Commands};
use crate::observability::{init_logging, ObservabilityConfig};
use crate::persistence::{load_quotes, load_schedule};
use crate::state::{AppState, ScheduleState, SharedState};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => run_server(port).await,
        Commands::Status { server } => cli::cmd_status(&server).await,
        Commands::ListQuotes { server } => cli::cmd_list_quotes(&server).await,
    }
}

async fn run_server(port: u16) -> Result<()> {
    // Initialize logging
    let log_config = ObservabilityConfig::from_env();
    init_logging(log_config);

    info!("Nostr Daily Bot v{} starting", env!("CARGO_PKG_VERSION"));

    // Load persisted data
    let quotes = load_quotes().unwrap_or_default();
    let schedule = load_schedule().unwrap_or_default();

    info!(quotes = quotes.len(), cron = %schedule.cron, "Loaded configuration");

    // Create app state
    let state: SharedState = Arc::new(AppState::new(port));
    *state.quotes.write().await = quotes;
    *state.schedule.write().await = ScheduleState {
        cron: schedule.cron,
        next_post: None,
    };

    // Build router
    let app = Router::new()
        .merge(api::create_router(Arc::clone(&state)))
        .fallback(get(web::static_handler));

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    info!(address = %addr, "Server started");
    info!("Web UI available at http://localhost:{}", port);

    // Run with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
    info!("Shutdown signal received");
}
