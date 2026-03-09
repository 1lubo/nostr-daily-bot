# Nostr Daily Bot - Technical Specification

## Overview

A Rust application that posts scheduled messages to Nostr relays. Features a web UI for configuration and CLI for quick actions.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    nostr-daily-bot                          │
├─────────────────────────────────────────────────────────────┤
│  CLI (clap)           │  Web UI (Axum + embedded HTML/JS)   │
│  - serve              │  - Session management (nsec)         │
│  - status             │  - Quotes upload/view                │
│  - list-quotes        │  - Schedule editing                  │
├─────────────────────────────────────────────────────────────┤
│                      REST API                               │
│  POST /api/session/start    PUT  /api/schedule              │
│  POST /api/session/stop     POST /api/quotes/upload         │
│  GET  /api/status           GET  /api/quotes                │
├─────────────────────────────────────────────────────────────┤
│  Scheduler (tokio-cron)  │  Nostr Client (nostr-sdk)        │
│  - Cron-based jobs       │  - Relay connections             │
│  - Graceful shutdown     │  - Event publishing              │
├─────────────────────────────────────────────────────────────┤
│                    Persistence Layer                        │
│  ~/.config/nostr-daily-bot/quotes.json                      │
│  ~/.config/nostr-daily-bot/schedule.json                    │
└─────────────────────────────────────────────────────────────┘
```

## Project Structure

```
nostr-daily-bot/
├── Cargo.toml
├── Cargo.lock
├── Dockerfile
├── docker-compose.yml
├── docker-compose.prod.yml
├── .dockerignore
├── .gitignore
├── .env.example
├── config.toml              # Legacy config (optional)
├── SPEC.md                  # This file
├── static/
│   └── index.html           # Embedded web UI
└── src/
    ├── main.rs              # Entry point, CLI dispatch
    ├── cli.rs               # CLI command definitions
    ├── state.rs             # Shared application state
    ├── persistence.rs       # JSON file storage
    ├── web.rs               # Static file serving
    ├── api/
    │   ├── mod.rs
    │   ├── routes.rs        # API route definitions
    │   └── handlers.rs      # Request handlers
    ├── config/
    │   ├── mod.rs           # ConfigError
    │   ├── types.rs         # Config structs (legacy)
    │   ├── loader.rs        # TOML loading (legacy)
    │   └── validation.rs    # Validation functions
    ├── nostr/
    │   ├── mod.rs           # NostrClient wrapper
    │   └── error.rs         # NostrError enum
    ├── scheduler/
    │   ├── mod.rs           # Scheduler wrapper
    │   └── error.rs         # SchedulerError enum
    └── observability/
        ├── mod.rs           # Logging initialization
        └── spans.rs         # Tracing utilities
```

## Key Components

### State Management

```rust
pub struct AppState {
    pub session: RwLock<Option<ActiveSession>>,  // nsec + NostrClient
    pub quotes: RwLock<Vec<String>>,             // Message templates
    pub schedule: RwLock<ScheduleState>,         // Cron config
    pub scheduler: RwLock<Option<Scheduler>>,    // Active scheduler
    pub port: u16,
}
```

- **Session**: Holds nsec-derived keys and connected NostrClient
- **Quotes**: Loaded from quotes.json, editable via UI
- **Schedule**: Cron expression, loaded from schedule.json
- **Scheduler**: Active when session is running

### Security

- **nsec handling**: Session-only, never persisted to disk
- **Docker**: Runs as non-root user, dropped capabilities
- **No secrets in config files**: Use env vars or enter via UI

### Persistence

| File | Content | Location |
|------|---------|----------|
| quotes.json | `["quote1", "quote2", ...]` | ~/.config/nostr-daily-bot/ |
| schedule.json | `{"cron": "0 0 9 * * *"}` | ~/.config/nostr-daily-bot/ |

## API Reference

| Endpoint | Method | Description |
|----------|--------|-------------|
| /api/session/start | POST | Start session with `{"nsec": "nsec1..."}` |
| /api/session/stop | POST | Stop session, disconnect relays |
| /api/status | GET | Get bot status (active, relays, quotes) |
| /api/quotes | GET | List all quotes |
| /api/quotes/upload | POST | Replace quotes with `{"quotes": [...]}` |
| /api/schedule | GET | Get current cron schedule |
| /api/schedule | PUT | Update schedule with `{"cron": "..."}` |

## CLI Reference

```bash
# Start web server
nostr-daily-bot serve [--port 3000]

# Check status (requires running server)
nostr-daily-bot status [--server http://localhost:3000]

# List quotes (requires running server)
nostr-daily-bot list-quotes [--server http://localhost:3000]
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| tokio | Async runtime |
| nostr-sdk | Nostr protocol |
| axum | Web framework |
| clap | CLI parsing |
| serde/serde_json | Serialization |
| tracing | Structured logging |
| tokio-cron-scheduler | Cron jobs |
| rust-embed | Static file embedding |
| reqwest | HTTP client (CLI) |

## Deployment

### Docker

```bash
docker build -t nostr-daily-bot .
docker run -p 3000:3000 -v bot-data:/home/nostr/.config/nostr-daily-bot nostr-daily-bot
```

### Docker Compose

```bash
docker compose up -d
```

### Binary

```bash
cargo build --release
./target/release/nostr-daily-bot serve --port 3000
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| RUST_LOG | info | Log level (error, warn, info, debug, trace) |
| LOG_FORMAT | pretty | Log format (pretty, json) |

### Cron Expression Format

6-field format: `sec min hour day_of_month month day_of_week`

Examples:
- `0 0 9 * * *` - Daily at 9:00 AM UTC
- `0 0 */6 * * *` - Every 6 hours
- `0 30 8 * * 1-5` - 8:30 AM on weekdays

