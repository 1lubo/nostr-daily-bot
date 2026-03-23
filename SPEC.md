# Nostr Daily Bot - Technical Specification

## Overview

A multi-user Rust application that posts scheduled messages to Nostr relays. Features a web UI for configuration and CLI for quick actions. Users are identified by their npub (public key) and can authenticate via:

1. **NIP-07 Browser Extension** (recommended) - Sign events locally, server only publishes pre-signed events
2. **nsec Entry** (fallback) - Private key sent to server for signing

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    nostr-daily-bot                          │
├─────────────────────────────────────────────────────────────┤
│  CLI (clap)           │  Web UI (Axum + embedded HTML/JS)   │
│  - serve              │  - NIP-07 extension login           │
│  - status             │  - nsec fallback login              │
│  - list-quotes        │  - Pre-sign batch events            │
│                       │  - Per-user quotes management       │
│                       │  - Schedule editing                 │
├─────────────────────────────────────────────────────────────┤
│                      REST API                               │
│  POST /api/auth/challenge    GET /api/users/{npub}/status   │
│  POST /api/auth/verify       GET /api/users/{npub}/quotes   │
│  POST /api/session/start     GET /api/events/pending        │
│  POST /api/session/stop      POST /api/events/sign          │
│  POST /api/quotes            GET /api/cron/post             │
│  PUT  /api/schedule          POST /api/post                 │
├─────────────────────────────────────────────────────────────┤
│  Scheduler (tokio-cron)  │  Nostr Client (nostr-sdk)        │
│  - Per-user cron jobs    │  - Per-user relay connections    │
│  - Background presign    │  - Event publishing              │
│  - External cron webhook │  - Pre-signed event posting      │
├─────────────────────────────────────────────────────────────┤
│                   PostgreSQL Database                       │
│  Tables: users, quotes, post_history,                       │
│          auth_challenges, signed_events                     │
└─────────────────────────────────────────────────────────────┘
```

## Project Structure

```
nostr-daily-bot/
├── Cargo.toml
├── Cargo.lock
├── Dockerfile
├── fly.toml                 # Fly.io deployment config
├── .dockerignore
├── .gitignore
├── SPEC.md
├── .github/workflows/
│   ├── deploy.yml           # Auto-deploy to Fly.io
│   ├── cron-post.yml        # External cron trigger
│   ├── lint.yml             # Rustfmt + Clippy
│   └── test.yml             # Unit tests
├── migrations/
│   ├── 001_initial.sql      # Base schema
│   └── 002_presigning.sql   # NIP-07 + pre-signing tables
├── static/
│   └── index.html           # Embedded web UI
└── src/
    ├── main.rs              # Entry point, CLI dispatch
    ├── cli.rs               # CLI command definitions
    ├── auth.rs              # NIP-07 + nsec auth, token generation
    ├── models.rs            # User, Quote, SignedEvent structs
    ├── state.rs             # Multi-user application state
    ├── web.rs               # Static file serving
    ├── api/
    │   ├── mod.rs
    │   ├── routes.rs        # API route definitions
    │   └── handlers.rs      # Request handlers
    ├── db/
    │   ├── mod.rs           # Database module
    │   ├── pool.rs          # PostgreSQL connection pool
    │   ├── users.rs         # User CRUD operations
    │   ├── quotes.rs        # Quote CRUD operations
    │   ├── history.rs       # Post history operations
    │   ├── challenges.rs    # NIP-07 auth challenges
    │   └── signed_events.rs # Pre-signed events
    ├── nostr/
    │   ├── mod.rs           # NostrClient wrapper
    │   └── error.rs         # NostrError enum
    ├── scheduler/
    │   ├── mod.rs           # Scheduler wrapper
    │   ├── presign.rs       # Background pre-signed event poster
    │   └── error.rs         # SchedulerError enum
    └── observability/
        └── mod.rs           # Logging initialization
```

## Database Schema (PostgreSQL)

```sql
-- Users identified by npub
CREATE TABLE users (
    npub TEXT PRIMARY KEY,
    display_name TEXT,
    cron TEXT NOT NULL DEFAULT '0 0 9 * * *',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    auth_mode TEXT NOT NULL DEFAULT 'nsec',  -- 'nsec' or 'presign'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Quotes per user
CREATE TABLE quotes (
    id BIGSERIAL PRIMARY KEY,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    content TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Post history per user
CREATE TABLE post_history (
    id BIGSERIAL PRIMARY KEY,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    content TEXT NOT NULL,
    event_id TEXT,
    relay_count INTEGER NOT NULL DEFAULT 0,
    is_scheduled BOOLEAN NOT NULL DEFAULT TRUE,
    posted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- NIP-07 auth challenges
CREATE TABLE auth_challenges (
    id TEXT PRIMARY KEY,
    npub TEXT NOT NULL,
    challenge TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT FALSE
);

-- Pre-signed events for scheduled posting
CREATE TABLE signed_events (
    id BIGSERIAL PRIMARY KEY,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    event_json TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    content_preview TEXT NOT NULL,
    scheduled_for TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, posted, failed, cancelled
    posted_at TIMESTAMPTZ,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

## Key Components

### State Management

```rust
pub struct AppState {
    pub db: PgPool,                                        // PostgreSQL connection
    pub sessions: RwLock<HashMap<String, ActiveSession>>,  // npub -> nsec session
    pub presign_sessions: RwLock<HashMap<String, PresignSession>>,  // npub -> NIP-07 session
    pub schedulers: RwLock<HashMap<String, Scheduler>>,    // npub -> scheduler
    pub port: u16,
}

pub struct ActiveSession {      // nsec mode
    pub npub: String,
    pub token: String,
    pub nostr_client: Arc<NostrClient>,
    pub started_at: DateTime<Utc>,
}

pub struct PresignSession {     // NIP-07 mode
    pub npub: String,
    pub token: String,
    pub started_at: DateTime<Utc>,
}
```

### Authentication Flows

#### NIP-07 Browser Extension (Recommended)

1. User clicks "Login with Extension"
2. Extension provides hex pubkey
3. Server creates challenge (kind 22242 event template)
4. User signs challenge with extension
5. Server verifies signature, creates presign session
6. User signs batch of events for upcoming posts
7. Server stores pre-signed events, posts them at scheduled times
8. **Private key never leaves the browser**

#### nsec Fallback

1. User enters nsec in web UI
2. Server derives npub from nsec
3. Server generates session token (UUID)
4. Server holds nsec in memory for signing
5. **⚠️ nsec is sent to server** - only use on trusted servers

### Startup Behavior

1. Server starts, connects to PostgreSQL (via `DATABASE_URL`)
2. Runs migrations if needed
3. Starts background scheduler for pre-signed events
4. Web UI available immediately
5. Per-user scheduler starts when nsec session begins

### Security

| Aspect | NIP-07 Mode | nsec Mode |
|--------|-------------|-----------|
| Private key location | Browser only | Server memory |
| Server trust required | No | Yes |
| Events signed by | Browser extension | Server |
| Database compromise | Safe (no keys) | Safe (no keys stored) |

## API Reference

### NIP-07 Authentication

| Endpoint | Method | Body | Description |
|----------|--------|------|-------------|
| /api/auth/challenge | POST | `{"npub": "npub1..." or hex}` | Returns `{challenge_id, challenge, expires_in}` |
| /api/auth/verify | POST | `{"challenge_id": "...", "signed_event": {...}}` | Returns `{npub, token, auth_mode}` |

### nsec Session Management

| Endpoint | Method | Body | Description |
|----------|--------|------|-------------|
| /api/session/start | POST | `{"nsec": "nsec1..."}` | Returns `{npub, token, message}` |
| /api/session/stop | POST | `{"token": "..."}` | Ends session |

### Pre-signing (presign mode)

| Endpoint | Method | Query/Body | Description |
|----------|--------|------------|-------------|
| /api/events/pending | GET | `?token=...&days_ahead=7` | Get unsigned events to sign |
| /api/events/sign | POST | `{"token": "...", "signed_events": [...]}` | Store signed events |
| /api/events/status | GET | `?token=...` | Get event counts by status |

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
| /api/post | POST | `{"token": "...", "message": "..."}` | Post immediately (nsec mode only) |

### Cron Webhook (for serverless deployments)

| Endpoint | Method | Description |
|----------|--------|-------------|
| /api/cron/post | GET | Post all due pre-signed events (call from external cron) |

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
| sqlx | Database (PostgreSQL) |
| clap | CLI parsing |
| serde/serde_json | Serialization |
| tracing | Structured logging |
| tokio-cron-scheduler | Cron jobs |
| rust-embed | Static file embedding |
| reqwest | HTTP client (CLI) |
| chrono | Date/time handling |

## Deployment

### Fly.io (Recommended)

```bash
# Initial setup
fly launch
fly postgres create --name myapp-db
fly postgres attach myapp-db

# Deploy (or use GitHub Actions)
fly deploy
```

GitHub Actions automatically deploys on push to master.

### Docker

```bash
docker build -t nostr-daily-bot .
docker run -p 3000:3000 -e DATABASE_URL=postgres://... nostr-daily-bot
```

### Binary

```bash
export DATABASE_URL=postgres://user:pass@localhost/nostr_daily_bot
cargo build --release
./target/release/nostr-daily-bot serve --port 3000
```

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| DATABASE_URL | Yes | - | PostgreSQL connection string |
| RUST_LOG | No | info | Log level (error, warn, info, debug, trace) |
| LOG_FORMAT | No | pretty | Log format (pretty, json) |

### Cron Expression Format

6-field format: `sec min hour day_of_month month day_of_week`

Examples:
- `0 0 9 * * *` - Daily at 9:00 AM UTC
- `0 0 */6 * * *` - Every 6 hours
- `0 30 8 * * 1-5` - 8:30 AM on weekdays

## CI/CD

GitHub Actions workflows:

| Workflow | Trigger | Description |
|----------|---------|-------------|
| deploy.yml | Push to master | Auto-deploy to Fly.io |
| cron-post.yml | Every 5 minutes | Trigger posting of due events |
| lint.yml | Push/PR | Rustfmt + Clippy checks |
| test.yml | Push/PR | Unit tests |
