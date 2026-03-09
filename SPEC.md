# Nostr Daily Bot - Technical Specification

## Overview

A multi-user Rust application that posts scheduled messages to Nostr relays. Features a web UI for configuration and CLI for quick actions. Users are identified by their npub (public key) and authenticated by providing their nsec (private key) which is never stored.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    nostr-daily-bot                          │
├─────────────────────────────────────────────────────────────┤
│  CLI (clap)           │  Web UI (Axum + embedded HTML/JS)   │
│  - serve              │  - Session management (nsec login)  │
│  - status             │  - Per-user quotes management       │
│  - list-quotes        │  - Schedule editing                 │
│                       │  - Post Now + Post History          │
├─────────────────────────────────────────────────────────────┤
│                      REST API                               │
│  POST /api/session/start     GET /api/users/{npub}/status   │
│  POST /api/session/stop      GET /api/users/{npub}/quotes   │
│  POST /api/quotes            GET /api/users/{npub}/history  │
│  PUT  /api/schedule          POST /api/post                 │
├─────────────────────────────────────────────────────────────┤
│  Scheduler (tokio-cron)  │  Nostr Client (nostr-sdk)        │
│  - Per-user cron jobs    │  - Per-user relay connections    │
│  - Graceful shutdown     │  - Event publishing              │
├─────────────────────────────────────────────────────────────┤
│                    SQLite Database                          │
│  ~/.local/share/nostr-daily-bot/nostr_daily_bot.db          │
│  Tables: users, quotes, post_history                        │
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
├── SPEC.md
├── migrations/
│   └── 001_initial.sql      # Database schema
├── static/
│   └── index.html           # Embedded web UI
└── src/
    ├── main.rs              # Entry point, CLI dispatch
    ├── cli.rs               # CLI command definitions
    ├── auth.rs              # nsec parsing, token generation
    ├── models.rs            # User, Quote, PostHistory structs
    ├── state.rs             # Multi-user application state
    ├── web.rs               # Static file serving
    ├── api/
    │   ├── mod.rs
    │   ├── routes.rs        # API route definitions
    │   └── handlers.rs      # Request handlers
    ├── db/
    │   ├── mod.rs           # Database module
    │   ├── pool.rs          # Connection pool, migrations
    │   ├── users.rs         # User CRUD operations
    │   ├── quotes.rs        # Quote CRUD operations
    │   └── history.rs       # Post history operations
    ├── nostr/
    │   ├── mod.rs           # NostrClient wrapper
    │   └── error.rs         # NostrError enum
    ├── scheduler/
    │   ├── mod.rs           # Scheduler wrapper
    │   └── error.rs         # SchedulerError enum
    └── observability/
        └── mod.rs           # Logging initialization
```

## Database Schema

```sql
-- Users identified by npub
CREATE TABLE users (
    npub TEXT PRIMARY KEY,
    display_name TEXT,
    cron TEXT NOT NULL DEFAULT '0 0 9 * * *',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    created_at TEXT,
    updated_at TEXT
);

-- Quotes per user
CREATE TABLE quotes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    content TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT
);

-- Post history per user
CREATE TABLE post_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    content TEXT NOT NULL,
    event_id TEXT,
    relay_count INTEGER NOT NULL DEFAULT 0,
    is_scheduled INTEGER NOT NULL DEFAULT 1,
    posted_at TEXT
);
```

## Key Components

### State Management

```rust
pub struct AppState {
    pub db: SqlitePool,                              // Database connection
    pub sessions: RwLock<HashMap<String, ActiveSession>>,  // npub -> session
    pub schedulers: RwLock<HashMap<String, Scheduler>>,    // npub -> scheduler
    pub port: u16,
}

pub struct ActiveSession {
    pub npub: String,
    pub token: String,                // Session token for auth
    pub nostr_client: Arc<NostrClient>,
    pub started_at: DateTime<Utc>,
}
```

### Authentication Flow

1. User enters nsec in web UI
2. Server derives npub from nsec (cryptographic proof of ownership)
3. Server generates session token (UUID)
4. Token stored in browser localStorage
5. All mutations require token in request body
6. nsec is **never stored** - only held in memory for Nostr client

### Startup Behavior

1. Server starts, initializes SQLite database
2. Runs migrations if needed
3. Web UI available immediately (no login required to view)
4. User enters nsec to start session
5. Per-user scheduler starts when session begins

### Security

| Aspect | Implementation |
|--------|----------------|
| Identity | Users identified by npub (public key) |
| Authentication | nsec proves ownership of npub |
| Session | UUID token, stored in localStorage |
| Storage | nsec **never** written to disk |
| Database compromise | Only public data exposed (npub, quotes, history) |

**⚠️ Security Note:** User's nsec is sent to the server. Only use on a server you trust.

## API Reference

### Session Management

| Endpoint | Method | Body | Description |
|----------|--------|------|-------------|
| /api/session/start | POST | `{"nsec": "nsec1..."}` | Returns `{npub, token, message}` |
| /api/session/stop | POST | `{"token": "..."}` | Ends session |

### User Data (Public - no auth required)

| Endpoint | Method | Description |
|----------|--------|-------------|
| /api/users/{npub}/status | GET | User status, quote count, schedule |
| /api/users/{npub}/quotes | GET | List user's quotes |
| /api/users/{npub}/schedule | GET | User's cron schedule |
| /api/users/{npub}/history | GET | Recent post history |

### Authenticated Actions (token required)

| Endpoint | Method | Body | Description |
|----------|--------|------|-------------|
| /api/quotes | POST | `{"token": "...", "quotes": [...]}` | Replace all quotes |
| /api/schedule | PUT | `{"token": "...", "cron": "..."}` | Update schedule |
| /api/post | POST | `{"token": "...", "message": "..."}` | Post immediately |

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
| sqlx | Database (SQLite) |
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
docker run -p 3000:3000 -v bot-data:/home/nostr/.local/share/nostr-daily-bot nostr-daily-bot
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

### Database Location

| OS | Path |
|----|------|
| Linux | `~/.local/share/nostr-daily-bot/nostr_daily_bot.db` |
| macOS | `~/Library/Application Support/com.nostr.nostr-daily-bot/nostr_daily_bot.db` |
| Docker | `/home/nostr/.local/share/nostr-daily-bot/nostr_daily_bot.db` |

### Cron Expression Format

6-field format: `sec min hour day_of_month month day_of_week`

Examples:
- `0 0 9 * * *` - Daily at 9:00 AM UTC
- `0 0 */6 * * *` - Every 6 hours
- `0 30 8 * * 1-5` - 8:30 AM on weekdays
