const API_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:8080/v1';

function getHeaders() {
    const token = typeof window !== 'undefined' ? localStorage.getItem('admin_token') : '';
    return {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json'
    };
}

export const api = {
    getCompanies: async () => {
        const res = await fetch(`${API_URL}/companies`, { headers: getHeaders() });
        return res.json();
    },
    getOrgTree: async (companyId: string) => {
        const res = await fetch(`${API_URL}/companies/${companyId}/org-tree`, { headers: getHeaders() });
        return res.json();
    },
    hireCeo: async (companyId: string) => {
        const res = await fetch(`${API_URL}/companies/${companyId}/hire-ceo`, { method: 'POST', headers: getHeaders() });
        return res.json();
    },
    hireWorker: async (companyId: string) => {
        const res = await fetch(`${API_URL}/companies/${companyId}/hire-worker`, { method: 'POST', headers: getHeaders() });
        return res.json();
    },
    getAgent: async (agentId: string) => {
        const res = await fetch(`${API_URL}/agents/${agentId}`, { headers: getHeaders() });
        return res.json();
    },
    getThreads: async () => {
        const res = await fetch(`${API_URL}/threads`, { headers: getHeaders() });
        return res.json();
    },
    getThreadMessages: async (threadId: string) => {
        const res = await fetch(`${API_URL}/threads/${threadId}/messages`, { headers: getHeaders() });
        return res.json();
    }
};
