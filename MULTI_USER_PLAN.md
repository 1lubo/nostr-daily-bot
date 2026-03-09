# Multi-User Support Implementation Plan

## Overview

Add multi-user support to Nostr Daily Bot using SQLite with sqlx. Transform from single-user in-memory state to a multi-user system where each user is identified by their npub (Nostr public key) and authenticated by providing their nsec (session-only, never stored).

### Goals
- Support multiple concurrent users, each with their own quotes and schedules
- Persist user data in SQLite (quotes, schedules, post history)
- Maintain nsec as session-only (never stored in database)
- Preserve single binary deployment
- Keep backward compatibility with existing deployments

### Success Criteria
- Users can login with nsec, which derives their npub
- Each user sees only their own quotes, schedules, and history
- Multiple users can have active sessions simultaneously
- Server restarts preserve user data (except active sessions)
- Existing single-user deployments can migrate data

---

## 1. Dependencies & Setup

### New Dependencies (Cargo.toml)
```toml
# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "migrate"] }
```

### Migration Strategy
Use **embedded migrations** via `sqlx::migrate!()` macro for single-binary deployment.

### Database File Location
- Development: `./nostr-daily-bot.db`
- Production: `~/.config/nostr-daily-bot/nostr-daily-bot.db` (same directory as existing JSON files)
- Configurable via `DATABASE_URL` env var or CLI flag

### Estimated Effort: **1-2 hours**

---

## 2. Database Schema

### Directory Structure for Migrations
```
Nostr_Daily_Bot/
└── migrations/
    └── 20240101000000_initial_schema.sql
```

### Schema Design

```sql
-- migrations/20240101000000_initial_schema.sql

-- Users table: stores user profiles, npub is the identifier
CREATE TABLE users (
    npub TEXT PRIMARY KEY NOT NULL,           -- bech32 public key (npub1...)
    display_name TEXT,                         -- optional display name
    default_cron TEXT NOT NULL DEFAULT '0 0 9 * * *',  -- user's schedule
    timezone TEXT NOT NULL DEFAULT 'UTC',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Quotes table: user's message templates with ordering
CREATE TABLE quotes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    content TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,    -- for custom ordering
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(user_npub, content)                -- prevent duplicate quotes per user
);

CREATE INDEX idx_quotes_user ON quotes(user_npub);
CREATE INDEX idx_quotes_order ON quotes(user_npub, sort_order);

-- Post history table: track what was posted and when
CREATE TABLE post_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    content TEXT NOT NULL,
    event_id TEXT,                            -- nostr event ID (note1... or hex)
    relay_count INTEGER NOT NULL DEFAULT 0,   -- how many relays accepted
    posted_at TEXT NOT NULL DEFAULT (datetime('now')),
    is_scheduled BOOLEAN NOT NULL DEFAULT FALSE  -- scheduled vs manual post
);

CREATE INDEX idx_history_user ON post_history(user_npub);
CREATE INDEX idx_history_posted ON post_history(user_npub, posted_at DESC);
```

### Key Design Decisions
1. **npub as primary key**: Natural identifier, avoids surrogate key complexity
2. **Soft delete not needed**: Users can just stop using the service
3. **Quote ordering**: `sort_order` allows drag-and-drop reordering in UI
4. **Post history**: Enables "Recent Posts" feature, debugging, rate limiting

### Estimated Effort: **1 hour**

---

## 3. State Management Changes

### Current State Structure
```rust
// Current: Single-user, in-memory
pub struct AppState {
    pub session: RwLock<Option<ActiveSession>>,
    pub quotes: RwLock<Vec<String>>,
    pub schedule: RwLock<ScheduleState>,
    pub scheduler: RwLock<Option<Scheduler>>,
    pub port: u16,
}
```

### New State Structure
```rust
// New: Multi-user with SQLite pool
use sqlx::SqlitePool;
use std::collections::HashMap;

pub struct AppState {
    /// SQLite connection pool
    pub db: SqlitePool,
    
    /// Active sessions keyed by npub
    /// These are session-only (not persisted)
    pub sessions: RwLock<HashMap<String, ActiveSession>>,
    
    /// Active schedulers keyed by npub  
    /// Each user with an active session has their own scheduler
    pub schedulers: RwLock<HashMap<String, Scheduler>>,
    
    /// Server port
    pub port: u16,
}

pub struct ActiveSession {
    pub npub: String,                      // User's public key
    pub nostr_client: Arc<NostrClient>,
    pub started_at: DateTime<Utc>,
}
```

### How Schedulers Work Per-User
1. When user starts session (provides nsec):
   - Derive npub from nsec
   - Look up user in DB (create if first time)
   - Load their quotes and schedule from DB
   - Create a Scheduler instance for this user
   - Store in `schedulers` HashMap keyed by npub
2. When scheduler fires:
   - Query user's quotes from DB
   - Post next quote
   - Record in post_history
3. When user stops session:
   - Remove scheduler from HashMap
   - Disconnect NostrClient
   - Session data (nsec) is garbage collected

### Estimated Effort: **2-3 hours**

---

## 4. API Changes

### Authentication Pattern
The nsec is provided in request body for session operations. It's used to:
1. Derive the npub (proves ownership)
2. Create the NostrClient (for signing events)
3. Never stored - only held in memory for session duration

### New/Modified Endpoints

| Endpoint | Method | Change | Description |
|----------|--------|--------|-------------|
| `/api/session/start` | POST | **Modified** | Body: `{"nsec": "..."}` → derives npub, creates/updates user, starts scheduler |
| `/api/session/stop` | POST | **Modified** | Body: `{"npub": "..."}` OR uses session cookie/header |
| `/api/status` | GET | **Modified** | Returns status for authenticated user (via npub param or session) |
| `/api/status/:npub` | GET | **New** | Get status for specific user (if they have active session) |
| `/api/quotes` | GET | **Modified** | Requires npub, returns that user's quotes |
| `/api/quotes` | POST | **Modified** | Requires npub, adds/replaces quotes for user |
| `/api/schedule` | GET/PUT | **Modified** | Requires npub, manages user's schedule |
| `/api/post` | POST | **Modified** | Requires active session, posts for that user |
| `/api/history/:npub` | GET | **New** | Get user's post history |

### Authentication Options

**Option A: npub in request body/query (Stateless)**
```rust
#[derive(Deserialize)]
pub struct AuthenticatedRequest<T> {
    pub npub: String,  // Identifies the user
    #[serde(flatten)]
    pub data: T,       // Request-specific data
}
```

**Option B: Session token after login (Stateful)**
- On `/api/session/start`, return a session token
- Token stored in `sessions` HashMap
- Subsequent requests include token in header
- Simpler for UI, more like traditional auth

**Recommended: Option B** - Better UX, UI doesn't need to manage npub

### Key Handler Changes

```rust
// src/api/handlers.rs

#[derive(Deserialize)]
pub struct StartSessionRequest {
    pub nsec: String,
}

#[derive(Serialize)]
pub struct StartSessionResponse {
    pub message: String,
    pub npub: String,           // Return the derived npub
    pub session_token: String,  // For subsequent requests
}

pub async fn start_session(
    State(state): State<SharedState>,
    Json(req): Json<StartSessionRequest>,
) -> ApiResult<StartSessionResponse> {
    // 1. Parse nsec and derive keys
    let keys = NostrClient::keys_parse(&req.nsec)?;
    let npub = keys.public_key().to_bech32()?;

    // 2. Create or get user in database
    let user = db::get_or_create_user(&state.db, &npub).await?;

    // 3. Check if session already exists for this user
    if state.sessions.read().await.contains_key(&npub) {
        return Err(api_error(StatusCode::CONFLICT, "Session already active for this user"));
    }

    // 4. Create NostrClient and connect
    let nostr_client = Arc::new(NostrClient::with_keys(keys).await?);
    nostr_client.connect().await?;

    // 5. Load user's schedule and start scheduler
    let scheduler = start_user_scheduler(&state, &npub, Arc::clone(&nostr_client)).await?;

    // 6. Generate session token
    let session_token = uuid::Uuid::new_v4().to_string();

    // 7. Store session
    let session = ActiveSession {
        npub: npub.clone(),
        nostr_client,
        started_at: Utc::now(),
        token: session_token.clone(),
    };

    state.sessions.write().await.insert(npub.clone(), session);
    state.schedulers.write().await.insert(npub.clone(), scheduler);

    Ok(Json(StartSessionResponse {
        message: "Session started".to_string(),
        npub,
        session_token,
    }))
}
```

### Estimated Effort: **4-5 hours**

---

## 5. Web UI Changes

### Login Flow
1. User enters nsec in input field
2. Click "Start Session"
3. Backend derives npub, returns it + session token
4. UI stores session token (localStorage or sessionStorage)
5. UI displays "Logged in as: npub1abc..."
6. All subsequent API calls include session token

### UI Components to Modify

**Header/Status Area:**
```html
<!-- Show logged-in user's npub (truncated) -->
<div id="userInfo" style="display:none;">
    Logged in as: <code id="userNpub">npub1...</code>
    <button onclick="logout()">Logout</button>
</div>
```

**Session Card Changes:**
- Show npub after successful login
- "Logout" instead of "Stop Session"
- Multiple sessions indicator (optional, for admin view)

**Quotes/Schedule Cards:**
- No changes needed - they auto-scope to logged-in user

**New: Post History Section:**
```html
<div class="card" id="historyCard">
    <h2>Recent Posts</h2>
    <div id="historyList"></div>
</div>
```

### JavaScript Changes

```javascript
// Store session after login
let sessionToken = null;
let userNpub = null;

async function startSession() {
    const nsec = document.getElementById('nsecInput').value.trim();
    if (!nsec) return showMessage('Enter your nsec', 'error');

    const data = await api('/session/start', {
        method: 'POST',
        body: JSON.stringify({ nsec })
    });

    sessionToken = data.session_token;
    userNpub = data.npub;
    localStorage.setItem('session_token', sessionToken);
    localStorage.setItem('user_npub', userNpub);

    updateUIForLoggedIn();
}

// Include token in all API calls
async function api(endpoint, options = {}) {
    const headers = { 'Content-Type': 'application/json' };
    if (sessionToken) {
        headers['X-Session-Token'] = sessionToken;
    }
    // ... rest of api function
}

// On page load, restore session
window.onload = function() {
    sessionToken = localStorage.getItem('session_token');
    userNpub = localStorage.getItem('user_npub');
    if (sessionToken) {
        updateUIForLoggedIn();
        loadStatus();
    }
};
```

### Estimated Effort: **3-4 hours**

---

## 6. Migration Path

### From JSON to SQLite

Create a one-time migration script/function that:
1. Reads existing `quotes.json` and `schedule.json`
2. Creates a "default" user (or prompts for npub)
3. Imports data into SQLite

```rust
// src/migration.rs

pub async fn migrate_from_json(db: &SqlitePool, default_npub: &str) -> Result<()> {
    // Check if migration already done
    let user_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE npub = ?"
    )
    .bind(default_npub)
    .fetch_one(db)
    .await? > 0;

    if user_exists {
        info!("User already exists, skipping JSON migration");
        return Ok(());
    }

    // Load JSON files
    let quotes = persistence::load_quotes().unwrap_or_default();
    let schedule = persistence::load_schedule().unwrap_or_default();

    // Create user
    sqlx::query(
        "INSERT INTO users (npub, default_cron) VALUES (?, ?)"
    )
    .bind(default_npub)
    .bind(&schedule.cron)
    .execute(db)
    .await?;

    // Insert quotes
    for (i, quote) in quotes.iter().enumerate() {
        sqlx::query(
            "INSERT INTO quotes (user_npub, content, sort_order) VALUES (?, ?, ?)"
        )
        .bind(default_npub)
        .bind(quote)
        .bind(i as i32)
        .execute(db)
        .await?;
    }

    info!(quotes = quotes.len(), "Migrated data from JSON files");
    Ok(())
}
```

### Backward Compatibility
1. Keep `persistence.rs` for reading (not writing) during transition
2. Add `--migrate-from-json <npub>` CLI command
3. First-time users get empty state, existing users migrate

### Estimated Effort: **2 hours**

---

## 7. File Structure

### New/Modified Files

```
Nostr_Daily_Bot/
├── Cargo.toml                    # Add sqlx dependency
├── migrations/
│   └── 20240101000000_initial.sql
├── src/
│   ├── main.rs                   # Add DB init
│   ├── state.rs                  # Complete rewrite for multi-user
│   ├── persistence.rs            # Keep for migration, deprecate
│   ├── db/
│   │   ├── mod.rs               # NEW: Database module
│   │   ├── pool.rs              # NEW: Connection pool setup
│   │   ├── users.rs             # NEW: User CRUD operations
│   │   ├── quotes.rs            # NEW: Quotes CRUD operations
│   │   ├── schedules.rs         # NEW: Schedule operations
│   │   └── history.rs           # NEW: Post history operations
│   ├── models.rs                # NEW: Shared data models
│   ├── auth.rs                  # NEW: Session/token management
│   ├── api/
│   │   ├── handlers.rs          # Major changes
│   │   ├── routes.rs            # Add new routes
│   │   └── extractors.rs        # NEW: Auth extractors
│   └── ...
└── static/
    └── index.html               # UI updates
```

### Module Organization

```rust
// src/db/mod.rs
pub mod pool;
pub mod users;
pub mod quotes;
pub mod schedules;
pub mod history;

pub use pool::create_pool;

// src/models.rs
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub npub: String,
    pub display_name: Option<String>,
    pub default_cron: String,
    pub timezone: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Quote {
    pub id: i64,
    pub user_npub: String,
    pub content: String,
    pub sort_order: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct PostHistoryEntry {
    pub id: i64,
    pub user_npub: String,
    pub content: String,
    pub event_id: Option<String>,
    pub relay_count: i32,
    pub posted_at: String,
    pub is_scheduled: bool,
}
```

### Estimated Effort: **2 hours** (file organization)

---

## 8. Implementation Order

### Phase 1: Database Foundation (Day 1)
1. [ ] Add sqlx dependency to Cargo.toml
2. [ ] Create migrations directory and initial schema
3. [ ] Create `src/db/` module with pool setup
4. [ ] Test DB connection and migrations locally

### Phase 2: Data Layer (Day 1-2)
5. [ ] Implement `db/users.rs` - CRUD for users
6. [ ] Implement `db/quotes.rs` - CRUD for quotes
7. [ ] Implement `db/schedules.rs` - schedule operations
8. [ ] Implement `db/history.rs` - post history operations
9. [ ] Create `models.rs` with shared types

### Phase 3: State Refactor (Day 2)
10. [ ] Rewrite `state.rs` for multi-user
11. [ ] Create `auth.rs` with session management
12. [ ] Create API extractors for auth

### Phase 4: API Migration (Day 2-3)
13. [ ] Update `start_session` handler
14. [ ] Update `stop_session` handler
15. [ ] Update `get_status` handler
16. [ ] Update quotes handlers (get/upload)
17. [ ] Update schedule handlers
18. [ ] Update `post_now` handler
19. [ ] Add history endpoint

### Phase 5: UI Updates (Day 3)
20. [ ] Update login flow in UI
21. [ ] Add session token handling
22. [ ] Add npub display
23. [ ] Add post history section
24. [ ] Test multi-user scenarios

### Phase 6: Migration & Polish (Day 3-4)
25. [ ] Implement JSON migration tool
26. [ ] Update documentation/SPEC.md
27. [ ] Add integration tests
28. [ ] Update Dockerfile for SQLite support

---

## 9. Estimated Total Effort

| Phase | Effort |
|-------|--------|
| Database Foundation | 1-2 hours |
| Data Layer | 3-4 hours |
| State Refactor | 2-3 hours |
| API Migration | 4-5 hours |
| UI Updates | 3-4 hours |
| Migration & Polish | 2-3 hours |
| **Total** | **15-21 hours** (~2-3 days) |

### Complexity Assessment: **Medium-High**
- Core refactoring of state management
- Multiple interconnected changes
- Backward compatibility considerations
- Testing across user scenarios

---

## 10. Rollback Plan

### If Issues Occur
1. The JSON persistence code remains intact (just deprecated)
2. Users can fallback to single-user mode by:
   - Not running migrations
   - Using env var `SINGLE_USER_MODE=true`
3. Database file can be deleted to start fresh

### Data Safety
- SQLite file is easily backed up
- Migration is one-way but JSON files preserved
- No destructive changes to existing data

---

## 11. Key Code Snippets

### Database Pool Initialization

```rust
// src/db/pool.rs
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn create_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    // Run embedded migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
```

### Auth Extractor

```rust
// src/api/extractors.rs
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

pub struct AuthenticatedUser {
    pub npub: String,
    pub session: Arc<ActiveSession>,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    SharedState: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = SharedState::from_ref(state);

        // Get token from header
        let token = parts
            .headers
            .get("X-Session-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "Missing session token"))?;

        // Find session by token
        let sessions = state.sessions.read().await;
        let (npub, session) = sessions
            .iter()
            .find(|(_, s)| s.token == token)
            .map(|(k, v)| (k.clone(), Arc::clone(&v)))
            .ok_or((StatusCode::UNAUTHORIZED, "Invalid session token"))?;

        Ok(AuthenticatedUser { npub, session })
    }
}
```

### Updated Main Startup

```rust
// src/main.rs (partial)
async fn run_server(port: u16) -> Result<()> {
    init_logging(ObservabilityConfig::from_env());
    info!("Nostr Daily Bot v{} starting", env!("CARGO_PKG_VERSION"));

    // Initialize database
    let db_path = get_database_path()?;
    let database_url = format!("sqlite:{}", db_path.display());
    let db = db::create_pool(&database_url).await?;
    info!(path = %db_path.display(), "Database initialized");

    // Create app state (no longer loads from JSON)
    let state: SharedState = Arc::new(AppState::new(db, port));

    // Build router
    let app = Router::new()
        .merge(api::create_router(Arc::clone(&state)))
        .fallback(get(web::static_handler));

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!(address = %addr, "Server started");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}
```

---

## 12. Testing Strategy

### Unit Tests
- `db/users.rs`: Create, get, update user
- `db/quotes.rs`: CRUD operations, ordering
- `db/history.rs`: Insert, query with pagination
- `auth.rs`: Token generation, validation

### Integration Tests
- Start session flow (nsec → npub → token)
- Multi-user isolation (user A can't see user B's quotes)
- Scheduler per-user (both users get scheduled posts)
- Session cleanup on stop

### Manual Testing
1. Start server fresh (empty DB)
2. Login with nsec #1 → add quotes → start scheduler
3. In incognito: login with nsec #2 → different quotes
4. Verify both schedulers fire independently
5. Restart server → sessions gone, data persists
6. Re-login → data restored

---

## Summary

This plan transforms Nostr Daily Bot from a single-user application to a multi-user system while:
- Preserving the single-binary deployment model
- Keeping nsec security (never stored)
- Maintaining backward compatibility via migration
- Using SQLite for simple, embedded persistence

The implementation can be done incrementally, with the database layer built first, then state management, then API changes, and finally UI updates.

