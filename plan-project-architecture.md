# Implementation Plan: Project Architecture

## 1. Overview

### Description
Define the foundational architecture for the Nostr Daily Bot: project structure, dependencies, error handling strategy, module relationships, and application flow.

### Goals
- ✅ Clean, idiomatic Rust project structure
- ✅ Explicit dependency versions in Cargo.toml
- ✅ Two-tier error handling: `thiserror` for modules, `anyhow` for application
- ✅ Clear module dependency graph
- ✅ Graceful startup and shutdown flow in main.rs

---

## 2. Project Directory Structure

```
nostr-daily-bot/
├── Cargo.toml
├── Cargo.lock
├── config.toml              # Runtime configuration
├── .env.example             # Environment variable template
├── .gitignore
├── README.md
├── Dockerfile
├── docker-compose.yml
└── src/
    ├── main.rs              # Entry point, orchestration
    ├── lib.rs               # Library root (optional, for testing)
    ├── config/
    │   ├── mod.rs           # Re-exports, ConfigError
    │   ├── types.rs         # Config structs
    │   ├── loader.rs        # File loading, env overrides
    │   └── validation.rs    # Validation functions
    ├── nostr/
    │   ├── mod.rs           # Re-exports, NostrClient
    │   └── error.rs         # NostrError
    ├── scheduler/
    │   ├── mod.rs           # Scheduler, run_until_shutdown
    │   └── error.rs         # SchedulerError
    └── observability/
        ├── mod.rs           # init_logging, ObservabilityConfig
        └── spans.rs         # Span utilities, operation IDs
```

---

## 3. Cargo.toml Dependencies

```toml
[package]
name = "nostr-daily-bot"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
authors = ["1Lubo <1lubo@pm.me>"]
description = "A bot that posts daily messages to Nostr relays"
license = "MIT"

[dependencies]
# Async runtime
tokio = { version = "1.43", features = ["rt-multi-thread", "macros", "signal"] }

# Nostr protocol
nostr-sdk = "0.39"

# Scheduling
tokio-cron-scheduler = "0.13"
chrono-tz = "0.10"

# Configuration
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Logging/tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }

# Utilities
uuid = { version = "1.11", features = ["v4"] }
cron = "0.13"

[dev-dependencies]
tempfile = "3.14"
tokio-test = "0.4"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

---

## 4. Error Handling Strategy

### Two-Tier Approach

| Layer | Crate | Purpose |
|-------|-------|---------|
| **Module errors** | `thiserror` | Typed, specific errors per module |
| **Application errors** | `anyhow` | Aggregation in main.rs, context addition |

### Module Error Pattern

```rust
// src/config/mod.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    FileRead { path: String, #[source] source: std::io::Error },

    #[error("Invalid cron expression '{expr}': {reason}")]
    InvalidCron { expr: String, reason: String },
    // ...
}
```

### Application Error Pattern

```rust
// src/main.rs
use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load("config.toml")
        .context("Failed to load configuration")?;
    // ...
}
```

---

## 5. Module Dependency Graph

```
                    ┌─────────────┐
                    │   main.rs   │

## 6. main.rs Flow

### Initialization Order

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize observability FIRST (before any logging)
    let log_config = ObservabilityConfig::from_env();
    init_logging(log_config);

    // 2. Enter startup span for tracing
    let _startup = spans::startup_span().entered();
    info!("Nostr Daily Bot starting");

    // 3. Load and validate configuration
    let config = Config::load("config.toml")
        .context("Failed to load configuration")?;

    // 4. Initialize Nostr client
    let keys = NostrClient::keys_parse(&config.get_private_key()?)
        .context("Invalid private key")?;
    let nostr_client = Arc::new(NostrClient::with_keys(keys).await?);

    // 5. Connect to relays
    nostr_client.connect().await
        .context("Failed to connect to relays")?;

    // 6. Setup scheduler with posting job
    let scheduler = setup_scheduler(&config, Arc::clone(&nostr_client)).await?;

    // 7. Run until shutdown signal (SIGTERM/SIGINT)
    run_until_shutdown(scheduler).await?;

    // 8. Cleanup
    nostr_client.shutdown().await;
    info!("Shutdown complete");

    Ok(())
}
```

### Graceful Shutdown Flow

```
SIGTERM/SIGINT received
        │
        ▼
┌───────────────────┐
│ Stop scheduler    │  ← Stops accepting new jobs
│ (wait for current)│  ← Waits for running job to complete
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│ Disconnect relays │  ← Clean WebSocket close
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│ Flush logs        │  ← Ensure all logs written
└─────────┬─────────┘
          │
          ▼
      Exit(0)
```

---

## 7. File Changes Summary

### Files to Create

| File | Purpose |
|------|---------|
| `Cargo.toml` | Project manifest with dependencies |
| `src/main.rs` | Application entry point |
| `src/lib.rs` | Library root (optional) |
| `.gitignore` | Git ignore patterns |
| `.env.example` | Environment variable template |
| `config.toml` | Example configuration |

---

## 8. .gitignore Content

```gitignore
# Build artifacts
/target/
Cargo.lock

# IDE
.idea/
.vscode/
*.swp
*.swo

# Environment
.env
*.pem
*.key

# OS
.DS_Store
Thumbs.db

# Logs
*.log
```

---

## 9. Recommended Implementation Order

1. **Project Setup** → Cargo.toml, directory structure, .gitignore
2. **Observability** → Logging first (needed for debugging everything else)
3. **Configuration** → Load settings before other modules
4. **Nostr Client** → Core functionality
5. **Scheduler** → Ties everything together
6. **main.rs** → Orchestration
7. **Containerization** → Dockerfile after code works

---

## 10. Idiomatic Rust Patterns Used

| Pattern | Where | Why |
|---------|-------|-----|
| `Arc<T>` | Sharing NostrClient | Thread-safe shared ownership for async |
| `thiserror` | Module errors | Derive Error trait idiomatically |
| `anyhow` | main.rs | Easy error context and propagation |
| `?` operator | Everywhere | Clean error propagation |
| `#[instrument]` | Async functions | Automatic span creation |
| Module re-exports | `mod.rs` files | Clean public API |
| Builder pattern | NostrClient | Flexible configuration |

---

## 11. Estimated Effort

| Task | Time |
|------|------|
| Cargo.toml + structure | 30 min |
| .gitignore, .env.example | 10 min |
| Skeleton main.rs | 20 min |
| **Total** | ~1 hour |
