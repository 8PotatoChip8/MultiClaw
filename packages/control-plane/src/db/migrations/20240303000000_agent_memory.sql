-- Agent memory system for persistent context
CREATE TABLE IF NOT EXISTS agent_memories (
    id UUID PRIMARY KEY,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK (category IN ('IDENTITY','TASK','CONTEXT','NOTE')),
    key TEXT NOT NULL,
    content TEXT NOT NULL,
    importance INT NOT NULL DEFAULT 5,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(agent_id, category, key)
);

CREATE INDEX IF NOT EXISTS idx_memories_agent ON agent_memories(agent_id, importance DESC);
