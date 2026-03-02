CREATE TABLE holdings (
    id UUID PRIMARY KEY,
    owner_user_id UUID,
    name TEXT NOT NULL,
    main_agent_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE tool_policies (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    allowlist JSONB NOT NULL,
    denylist JSONB NOT NULL,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE companies (
    id UUID PRIMARY KEY,
    holding_id UUID NOT NULL REFERENCES holdings(id),
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('INTERNAL','EXTERNAL')),
    description TEXT,
    tags JSONB,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE vms (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    provider_ref TEXT NOT NULL,
    hostname TEXT NOT NULL,
    ip_address TEXT,
    resources JSONB NOT NULL,
    state TEXT NOT NULL,
    last_heartbeat_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_vms_provider_ref ON vms(provider_ref);

CREATE TABLE agents (
    id UUID PRIMARY KEY,
    holding_id UUID NOT NULL REFERENCES holdings(id),
    company_id UUID REFERENCES companies(id),
    role TEXT NOT NULL CHECK (role IN ('MAIN','CEO','MANAGER','WORKER')),
    name TEXT NOT NULL,
    specialty TEXT,
    parent_agent_id UUID REFERENCES agents(id),
    preferred_model TEXT,
    effective_model TEXT NOT NULL,
    system_prompt TEXT,
    tool_policy_id UUID NOT NULL REFERENCES tool_policies(id),
    vm_id UUID REFERENCES vms(id),
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agents_company_role ON agents(company_id, role);

CREATE TABLE company_ceos (
    company_id UUID NOT NULL REFERENCES companies(id),
    agent_id UUID NOT NULL REFERENCES agents(id),
    PRIMARY KEY (company_id, agent_id)
);

CREATE TABLE threads (
    id UUID PRIMARY KEY,
    type TEXT NOT NULL CHECK (type IN ('DM','GROUP','ENGAGEMENT')),
    title TEXT,
    created_by_user_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE thread_members (
    thread_id UUID NOT NULL REFERENCES threads(id),
    member_type TEXT NOT NULL CHECK (member_type IN ('USER','AGENT','COMPANY')),
    member_id UUID NOT NULL,
    permissions JSONB,
    added_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (thread_id, member_type, member_id)
);

CREATE TABLE messages (
    id UUID PRIMARY KEY,
    thread_id UUID NOT NULL REFERENCES threads(id),
    sender_type TEXT NOT NULL CHECK (sender_type IN ('USER','AGENT','SYSTEM')),
    sender_id UUID NOT NULL,
    content JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_messages_thread_time ON messages(thread_id, created_at);

CREATE TABLE dispatches (
    id UUID PRIMARY KEY,
    message_id UUID NOT NULL REFERENCES messages(id),
    target_agent_id UUID NOT NULL REFERENCES agents(id),
    mode TEXT NOT NULL CHECK (mode IN ('PARALLEL','SEQUENTIAL')),
    status TEXT NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE TABLE requests (
    id UUID PRIMARY KEY,
    type TEXT NOT NULL,
    created_by_agent_id UUID REFERENCES agents(id),
    created_by_user_id UUID,
    company_id UUID REFERENCES companies(id),
    payload JSONB NOT NULL,
    status TEXT NOT NULL,
    current_approver_type TEXT NOT NULL,
    current_approver_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_requests_status_approver ON requests(status, current_approver_type);

CREATE TABLE approvals (
    id UUID PRIMARY KEY,
    request_id UUID NOT NULL REFERENCES requests(id),
    approver_type TEXT NOT NULL,
    approver_id UUID NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('APPROVE','REJECT')),
    note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE service_catalog (
    id UUID PRIMARY KEY,
    provider_company_id UUID NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    description TEXT,
    pricing_model TEXT NOT NULL,
    rate JSONB NOT NULL,
    tags JSONB,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE service_engagements (
    id UUID PRIMARY KEY,
    service_id UUID NOT NULL REFERENCES service_catalog(id),
    client_company_id UUID NOT NULL REFERENCES companies(id),
    provider_company_id UUID NOT NULL REFERENCES companies(id),
    scope JSONB NOT NULL,
    status TEXT NOT NULL,
    created_by_agent_id UUID,
    thread_id UUID NOT NULL REFERENCES threads(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE ledger_entries (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    counterparty_company_id UUID REFERENCES companies(id),
    engagement_id UUID REFERENCES service_engagements(id),
    type TEXT NOT NULL CHECK (type IN ('EXPENSE','REVENUE','INTERNAL_TRANSFER')),
    amount NUMERIC NOT NULL,
    currency TEXT NOT NULL,
    memo TEXT,
    is_virtual BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ledger_company_time ON ledger_entries(company_id, created_at);

CREATE TABLE secrets (
    id UUID PRIMARY KEY,
    scope_type TEXT NOT NULL,
    scope_id UUID NOT NULL,
    kind TEXT NOT NULL,
    ciphertext BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
