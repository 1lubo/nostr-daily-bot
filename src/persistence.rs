//! Persistence layer for quotes and schedule.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Get the config directory path.
pub fn config_dir() -> Result<PathBuf> {
    let proj_dirs = ProjectDirs::from("com", "nostr", "nostr-daily-bot")
        .context("Could not determine config directory")?;
    
    let config_dir = proj_dirs.config_dir().to_path_buf();
    
    // Create directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)
            .context("Failed to create config directory")?;
        info!(path = %config_dir.display(), "Created config directory");
    }
    
    Ok(config_dir)
}

/// Persisted schedule configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct PersistedSchedule {
    pub cron: String,
}

impl Default for PersistedSchedule {
    fn default() -> Self {
        Self {
            cron: "0 0 9 * * *".to_string(),
        }
    }
}

/// Load quotes from disk.
pub fn load_quotes() -> Result<Vec<String>> {
    let path = config_dir()?.join("quotes.json");
    
    if !path.exists() {
        info!("No quotes file found, using defaults");
        return Ok(default_quotes());
    }
    
    let contents = fs::read_to_string(&path)
        .context("Failed to read quotes.json")?;
    
    let quotes: Vec<String> = serde_json::from_str(&contents)
        .context("Failed to parse quotes.json")?;
    
    info!(count = quotes.len(), "Loaded quotes from disk");
    Ok(quotes)
}

/// Save quotes to disk.
pub fn save_quotes(quotes: &[String]) -> Result<()> {
    let path = config_dir()?.join("quotes.json");
    
    let contents = serde_json::to_string_pretty(quotes)
        .context("Failed to serialize quotes")?;
    
    fs::write(&path, contents)
        .context("Failed to write quotes.json")?;
    
    info!(count = quotes.len(), path = %path.display(), "Saved quotes to disk");
    Ok(())
}

/// Load schedule from disk.
pub fn load_schedule() -> Result<PersistedSchedule> {
    let path = config_dir()?.join("schedule.json");
    
    if !path.exists() {
        info!("No schedule file found, using defaults");
        return Ok(PersistedSchedule::default());
    }
    
    let contents = fs::read_to_string(&path)
        .context("Failed to read schedule.json")?;
    
    let schedule: PersistedSchedule = serde_json::from_str(&contents)
        .context("Failed to parse schedule.json")?;
    
    info!(cron = %schedule.cron, "Loaded schedule from disk");
    Ok(schedule)
}

/// Save schedule to disk.
pub fn save_schedule(schedule: &PersistedSchedule) -> Result<()> {
    let path = config_dir()?.join("schedule.json");
    
    let contents = serde_json::to_string_pretty(schedule)
        .context("Failed to serialize schedule")?;
    
    fs::write(&path, contents)
        .context("Failed to write schedule.json")?;
    
    info!(cron = %schedule.cron, path = %path.display(), "Saved schedule to disk");
    Ok(())
}

/// Default quotes if none are configured.
fn default_quotes() -> Vec<String> {
    vec![
        "🌅 Good morning! Another day to build something meaningful.".to_string(),
        "💡 Daily reminder: Small consistent actions lead to big results.".to_string(),
        "🚀 What are you working on today? Share your progress!".to_string(),
        "📚 Learning something new? The best time to start was yesterday. The second best time is now.".to_string(),
    ]
}

