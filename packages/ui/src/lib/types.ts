export interface Agent {
    id: string;
    name: string;
    role: 'MAIN' | 'CEO' | 'MANAGER' | 'WORKER';
    company_id: string | null;
    status: string;
    effective_model: string;
}

export interface Company {
    id: string;
    name: string;
    type: 'INTERNAL' | 'EXTERNAL';
    status: string;
}

export interface OrgNode {
    agent: Agent;
    children: OrgNode[];
}

export interface Thread {
    id: string;
    title: string;
    type: 'DM' | 'GROUP' | 'ENGAGEMENT';
}

export interface Message {
    id: string;
    sender_type: 'USER' | 'AGENT' | 'SYSTEM';
    sender_id: string;
    content: any;
    created_at: string;
}
