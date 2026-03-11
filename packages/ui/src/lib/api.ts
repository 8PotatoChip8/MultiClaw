function getApiUrl() {
    // If env var is set and non-empty, use it
    if (process.env.NEXT_PUBLIC_API_URL) return process.env.NEXT_PUBLIC_API_URL;
    // Otherwise derive from the current browser location
    if (typeof window !== 'undefined') {
        return `${window.location.protocol}//${window.location.hostname}:8080/v1`;
    }
    return 'http://localhost:8080/v1';
}

const API_URL = typeof window !== 'undefined' ? getApiUrl() : 'http://localhost:8080/v1';

function getHeaders() {
    const token = typeof window !== 'undefined' ? localStorage.getItem('admin_token') : '';
    return {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json'
    };
}

async function request(path: string, options?: RequestInit) {
    const res = await fetch(`${API_URL}${path}`, {
        ...options,
        headers: { ...getHeaders(), ...(options?.headers || {}) }
    });
    return res.json();
}

export const api = {
    // Health
    health: () => request('/health'),

    // Companies
    getCompanies: () => request('/companies'),
    getCompany: (id: string) => request(`/companies/${id}`),
    createCompany: (data: { name: string; type: string; description?: string }) =>
        request('/companies', { method: 'POST', body: JSON.stringify(data) }),
    getOrgTree: (companyId: string) => request(`/companies/${companyId}/org-tree`),
    hireCeo: (companyId: string, data: { name: string; specialty?: string; preferred_model?: string }) =>
        request(`/companies/${companyId}/hire-ceo`, { method: 'POST', body: JSON.stringify(data) }),
    getLedger: (companyId: string) => request(`/companies/${companyId}/ledger`),
    createLedgerEntry: (companyId: string, data: { type: string; amount: number; currency: string; memo?: string; counterparty_company_id?: string }) =>
        request(`/companies/${companyId}/ledger`, { method: 'POST', body: JSON.stringify(data) }),
    getBalance: (companyId: string) => request(`/companies/${companyId}/balance`),

    // Agents
    getAgents: () => request('/agents'),
    getAgent: (id: string) => request(`/agents/${id}`),
    patchAgent: (id: string, data: { preferred_model?: string; specialty?: string; system_prompt?: string }) =>
        request(`/agents/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
    hireManager: (ceoId: string, data: { name: string; specialty?: string; preferred_model?: string }) =>
        request(`/agents/${ceoId}/hire-manager`, { method: 'POST', body: JSON.stringify(data) }),
    hireWorker: (mgrId: string, data: { name: string; specialty?: string; preferred_model?: string }) =>
        request(`/agents/${mgrId}/hire-worker`, { method: 'POST', body: JSON.stringify(data) }),
    vmStart: (id: string, target?: string) => request(`/agents/${id}/vm/start${target ? `?target=${target}` : ''}`, { method: 'POST' }),
    vmStop: (id: string, target?: string) => request(`/agents/${id}/vm/stop${target ? `?target=${target}` : ''}`, { method: 'POST' }),
    vmRebuild: (id: string, target?: string) => request(`/agents/${id}/vm/rebuild${target ? `?target=${target}` : ''}`, { method: 'POST' }),
    vmExec: (id: string, data: { command: string; user?: string; working_dir?: string; timeout_secs?: number }, target?: string) =>
        request(`/agents/${id}/vm/exec${target ? `?target=${target}` : ''}`, { method: 'POST', body: JSON.stringify(data) }),
    vmInfo: (id: string, target?: string) => request(`/agents/${id}/vm/info${target ? `?target=${target}` : ''}`),
    vmFilePush: (id: string, data: { path: string; content: string; encoding?: string }, target?: string) =>
        request(`/agents/${id}/vm/file/push${target ? `?target=${target}` : ''}`, { method: 'POST', body: JSON.stringify(data) }),
    vmFilePull: (id: string, data: { path: string }, target?: string) =>
        request(`/agents/${id}/vm/file/pull${target ? `?target=${target}` : ''}`, { method: 'POST', body: JSON.stringify(data) }),
    vmProvision: (id: string) => request(`/agents/${id}/vm/provision`, { method: 'POST' }),
    vmSandboxProvision: (id: string) => request(`/agents/${id}/vm/sandbox/provision`, { method: 'POST' }),
    panic: (id: string) => request(`/agents/${id}/panic`, { method: 'POST' }),

    // Threads & Messages
    getThreads: () => request('/threads'),
    getAgentOnlyThreads: () => request('/threads?agent_only=true'),
    getThread: (id: string) => request(`/threads/${id}`),
    createThread: (data: { type: string; title?: string; member_ids?: string[] }) =>
        request('/threads', { method: 'POST', body: JSON.stringify(data) }),
    getMessages: (threadId: string) => request(`/threads/${threadId}/messages`),
    sendMessage: (threadId: string, data: { content: any; sender_type?: string; sender_id?: string }) =>
        request(`/threads/${threadId}/messages`, { method: 'POST', body: JSON.stringify(data) }),

    // Requests & Approvals
    getRequests: (status?: string, approverType?: string) => {
        const params = new URLSearchParams();
        if (status) params.set('status', status);
        if (approverType) params.set('approver_type', approverType);
        const qs = params.toString();
        return request(`/requests${qs ? `?${qs}` : ''}`);
    },
    createRequest: (data: { type: string; company_id?: string; payload: any }) =>
        request('/requests', { method: 'POST', body: JSON.stringify(data) }),
    approveRequest: (id: string, note?: string) =>
        request(`/requests/${id}/approve`, { method: 'POST', body: JSON.stringify({ note }) }),
    rejectRequest: (id: string, note?: string) =>
        request(`/requests/${id}/reject`, { method: 'POST', body: JSON.stringify({ note }) }),

    // Services
    getServices: () => request('/services'),
    createService: (data: { provider_company_id: string; name: string; description?: string; pricing_model: string; rate: any }) =>
        request('/services', { method: 'POST', body: JSON.stringify(data) }),
    createEngagement: (data: { service_id: string; client_company_id: string; scope: any }) =>
        request('/engagements', { method: 'POST', body: JSON.stringify(data) }),
    activateEngagement: (id: string) => request(`/engagements/${id}/activate`, { method: 'POST' }),
    completeEngagement: (id: string) => request(`/engagements/${id}/complete`, { method: 'POST' }),

    // Company Editing
    updateCompany: (id: string, data: { name?: string; type?: string; description?: string; status?: string }) =>
        request(`/companies/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),

    // Agent Thread (get or create DM)
    getAgentThread: (agentId: string) => request(`/agents/${agentId}/thread`),
    getThreadParticipants: (threadId: string) => request(`/threads/${threadId}/participants`),

    // System Updates
    checkForUpdate: () => request('/system/update-check'),
    performUpdate: () => request('/system/update', { method: 'POST' }),

    // Container Status
    getContainers: () => request('/system/containers'),
    getContainerLogs: (id: string, tail?: number) => request(`/system/containers/${id}/logs?tail=${tail || 200}`),

    // Agent Memories
    getAgentMemories: (agentId: string) => request(`/agents/${agentId}/memories`),
    createAgentMemory: (agentId: string, data: { category: string; key: string; content: string; importance?: number }) =>
        request(`/agents/${agentId}/memories`, { method: 'POST', body: JSON.stringify(data) }),
    deleteAgentMemory: (agentId: string, memoryId: string) =>
        request(`/agents/${agentId}/memories/${memoryId}`, { method: 'DELETE' }),

    // OpenClaw Files
    getOpenClawFiles: (agentId: string) => request(`/agents/${agentId}/openclaw-files`),

    // Thread Participants
    addParticipant: (threadId: string, data: { member_id: string; member_type?: string }) =>
        request(`/threads/${threadId}/participants`, { method: 'POST', body: JSON.stringify(data) }),
    removeParticipant: (threadId: string, memberId: string) =>
        request(`/threads/${threadId}/participants/${memberId}`, { method: 'DELETE' }),

    // Models
    getModels: () => request('/models') as Promise<{ models: string[]; default: string }>,

    // System Settings
    getSettings: () => request('/system/settings'),
    updateSettings: (data: Record<string, string>) =>
        request('/system/settings', { method: 'PUT', body: JSON.stringify(data) }),

    // World
    getWorldSnapshot: () => request('/world/snapshot'),

    // Secrets
    getSecrets: (scopeType?: string, scopeId?: string) => {
        const params = new URLSearchParams();
        if (scopeType) params.set('scope_type', scopeType);
        if (scopeId) params.set('scope_id', scopeId);
        const qs = params.toString();
        return request(`/secrets${qs ? `?${qs}` : ''}`);
    },
    createSecret: (data: { scope_type: string; scope_id: string; name: string; fields: { label: string; value: string }[]; description?: string }) =>
        request('/secrets', { method: 'POST', body: JSON.stringify(data) }),
    deleteSecret: (id: string) =>
        request(`/secrets/${id}`, { method: 'DELETE' }),
};
