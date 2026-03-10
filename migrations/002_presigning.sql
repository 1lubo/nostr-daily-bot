-- Migration: Add NIP-07 authentication and pre-signed events support

-- Auth challenges for NIP-07 login
CREATE TABLE IF NOT EXISTS auth_challenges (
    id TEXT PRIMARY KEY,
    npub TEXT NOT NULL,
    challenge TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_challenges_npub ON auth_challenges(npub);
CREATE INDEX IF NOT EXISTS idx_challenges_expires ON auth_challenges(expires_at);

-- Pre-signed events for scheduled posting
CREATE TABLE IF NOT EXISTS signed_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL,
    event_json TEXT NOT NULL,
    event_id TEXT NOT NULL,
    content_preview TEXT NOT NULL,
    scheduled_for TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    posted_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_npub) REFERENCES users(npub) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_signed_status ON signed_events(user_npub, status, scheduled_for);
CREATE UNIQUE INDEX IF NOT EXISTS idx_signed_event_id ON signed_events(event_id);

-- Add auth_mode column to users table
ALTER TABLE users ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'nsec';

