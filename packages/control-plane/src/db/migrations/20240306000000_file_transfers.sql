-- Inter-agent file transfer audit log
CREATE TABLE IF NOT EXISTS file_transfers (
    id           UUID PRIMARY KEY,
    sender_id    UUID NOT NULL REFERENCES agents(id),
    receiver_id  UUID NOT NULL REFERENCES agents(id),
    filename     TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL,
    encoding     TEXT NOT NULL DEFAULT 'text',
    dest_path    TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'DELIVERED'
                     CHECK (status IN ('DELIVERED', 'FAILED')),
    error        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_file_transfers_sender   ON file_transfers(sender_id, created_at DESC);
CREATE INDEX idx_file_transfers_receiver ON file_transfers(receiver_id, created_at DESC);
