-- Add handle column to agents for Slack-like messaging
ALTER TABLE agents ADD COLUMN IF NOT EXISTS handle TEXT UNIQUE;

-- Add version tracking for auto-updater
CREATE TABLE IF NOT EXISTS system_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert current version
INSERT INTO system_meta (key, value) VALUES ('version', '0.1.0')
ON CONFLICT (key) DO NOTHING;
