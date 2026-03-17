CREATE TABLE message_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID NOT NULL REFERENCES agents(id),
    priority SMALLINT NOT NULL DEFAULT 5,
    -- Priority levels:
    --   1 = USER message (operator)
    --   2 = Critical notification (hire approval, credential update)
    --   3 = Agent-to-agent DM (initial message), urgent briefing
    --   4 = Post-DM action prompt, recovery prompt
    --   5 = Routine (heartbeat)

    kind TEXT NOT NULL,
    -- Kind identifies the handler: 'thread_reply', 'dm_initiate', 'dm_continue',
    -- 'heartbeat', 'hire_notify', 'action_prompt', 'file_notify', 'approval_escalate', etc.

    payload JSONB NOT NULL,
    -- Opaque JSON blob. Each 'kind' defines its own schema.

    status TEXT NOT NULL DEFAULT 'PENDING'
        CHECK (status IN ('PENDING', 'PROCESSING', 'COMPLETED', 'FAILED', 'CANCELLED')),

    retry_count SMALLINT NOT NULL DEFAULT 0,
    max_retries SMALLINT NOT NULL DEFAULT 3,
    error TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,

    -- For DM conversations: links continuation messages to the parent
    parent_queue_id UUID REFERENCES message_queue(id)
);

-- The worker queries: oldest PENDING row per agent, ordered by priority then time
CREATE INDEX idx_mq_agent_status ON message_queue(agent_id, status, priority, created_at);

-- For monitoring: find stuck/failed items
CREATE INDEX idx_mq_status ON message_queue(status) WHERE status IN ('PROCESSING', 'FAILED');
