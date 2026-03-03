const API_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:8080/v1';

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

    // Agents
    getAgents: () => request('/agents'),
    getAgent: (id: string) => request(`/agents/${id}`),
    patchAgent: (id: string, data: { preferred_model?: string; specialty?: string; system_prompt?: string }) =>
        request(`/agents/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
    hireManager: (ceoId: string, data: { name: string; specialty?: string; preferred_model?: string }) =>
        request(`/agents/${ceoId}/hire-manager`, { method: 'POST', body: JSON.stringify(data) }),
    hireWorker: (mgrId: string, data: { name: string; specialty?: string; preferred_model?: string }) =>
        request(`/agents/${mgrId}/hire-worker`, { method: 'POST', body: JSON.stringify(data) }),
    vmStart: (id: string) => request(`/agents/${id}/vm/start`, { method: 'POST' }),
    vmStop: (id: string) => request(`/agents/${id}/vm/stop`, { method: 'POST' }),
    vmRebuild: (id: string) => request(`/agents/${id}/vm/rebuild`, { method: 'POST' }),
    panic: (id: string) => request(`/agents/${id}/panic`, { method: 'POST' }),

    // Threads & Messages
    getThreads: () => request('/threads'),
    getThread: (id: string) => request(`/threads/${id}`),
    createThread: (data: { type: string; title?: string }) =>
        request('/threads', { method: 'POST', body: JSON.stringify(data) }),
    getMessages: (threadId: string) => request(`/threads/${threadId}/messages`),
    sendMessage: (threadId: string, data: { content: any; sender_type?: string; sender_id?: string }) =>
        request(`/threads/${threadId}/messages`, { method: 'POST', body: JSON.stringify(data) }),

    // Requests & Approvals
    getRequests: (status?: string) => request(`/requests${status ? `?status=${status}` : ''}`),
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
};
