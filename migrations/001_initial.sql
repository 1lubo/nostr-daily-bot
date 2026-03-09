-- Initial schema for multi-user Nostr Daily Bot
-- Users identified by npub (public key)

-- Users table
CREATE TABLE IF NOT EXISTS users (
    npub TEXT PRIMARY KEY,
    display_name TEXT,
    cron TEXT NOT NULL DEFAULT '0 0 9 * * *',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Quotes table (user's message templates)
CREATE TABLE IF NOT EXISTS quotes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL,
    content TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_npub) REFERENCES users(npub) ON DELETE CASCADE
);

-- Index for faster quote lookups by user
CREATE INDEX IF NOT EXISTS idx_quotes_user ON quotes(user_npub, sort_order);

-- Post history table
CREATE TABLE IF NOT EXISTS post_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_npub TEXT NOT NULL,
    content TEXT NOT NULL,
    event_id TEXT,
    relay_count INTEGER NOT NULL DEFAULT 0,
    is_scheduled INTEGER NOT NULL DEFAULT 1,
    posted_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_npub) REFERENCES users(npub) ON DELETE CASCADE
);

-- Index for post history by user and time
CREATE INDEX IF NOT EXISTS idx_posts_user_time ON post_history(user_npub, posted_at DESC);

