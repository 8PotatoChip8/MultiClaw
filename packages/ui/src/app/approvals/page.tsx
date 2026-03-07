'use client';
import { useEffect, useState, useCallback } from 'react';
import { api } from '../../lib/api';
import { useMultiClawEvents } from '../../lib/ws';
import { Request, Agent } from '../../lib/types';
import { CheckCircle, XCircle, Clock, AlertTriangle, Filter, Inbox } from 'lucide-react';

type StatusFilter = 'PENDING' | 'APPROVED' | 'REJECTED' | 'ALL';

export default function ApprovalsPage() {
    const [requests, setRequests] = useState<Request[]>([]);
    const [agents, setAgents] = useState<Record<string, Agent>>({});
    const [loading, setLoading] = useState(true);
    const [filter, setFilter] = useState<StatusFilter>('PENDING');
    const [actionLoading, setActionLoading] = useState<string | null>(null);
    const event = useMultiClawEvents();

    const load = useCallback(() => {
        const status = filter === 'ALL' ? undefined : filter;
        api.getRequests(status).then(d => {
            setRequests(Array.isArray(d) ? d : []);
            setLoading(false);
        }).catch(() => setLoading(false));
    }, [filter]);

    // Load agents for name resolution
    useEffect(() => {
        api.getAgents().then(d => {
            if (Array.isArray(d)) {
                const map: Record<string, Agent> = {};
                d.forEach((a: Agent) => { map[a.id] = a; });
                setAgents(map);
            }
        }).catch(() => {});
    }, []);

    useEffect(() => { load(); }, [load]);

    // Real-time updates: refresh when request events come in
    useEffect(() => {
        if (event?.type === 'new_request' || event?.type === 'request_approved' || event?.type === 'request_rejected') {
            load();
        }
    }, [event, load]);

    const handleApprove = async (id: string) => {
        setActionLoading(id);
        try {
            await api.approveRequest(id);
            load();
        } finally {
            setActionLoading(null);
        }
    };

    const handleReject = async (id: string) => {
        setActionLoading(id);
        try {
            await api.rejectRequest(id);
            load();
        } finally {
            setActionLoading(null);
        }
    };

    const getRequesterName = (r: Request): string => {
        if (r.created_by_agent_id && agents[r.created_by_agent_id]) {
            return agents[r.created_by_agent_id].name;
        }
        if (r.created_by_user_id) return 'You (Operator)';
        // Try to find requester_id in payload
        if (r.payload?.requester_id && agents[r.payload.requester_id]) {
            return agents[r.payload.requester_id].name;
        }
        return 'Unknown';
    };

    const getRequestDescription = (r: Request): string => {
        if (r.payload?.description) return r.payload.description;
        if (r.payload?.reason) return r.payload.reason;
        // Build a description from the type
        const type = r.type.replace(/_/g, ' ').toLowerCase();
        return `${type} request`;
    };

    const getStatusIcon = (status: string) => {
        switch (status) {
            case 'PENDING': return <Clock size={18} style={{ color: 'var(--warning)' }} />;
            case 'APPROVED': return <CheckCircle size={18} style={{ color: 'var(--success)' }} />;
            case 'REJECTED': return <XCircle size={18} style={{ color: 'var(--danger)' }} />;
            default: return <AlertTriangle size={18} style={{ color: 'var(--text-muted)' }} />;
        }
    };

    const getStatusBadgeClass = (status: string) => {
        switch (status) {
            case 'PENDING': return 'pending';
            case 'APPROVED': return 'active';
            case 'REJECTED': return 'quarantined';
            default: return '';
        }
    };

    const pendingCount = requests.filter(r => r.status === 'PENDING').length;
    const filterTabs: { key: StatusFilter; label: string }[] = [
        { key: 'PENDING', label: 'Pending' },
        { key: 'APPROVED', label: 'Approved' },
        { key: 'REJECTED', label: 'Rejected' },
        { key: 'ALL', label: 'All' },
    ];

    const timeAgo = (dateStr: string): string => {
        const diff = Date.now() - new Date(dateStr).getTime();
        const mins = Math.floor(diff / 60000);
        if (mins < 1) return 'just now';
        if (mins < 60) return `${mins}m ago`;
        const hrs = Math.floor(mins / 60);
        if (hrs < 24) return `${hrs}h ago`;
        const days = Math.floor(hrs / 24);
        return `${days}d ago`;
    };

    return (
        <div className="animate-in" style={{ maxWidth: '960px' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '24px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '6px' }}>Approvals</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>
                        Review and respond to requests from your agents.
                    </p>
                </div>
                {filter === 'PENDING' && pendingCount > 0 && (
                    <div style={{
                        background: 'rgba(245, 158, 11, 0.15)',
                        color: 'var(--warning)',
                        padding: '6px 14px',
                        borderRadius: '20px',
                        fontSize: '13px',
                        fontWeight: 600,
                    }}>
                        {pendingCount} pending
                    </div>
                )}
            </div>

            {/* Filter tabs */}
            <div style={{ display: 'flex', gap: '4px', marginBottom: '24px', background: 'rgba(0,0,0,0.2)', padding: '4px', borderRadius: '10px', width: 'fit-content' }}>
                {filterTabs.map(tab => (
                    <button
                        key={tab.key}
                        onClick={() => { setLoading(true); setFilter(tab.key); }}
                        style={{
                            padding: '8px 18px',
                            borderRadius: '8px',
                            border: 'none',
                            cursor: 'pointer',
                            fontSize: '13px',
                            fontWeight: 600,
                            transition: 'all 0.2s',
                            background: filter === tab.key ? 'var(--panel)' : 'transparent',
                            color: filter === tab.key ? 'var(--text)' : 'var(--text-muted)',
                        }}
                    >
                        {tab.label}
                    </button>
                ))}
            </div>

            {loading ? (
                <p style={{ color: 'var(--text-muted)' }}>Loading...</p>
            ) : requests.length === 0 ? (
                <div className="panel" style={{ textAlign: 'center', padding: '80px 20px' }}>
                    <Inbox size={48} style={{ color: 'var(--text-muted)', marginBottom: '16px', opacity: 0.5 }} />
                    <p style={{ color: 'var(--text-muted)', fontSize: '15px', marginBottom: '4px' }}>
                        {filter === 'PENDING' ? 'No pending requests' : `No ${filter.toLowerCase()} requests`}
                    </p>
                    <p style={{ color: 'var(--text-muted)', fontSize: '13px', opacity: 0.7 }}>
                        {filter === 'PENDING' ? 'All clear! Your agents are operating within their authority.' : 'Nothing to show here yet.'}
                    </p>
                </div>
            ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                    {requests.map(r => (
                        <div key={r.id} className="panel" style={{ padding: '20px' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                                <div style={{ flex: 1, minWidth: 0 }}>
                                    {/* Header row */}
                                    <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '8px' }}>
                                        {getStatusIcon(r.status)}
                                        <span style={{ fontWeight: 700, fontSize: '15px' }}>{r.type.replace(/_/g, ' ')}</span>
                                        <span className={`badge ${getStatusBadgeClass(r.status)}`} style={{ fontSize: '11px' }}>{r.status}</span>
                                    </div>

                                    {/* Description */}
                                    <p style={{ color: 'var(--text)', fontSize: '14px', marginBottom: '10px', lineHeight: 1.5 }}>
                                        {getRequestDescription(r)}
                                    </p>

                                    {/* Metadata */}
                                    <div style={{ display: 'flex', gap: '16px', fontSize: '12px', color: 'var(--text-muted)' }}>
                                        <span>From: <strong style={{ color: 'var(--text)' }}>{getRequesterName(r)}</strong></span>
                                        <span>{timeAgo(r.created_at)}</span>
                                        {r.company_id && agents[r.created_by_agent_id || '']?.company_id && (
                                            <span>Company: {r.company_id.slice(0, 8)}...</span>
                                        )}
                                    </div>

                                    {/* Payload details (collapsible) */}
                                    {r.payload && Object.keys(r.payload).length > 0 && (
                                        <details style={{ marginTop: '12px' }}>
                                            <summary style={{ fontSize: '12px', color: 'var(--text-muted)', cursor: 'pointer', userSelect: 'none' }}>
                                                View details
                                            </summary>
                                            <pre style={{
                                                fontSize: '11px',
                                                color: 'var(--text-muted)',
                                                marginTop: '8px',
                                                background: 'rgba(0,0,0,0.25)',
                                                padding: '10px',
                                                borderRadius: '6px',
                                                overflowX: 'auto',
                                                whiteSpace: 'pre-wrap',
                                                wordBreak: 'break-word',
                                            }}>
                                                {JSON.stringify(r.payload, null, 2)}
                                            </pre>
                                        </details>
                                    )}
                                </div>

                                {/* Action buttons */}
                                {r.status === 'PENDING' && (
                                    <div style={{ display: 'flex', gap: '8px', flexShrink: 0, marginLeft: '20px' }}>
                                        <button
                                            className="button small"
                                            onClick={() => handleApprove(r.id)}
                                            disabled={actionLoading === r.id}
                                            style={{ display: 'flex', alignItems: 'center', gap: '6px' }}
                                        >
                                            <CheckCircle size={14} /> Approve
                                        </button>
                                        <button
                                            className="button small danger"
                                            onClick={() => handleReject(r.id)}
                                            disabled={actionLoading === r.id}
                                            style={{ display: 'flex', alignItems: 'center', gap: '6px' }}
                                        >
                                            <XCircle size={14} /> Reject
                                        </button>
                                    </div>
                                )}
                            </div>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
