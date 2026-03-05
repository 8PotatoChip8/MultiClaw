export interface Agent {
    id: string;
    name: string;
    role: 'MAIN' | 'CEO' | 'MANAGER' | 'WORKER';
    company_id: string | null;
    holding_id: string;
    specialty: string | null;
    parent_agent_id: string | null;
    preferred_model: string | null;
    effective_model: string;
    system_prompt: string | null;
    vm_id: string | null;
    sandbox_vm_id: string | null;
    handle: string | null;
    status: string;
    created_at: string;
}

export interface Company {
    id: string;
    holding_id: string;
    name: string;
    type: 'INTERNAL' | 'EXTERNAL';
    description: string | null;
    tags: any;
    status: string;
    created_at: string;
}

export interface OrgNode {
    agent: Agent;
    children: OrgNode[];
}

export interface Thread {
    id: string;
    title: string | null;
    type: 'DM' | 'GROUP' | 'ENGAGEMENT';
    created_at: string;
}

export interface Message {
    id: string;
    thread_id: string;
    sender_type: 'USER' | 'AGENT' | 'SYSTEM';
    sender_id: string;
    content: any;
    created_at: string;
}

export interface Request {
    id: string;
    type: string;
    created_by_agent_id: string | null;
    created_by_user_id: string | null;
    company_id: string | null;
    payload: any;
    status: string;
    current_approver_type: string;
    current_approver_id: string | null;
    created_at: string;
    updated_at: string;
}

export interface ServiceCatalogItem {
    id: string;
    provider_company_id: string;
    name: string;
    description: string | null;
    pricing_model: string;
    rate: any;
    active: boolean;
}

export interface LedgerEntry {
    id: string;
    company_id: string;
    counterparty_company_id: string | null;
    type: 'EXPENSE' | 'REVENUE' | 'INTERNAL_TRANSFER';
    amount: number;
    currency: string;
    memo: string | null;
    is_virtual: boolean;
    created_at: string;
}
