-- Add MEETING thread type
ALTER TABLE threads DROP CONSTRAINT threads_type_check;
ALTER TABLE threads ADD CONSTRAINT threads_type_check
    CHECK (type IN ('DM', 'GROUP', 'ENGAGEMENT', 'MEETING'));

-- Meetings table
CREATE TABLE meetings (
    id UUID PRIMARY KEY,
    thread_id UUID NOT NULL REFERENCES threads(id),
    topic TEXT NOT NULL,
    organizer_id UUID NOT NULL REFERENCES agents(id),
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('SCHEDULED', 'ACTIVE', 'CLOSED')),
    scheduled_for TIMESTAMPTZ,
    summary TEXT,
    closed_at TIMESTAMPTZ,
    closed_by_id UUID REFERENCES agents(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_meetings_status ON meetings(status);
CREATE INDEX idx_meetings_thread_id ON meetings(thread_id);
CREATE INDEX idx_meetings_scheduled ON meetings(status, scheduled_for)
    WHERE status = 'SCHEDULED';
