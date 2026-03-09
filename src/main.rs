//! Nostr Daily Bot - Posts scheduled messages to Nostr relays
//!
//! A learning project for Rust backend development.

mod api;
mod auth;
mod cli;
mod config;
mod db;
mod models;
mod nostr;
mod observability;
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
use crate::db::init_db;
use crate::observability::{init_logging, ObservabilityConfig};
use crate::state::{AppState, SharedState};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => run_server(port).await,
        Commands::Status { server, npub } => cli::cmd_status(&server, &npub).await,
        Commands::ListQuotes { server, npub } => cli::cmd_list_quotes(&server, &npub).await,
    }
}

async fn run_server(port: u16) -> Result<()> {
    // Initialize logging
    let log_config = ObservabilityConfig::from_env();
    init_logging(log_config);

    info!("Nostr Daily Bot v{} starting", env!("CARGO_PKG_VERSION"));

    // Initialize database
    let db = init_db().await?;
    info!("Database initialized");

    // Create app state
    let state: SharedState = Arc::new(AppState::new(db, port));

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
