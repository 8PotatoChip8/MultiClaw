-- Shared team knowledge base: agents can publish findings that
-- team members (same parent or same company) can see.
CREATE TABLE team_knowledge (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID NOT NULL REFERENCES agents(id),
    company_id UUID REFERENCES companies(id),
    topic TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Workers query by parent_agent_id (their manager), CEOs query by company_id
CREATE INDEX idx_tk_company ON team_knowledge(company_id, created_at DESC);
CREATE INDEX idx_tk_agent ON team_knowledge(agent_id, created_at DESC);
