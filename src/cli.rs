//! CLI command definitions and handlers.

use clap::{Parser, Subcommand};

/// Nostr Daily Bot - Posts scheduled messages to Nostr relays
#[derive(Parser)]
#[command(name = "nostr-daily-bot")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the web server and scheduler
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
    /// Show bot status (requires running server)
    Status {
        /// Server URL
        #[arg(short, long, default_value = "http://localhost:3000")]
        server: String,
    },
    /// List configured quotes (requires running server)
    ListQuotes {
        /// Server URL
        #[arg(short, long, default_value = "http://localhost:3000")]
        server: String,
    },
}

/// Execute the status command
pub async fn cmd_status(server: &str) -> anyhow::Result<()> {
    let url = format!("{}/api/status", server);
    
    let response = reqwest::get(&url).await;
    
    match response {
        Ok(resp) if resp.status().is_success() => {
            let status: serde_json::Value = resp.json().await?;
            
            let active = status["active"].as_bool().unwrap_or(false);
            
            if active {
                println!("Status: \x1b[32mActive\x1b[0m");
                if let Some(started) = status["session_started_at"].as_str() {
                    println!("Session: Running since {}", started);
                }
                println!("Relays: {} connected", status["relay_count"]);
            } else {
                println!("Status: \x1b[33mInactive\x1b[0m");
                println!("Session: Not started (enter nsec via web UI)");
            }
            
            println!("Schedule: {} (UTC)", status["cron"]);
            
            if let Some(next) = status["next_post"].as_str() {
                println!("Next post: {}", next);
            }
            
            println!("Quotes: {} loaded", status["quote_count"]);
            println!("Server: {}", status["server_url"]);
        }
        Ok(resp) => {
            eprintln!("Error: Server returned {}", resp.status());
        }
        Err(_) => {
            eprintln!("Error: Could not connect to server at {}", server);
            eprintln!("Make sure the bot is running: nostr-daily-bot serve");
        }
    }
    
    Ok(())
}

/// Execute the list-quotes command
pub async fn cmd_list_quotes(server: &str) -> anyhow::Result<()> {
    let url = format!("{}/api/quotes", server);
    
    let response = reqwest::get(&url).await;
    
    match response {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().await?;
            
            if let Some(quotes) = data["quotes"].as_array() {
                if quotes.is_empty() {
                    println!("No quotes configured.");
                } else {
                    println!("Quotes ({}):", quotes.len());
                    for (i, quote) in quotes.iter().enumerate() {
                        if let Some(q) = quote.as_str() {
                            // Truncate long quotes for display
                            let display = if q.len() > 60 {
                                format!("{}...", &q[..57])
                            } else {
                                q.to_string()
                            };
                            println!("  {}. {}", i + 1, display);
                        }
                    }
                }
            }
        }
        Ok(resp) => {
            eprintln!("Error: Server returned {}", resp.status());
        }
        Err(_) => {
            eprintln!("Error: Could not connect to server at {}", server);
            eprintln!("Make sure the bot is running: nostr-daily-bot serve");
        }
    }
    
    Ok(())
}

