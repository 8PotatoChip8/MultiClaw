-- Trading Orders — journal of executed exchange orders (exchange-agnostic)
CREATE TABLE trading_orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id),
    agent_id UUID NOT NULL REFERENCES agents(id),
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    order_type TEXT NOT NULL CHECK (order_type IN ('MARKET', 'LIMIT')),
    quantity NUMERIC NOT NULL,
    price NUMERIC,
    quote_currency TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'FILLED', 'PARTIAL', 'CANCELLED', 'FAILED')),
    exchange_order_id TEXT,
    fill_price NUMERIC,
    fill_quantity NUMERIC,
    fee NUMERIC,
    fee_currency TEXT,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    filled_at TIMESTAMPTZ
);
CREATE INDEX idx_orders_company ON trading_orders(company_id, created_at DESC);
CREATE INDEX idx_orders_status ON trading_orders(company_id, status);
CREATE INDEX idx_orders_symbol ON trading_orders(company_id, symbol);

-- Company Budgets — spending guardrails per currency
CREATE TABLE company_budgets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id),
    currency TEXT NOT NULL,
    total_budget NUMERIC NOT NULL DEFAULT 0,
    spent_amount NUMERIC NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (company_id, currency)
);
CREATE INDEX idx_budgets_company ON company_budgets(company_id);

-- Secret Access Audit Log
CREATE TABLE secret_access_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    secret_id UUID NOT NULL REFERENCES secrets(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id),
    secret_name TEXT NOT NULL,
    action TEXT NOT NULL DEFAULT 'READ',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_secret_audit_secret ON secret_access_log(secret_id, created_at DESC);
CREATE INDEX idx_secret_audit_agent ON secret_access_log(agent_id, created_at DESC);

-- Link ledger entries to trading orders
ALTER TABLE ledger_entries ADD COLUMN order_id UUID REFERENCES trading_orders(id);
