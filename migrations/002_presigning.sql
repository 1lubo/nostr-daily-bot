-- Migration: Add NIP-07 authentication and pre-signed events support

-- Auth challenges for NIP-07 login
CREATE TABLE IF NOT EXISTS auth_challenges (
    id TEXT PRIMARY KEY,
    npub TEXT NOT NULL,
    challenge TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_challenges_npub ON auth_challenges(npub);
CREATE INDEX IF NOT EXISTS idx_challenges_expires ON auth_challenges(expires_at);

-- Pre-signed events for scheduled posting
CREATE TABLE IF NOT EXISTS signed_events (
    id BIGSERIAL PRIMARY KEY,
    user_npub TEXT NOT NULL REFERENCES users(npub) ON DELETE CASCADE,
    event_json TEXT NOT NULL,
    event_id TEXT NOT NULL,
    content_preview TEXT NOT NULL,
    scheduled_for TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    posted_at TIMESTAMPTZ,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_signed_status ON signed_events(user_npub, status, scheduled_for);
CREATE UNIQUE INDEX IF NOT EXISTS idx_signed_event_id ON signed_events(event_id);

-- Add auth_mode column to users table (PostgreSQL syntax)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'users' AND column_name = 'auth_mode'
    ) THEN
        ALTER TABLE users ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'nsec';
    END IF;
END $$;

