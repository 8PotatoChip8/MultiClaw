'use client';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Request } from '../lib/types';
import { CheckCircle, XCircle, Clock } from 'lucide-react';

export default function ApprovalsInbox() {
    const [requests, setRequests] = useState<Request[]>([]);

    const load = () => {
        api.getRequests('PENDING', 'USER').then(d => setRequests(Array.isArray(d) ? d : [])).catch(() => { });
    };
    useEffect(() => { load(); }, []);

    const handleApprove = async (id: string) => { await api.approveRequest(id); load(); };
    const handleReject = async (id: string) => { await api.rejectRequest(id); load(); };

    if (requests.length === 0) {
        return <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>No pending requests.</p>;
    }

    return (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
            {requests.map(r => (
                <div key={r.id} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '12px', background: 'rgba(0,0,0,0.2)', borderRadius: '8px' }}>
                    <div>
                        <div style={{ fontWeight: 600, fontSize: '14px' }}>{r.type.replace(/_/g, ' ')}</div>
                        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>Approver: {r.current_approver_type}</div>
                    </div>
                    <div style={{ display: 'flex', gap: '6px' }}>
                        <button className="button small" onClick={() => handleApprove(r.id)}><CheckCircle size={14} /></button>
                        <button className="button small danger" onClick={() => handleReject(r.id)}><XCircle size={14} /></button>
                    </div>
                </div>
            ))}
        </div>
    );
}
