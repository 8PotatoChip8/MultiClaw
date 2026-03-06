-- Add reply_depth to messages for inter-agent messaging loop prevention.
-- depth 0 = initial message (triggers auto-response), depth 1+ = auto-response (no further triggers).
ALTER TABLE messages ADD COLUMN reply_depth INTEGER NOT NULL DEFAULT 0;
