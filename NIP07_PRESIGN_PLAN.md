# NIP-07 Pre-Signing Implementation Plan

## Overview

Add NIP-07 browser extension authentication and pre-signed event scheduling, allowing users to authenticate and schedule posts without ever sending their nsec to the server.

## Goals

1. **NIP-07 Login** — Users authenticate by signing a challenge with their browser extension
2. **Pre-Signed Scheduling** — Users sign upcoming posts locally; server just publishes them
3. **Fallback** — Keep nsec entry for users without browser extensions
4. **7-Day Batches** — Sign one week of posts at a time for manageable UX

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Authentication Modes                            │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Mode 1: NIP-07 Extension (Recommended)                                 │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
│  │  Extension  │───>│  Challenge  │───>│   Session   │                 │
│  │  signs      │    │  verified   │    │   token     │                 │
│  └─────────────┘    └─────────────┘    └─────────────┘                 │
│        │                                      │                         │
│        ▼                                      ▼                         │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
│  │  Pre-sign   │───>│  Signed     │───>│  Scheduler  │                 │
│  │  7 days     │    │  events DB  │    │  publishes  │                 │
│  └─────────────┘    └─────────────┘    └─────────────┘                 │
│                                                                         │
│  Mode 2: nsec Fallback (Current behavior)                               │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
│  │  Enter nsec │───>│  Session +  │───>│  Server     │                 │
│  │  (sent to   │    │  NostrClient│    │  signs &    │                 │
│  │   server)   │    │  in memory  │    │  publishes  │                 │
│  └─────────────┘    └─────────────┘    └─────────────┘                 │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Database Changes

### New Table: `auth_challenges`

```sql
CREATE TABLE auth_challenges (
    id TEXT PRIMARY KEY,              -- UUID challenge ID
    npub TEXT NOT NULL,               -- Claimed public key
    challenge TEXT NOT NULL,          -- Random challenge string
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,         -- 5 minutes from creation
    used INTEGER NOT NULL DEFAULT 0   -- Prevent replay
);

CREATE INDEX idx_challenges_expires ON auth_challenges(expires_at);
```

### New Table: `signed_events`

```sql
CREATE TABLE signed_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL,
    event_json TEXT NOT NULL,         -- Full signed Nostr event
    event_id TEXT NOT NULL,           -- Nostr event ID (for dedup)
    content_preview TEXT NOT NULL,    -- First 100 chars for UI
    scheduled_for TEXT NOT NULL,      -- When to publish
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, posted, failed, cancelled
    posted_at TEXT,                   -- When actually posted
    error_message TEXT,               -- If failed
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_npub) REFERENCES users(npub) ON DELETE CASCADE
);

CREATE INDEX idx_signed_status ON signed_events(user_npub, status, scheduled_for);
CREATE UNIQUE INDEX idx_signed_event_id ON signed_events(event_id);
```

### Modified Table: `users`

```sql
ALTER TABLE users ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'nsec';
-- Values: 'nsec' (server signs) or 'presign' (pre-signed events)
```

## API Changes

### New Endpoints

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/api/auth/challenge` | POST | None | Get challenge to sign |

### Request/Response Examples

**POST /api/auth/challenge**
```json
// Request
{ "npub": "npub1abc123..." }

// Response
{
  "challenge_id": "550e8400-e29b-41d4-a716-446655440000",
  "challenge": "nostr-daily-bot-auth:550e8400:1741700000",
  "expires_in": 300
}
```

**POST /api/auth/verify**
```json
// Request
{
  "challenge_id": "550e8400-e29b-41d4-a716-446655440000",
  "signed_event": {
    "kind": 22242,
    "created_at": 1741700000,
    "content": "nostr-daily-bot-auth:550e8400:1741700000",
    "tags": [["challenge", "550e8400-e29b-41d4-a716-446655440000"]],
    "pubkey": "abc123...",
    "id": "...",
    "sig": "..."
  }
}

// Response
{
  "npub": "npub1abc123...",
  "token": "session-token-uuid",
  "auth_mode": "presign",
  "message": "Authenticated successfully"
}
```

**GET /api/events/pending**
```json
// Response
{
  "events_to_sign": [
    {
      "scheduled_for": "2025-03-12T09:00:00Z",
      "content": "Good morning sunshine!",
      "unsigned_event": {
        "kind": 1,
        "created_at": 1741770000,
        "content": "Good morning sunshine!",
        "tags": [],
        "pubkey": "abc123..."
      }
    },
    // ... more events
  ],
  "total_pending": 4,
  "next_unsigned": "2025-03-14T09:00:00Z"
}
```

**POST /api/events/sign**
```json
// Request
{
  "token": "session-token",
  "signed_events": [
    {
      "scheduled_for": "2025-03-12T09:00:00Z",
      "event": { /* full signed nostr event */ }
    },
    // ... more
  ]
}

// Response
{
  "stored": 4,
  "message": "Signed events stored successfully"
}
```

## Implementation Phases

### Phase 1: Database & Models (1-2 hours)

**Files to create/modify:**

1. **migrations/002_presigning.sql** — New tables
2. **src/db/challenges.rs** — Challenge CRUD
3. **src/db/signed_events.rs** — Signed events CRUD
4. **src/models.rs** — Add `AuthChallenge`, `SignedEvent` structs

**Challenge operations:**
```rust
pub async fn create_challenge(pool: &DbPool, npub: &str) -> Result<AuthChallenge>;
pub async fn verify_challenge(pool: &DbPool, id: &str, npub: &str) -> Result<bool>;
pub async fn mark_challenge_used(pool: &DbPool, id: &str) -> Result<()>;
pub async fn cleanup_expired_challenges(pool: &DbPool) -> Result<i32>;
```

**Signed event operations:**
```rust
pub async fn get_pending_events(pool: &DbPool, npub: &str, limit: i32) -> Result<Vec<SignedEvent>>;
pub async fn store_signed_events(pool: &DbPool, npub: &str, events: Vec<SignedEventInput>) -> Result<i32>;
pub async fn get_next_scheduled(pool: &DbPool, npub: &str) -> Result<Option<SignedEvent>>;
pub async fn mark_posted(pool: &DbPool, id: i64) -> Result<()>;
pub async fn get_event_counts(pool: &DbPool, npub: &str) -> Result<EventCounts>;
```

### Phase 2: NIP-07 Auth Endpoints (2-3 hours)

**Files to modify:**

1. **src/auth.rs** — Add signature verification
2. **src/api/handlers.rs** — Add challenge/verify handlers
3. **src/api/routes.rs** — Add new routes

**Signature verification:**
```rust
use nostr_sdk::{Event, secp256k1};

pub fn verify_signed_event(event: &Event, expected_challenge: &str) -> Result<bool, String> {
    // 1. Verify event signature is valid
    event.verify().map_err(|e| format!("Invalid signature: {}", e))?;

    // 2. Verify challenge matches
    if event.content != expected_challenge {
        return Err("Challenge mismatch".to_string());
    }

    // 3. Verify timestamp is recent (within 5 min)
    let now = chrono::Utc::now().timestamp();
    if (now - event.created_at.as_i64()).abs() > 300 {
        return Err("Event timestamp too old".to_string());
    }

    Ok(true)
}
```

### Phase 3: Pre-Signing Endpoints (2-3 hours)

**Files to modify:**

1. **src/api/handlers.rs** — Add pending/sign/status handlers
2. **src/api/routes.rs** — Add new routes

**Generate unsigned events for signing:**
```rust
pub async fn generate_pending_events(
    pool: &DbPool,
    npub: &str,
    days_ahead: i32,
) -> Result<Vec<UnsignedEvent>> {
    // 1. Get user's quotes and schedule
    let user = users::get_user(pool, npub).await?;
    let quotes = quotes::get_quotes(pool, npub).await?;

    // 2. Calculate next N posting times from cron
    let schedule = cron::Schedule::from_str(&user.cron)?;
    let posting_times: Vec<_> = schedule
        .upcoming(chrono::Utc)
        .take(days_ahead as usize)
        .collect();

    // 3. Get already-signed event times
    let existing = signed_events::get_scheduled_times(pool, npub).await?;

    // 4. Generate unsigned events for missing times
    let mut events = Vec::new();
    for (i, time) in posting_times.iter().enumerate() {
        if !existing.contains(time) {
            let quote_idx = (existing.len() + i) % quotes.len();
            events.push(UnsignedEvent {
                scheduled_for: *time,
                content: quotes[quote_idx].content.clone(),
                kind: 1,
                created_at: time.timestamp(),
                tags: vec![],
                pubkey: npub_to_hex(npub)?,
            });
        }
    }

    Ok(events)
}
```

### Phase 4: Scheduler Updates (1-2 hours)

**Files to modify:**

1. **src/scheduler/mod.rs** — Dual-mode posting
2. **src/api/handlers.rs** — Update `start_scheduler_for_user`

**Dual-mode scheduler:**
```rust
async fn post_for_user(db: &DbPool, npub: &str, nostr_client: Option<Arc<NostrClient>>) {
    let user = users::get_user(db, npub).await.unwrap();

    match user.auth_mode.as_str() {
        "presign" => {
            // Mode 1: Publish pre-signed event
            if let Some(event) = signed_events::get_next_due(db, npub).await.unwrap() {
                let nostr_event: Event = serde_json::from_str(&event.event_json).unwrap();

                // Create temporary client just for publishing (no signing needed)
                let client = Client::default();
                for relay in DEFAULT_RELAYS {
                    client.add_relay(relay).await.ok();
                }
                client.connect().await;

                match client.send_event(nostr_event).await {
                    Ok(id) => {
                        signed_events::mark_posted(db, event.id, &id.to_hex()).await.ok();
                        info!(npub = %npub, event_id = %id, "Posted pre-signed event");
                    }
                    Err(e) => {
                        signed_events::mark_failed(db, event.id, &e.to_string()).await.ok();
                        error!(npub = %npub, error = %e, "Failed to post pre-signed event");
                    }
                }
            }
        }
        "nsec" => {
            // Mode 2: Sign and publish (current behavior)
            if let Some(client) = nostr_client {
                // ... existing logic
            }
        }
        _ => {}
    }
}
```

### Phase 5: Frontend Updates (3-4 hours)

**Files to modify:**

1. **static/index.html** — Complete UI overhaul for dual auth modes

**UI Components to Add:**

```
┌─────────────────────────────────────────────────────────────┐
│  Login                                                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  🔑 Recommended: Browser Extension                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ [Login with Nostr Extension]                        │   │
│  │ nos2x, Alby, or other NIP-07 extension              │   │
│  │ ✅ Your private key never leaves your device         │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ─────────────── or ───────────────                         │
│                                                             │
│  ⚠️ Fallback: Enter nsec                                    │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ [nsec input field]                    [Login]       │   │
│  │ Your nsec is sent to server. Use only if trusted.   │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Scheduled Posts Panel (for presign mode):**

```
┌─────────────────────────────────────────────────────────────┐
│  Scheduled Posts                                            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  📊 Status: 3 signed, 4 pending, 12 posted                  │
│                                                             │
│  📅 Upcoming:                                               │
│  ✅ Mar 11, 9:00 AM - "Good morning!"          [Signed]     │
│  ✅ Mar 12, 9:00 AM - "Stay positive!"         [Signed]     │
│  ✅ Mar 13, 9:00 AM - "You've got this!"       [Signed]     │
│  ⏳ Mar 14, 9:00 AM - "Good morning!"          [Pending]    │
│  ⏳ Mar 15, 9:00 AM - "Stay positive!"         [Pending]    │
│  ⏳ Mar 16, 9:00 AM - "You've got this!"       [Pending]    │
│  ⏳ Mar 17, 9:00 AM - "Good morning!"          [Pending]    │
│                                                             │
│  ⚠️ 4 posts need your signature                             │
│                                                             │
│  [🔏 Sign Pending Posts]                                    │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**JavaScript for NIP-07:**

```javascript
// Detect extension
function hasNostrExtension() {
    return typeof window.nostr !== 'undefined';
}

// Login with extension
async function loginWithExtension() {
    if (!hasNostrExtension()) {
        showMessage('No Nostr extension found. Install nos2x or Alby.', 'error');
        return;
    }

    try {
        // 1. Get public key from extension
        const pubkey = await window.nostr.getPublicKey();
        const npub = hexToNpub(pubkey);

        // 2. Request challenge from server
        const { challenge_id, challenge } = await api('/auth/challenge', {
            method: 'POST',
            body: JSON.stringify({ npub })
        });

        // 3. Sign challenge with extension
        const signedEvent = await window.nostr.signEvent({
            kind: 22242,
            created_at: Math.floor(Date.now() / 1000),
            content: challenge,
            tags: [['challenge', challenge_id]],
            pubkey: pubkey
        });

        // 4. Verify with server
        const { token, auth_mode } = await api('/auth/verify', {
            method: 'POST',
            body: JSON.stringify({ challenge_id, signed_event: signedEvent })
        });

        // 5. Store session
        sessionToken = token;
        userNpub = npub;
        authMode = auth_mode;
        localStorage.setItem('sessionToken', token);
        localStorage.setItem('userNpub', npub);
        localStorage.setItem('authMode', auth_mode);

        showMessage('Logged in successfully!');
        loadStatus();

    } catch (e) {
        showMessage('Login failed: ' + e.message, 'error');
    }
}

// Sign pending events
async function signPendingEvents() {
    if (!hasNostrExtension()) {
        showMessage('Extension required for signing', 'error');
        return;
    }

    // 1. Get pending events from server
    const { events_to_sign } = await api(`/events/pending?token=${sessionToken}`);

    if (events_to_sign.length === 0) {
        showMessage('No events to sign');
        return;
    }

    // 2. Sign each event
    const signedEvents = [];
    for (const item of events_to_sign) {
        try {
            const signed = await window.nostr.signEvent(item.unsigned_event);
            signedEvents.push({
                scheduled_for: item.scheduled_for,
                event: signed
            });
            updateSigningProgress(signedEvents.length, events_to_sign.length);
        } catch (e) {
            showMessage(`Signing cancelled at ${signedEvents.length}/${events_to_sign.length}`, 'error');
            break;
        }
    }

    // 3. Send to server
    if (signedEvents.length > 0) {
        await api('/events/sign', {
            method: 'POST',
            body: JSON.stringify({ token: sessionToken, signed_events: signedEvents })
        });
        showMessage(`Signed ${signedEvents.length} events!`);
        loadEventStatus();
    }
}
```

### Phase 6: Background Scheduler for Pre-signed Events (1-2 hours)

**Key Change:** The scheduler needs to run even without an active session for pre-sign users.

**Current flow:**
```
User starts session → Scheduler created → Posts while session active
```

**New flow for pre-sign:**
```
Server starts → Background task checks all users with pending signed events
             → Posts due events (no session needed)
```

**Implementation:**

```rust
// src/main.rs - Add background task
async fn run_server(port: u16) -> Result<()> {
    // ... existing setup ...

    // Start background scheduler for pre-signed events
    let db_clone = state.db.clone();
    tokio::spawn(async move {
        run_presign_scheduler(db_clone).await;
    });

    // ... rest of server setup ...
}

// src/scheduler/presign.rs
pub async fn run_presign_scheduler(db: DbPool) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        // Find all due signed events
        let due_events = signed_events::get_all_due(&db).await.unwrap_or_default();

        for event in due_events {
            post_presigned_event(&db, event).await;
        }
    }
}
```

## File Summary

### New Files

| File | Purpose |
|------|---------|
| `migrations/002_presigning.sql` | Database schema for challenges + signed events |
| `src/db/challenges.rs` | Challenge CRUD operations |
| `src/db/signed_events.rs` | Signed event CRUD operations |
| `src/scheduler/presign.rs` | Background scheduler for pre-signed posts |

### Modified Files

| File | Changes |
|------|---------|
| `src/db/mod.rs` | Export new modules |
| `src/models.rs` | Add AuthChallenge, SignedEvent, UnsignedEvent |
| `src/auth.rs` | Add signature verification |
| `src/api/handlers.rs` | Add 6 new handlers |
| `src/api/routes.rs` | Add new routes |
| `src/main.rs` | Start background presign scheduler |
| `static/index.html` | Dual login UI, signing panel |

## Testing Plan

### Unit Tests

1. **Challenge generation/verification**
   - Challenge expires after 5 minutes
   - Challenge can only be used once
   - Invalid signature rejected

2. **Signed event storage**
   - Duplicate event IDs rejected
   - Events stored with correct scheduled time
   - Status transitions work correctly

### Integration Tests

1. **Full NIP-07 flow** (manual with browser)
   - Extension detected correctly
   - Challenge signed and verified
   - Session token issued

2. **Pre-signing flow** (manual with browser)
   - Pending events generated correctly
   - Batch signing works
   - Events posted at scheduled time

3. **Fallback flow**
   - nsec login still works
   - Server-side signing still works

## Timeline

| Phase | Effort | Cumulative |
|-------|--------|------------|
| Phase 1: Database & Models | 1-2 hours | 1-2 hours |
| Phase 2: NIP-07 Auth | 2-3 hours | 3-5 hours |
| Phase 3: Pre-Signing Endpoints | 2-3 hours | 5-8 hours |
| Phase 4: Scheduler Updates | 1-2 hours | 6-10 hours |
| Phase 5: Frontend | 3-4 hours | 9-14 hours |
| Phase 6: Background Scheduler | 1-2 hours | 10-16 hours |
| **Total** | | **10-16 hours** |

## Security Considerations

1. **Challenge replay prevention** — Challenges are single-use and expire in 5 minutes
2. **Signature verification** — Use nostr-sdk's built-in verification
3. **Event tampering** — Signed events are stored as-is; any modification invalidates signature
4. **Rate limiting** — Consider adding rate limits on challenge generation
5. **HTTPS required** — All auth should happen over HTTPS (Fly.io provides this)

## Future Enhancements (Out of Scope)

1. **NIP-46 Nostr Connect** — For users who want remote signing without browser
2. **Mobile app** — Native app with secure key storage
3. **Email notifications** — Alert when signed events running low
4. **Webhook notifications** — Notify external service on post success/failure
| `/api/events/cancel` | POST | Token | Cancel pending signed events |

