'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Request } from '../../lib/types';
import { CheckCircle, XCircle, Clock } from 'lucide-react';

export default function ApprovalsPage() {
    const [requests, setRequests] = useState<Request[]>([]);
    const [loading, setLoading] = useState(true);

    const load = () => {
        api.getRequests().then(d => { setRequests(Array.isArray(d) ? d : []); setLoading(false); }).catch(() => setLoading(false));
    };
    useEffect(() => { load(); }, []);

    const handleApprove = async (id: string) => {
        await api.approveRequest(id);
        load();
    };
    const handleReject = async (id: string) => {
        await api.rejectRequest(id);
        load();
    };

    return (
        <div className="animate-in" style={{ maxWidth: '900px' }}>
            <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '8px' }}>Approvals Inbox</h1>
            <p style={{ color: 'var(--text-muted)', marginBottom: '32px', fontSize: '14px' }}>Review structural and resource changes requested by agents.</p>

            {loading ? <p style={{ color: 'var(--text-muted)' }}>Loading...</p> :
                requests.length === 0 ? (
                    <div className="panel" style={{ textAlign: 'center', padding: '60px 20px' }}>
                        <CheckCircle size={40} style={{ color: 'var(--success)', marginBottom: '12px' }} />
                        <p style={{ color: 'var(--text-muted)' }}>No pending requests. All clear!</p>
                    </div>
                ) : (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                        {requests.map(r => (
                            <div key={r.id} className="panel" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                <div>
                                    <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '6px' }}>
                                        <Clock size={16} style={{ color: 'var(--warning)' }} />
                                        <span style={{ fontWeight: 600 }}>{r.type.replace(/_/g, ' ')}</span>
                                        <span className={`badge ${r.status === 'PENDING' ? 'pending' : r.status === 'APPROVED' ? 'active' : 'quarantined'}`}>{r.status}</span>
                                    </div>
                                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
                                        Approver: {r.current_approver_type} · Created {new Date(r.created_at).toLocaleDateString()}
                                    </div>
                                    {r.payload && (
                                        <pre style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '8px', background: 'rgba(0,0,0,0.2)', padding: '8px', borderRadius: '6px' }}>
                                            {JSON.stringify(r.payload, null, 2)}
                                        </pre>
                                    )}
                                </div>
                                {r.status === 'PENDING' && (
                                    <div style={{ display: 'flex', gap: '8px', flexShrink: 0 }}>
                                        <button className="button small" onClick={() => handleApprove(r.id)} style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                            <CheckCircle size={14} /> Approve
                                        </button>
                                        <button className="button small danger" onClick={() => handleReject(r.id)} style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                            <XCircle size={14} /> Reject
                                        </button>
                                    </div>
                                )}
                            </div>
                        ))}
                    </div>
                )}
        </div>
    );
}
