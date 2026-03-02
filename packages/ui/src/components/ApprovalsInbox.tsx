import { CheckSquare, XSquare } from 'lucide-react';

export default function ApprovalsInbox() {
    const requests = [
        { id: '1', type: 'INCREASE_WORKER_LIMIT', company: 'Alpha Inc', status: 'PENDING', requestedBy: 'Manager Bob' }
    ]; // Mock data

    const handleApprove = (id: string) => { console.log('Approve', id); };
    const handleReject = (id: string) => { console.log('Reject', id); };

    if (requests.length === 0) {
        return <div style={{ color: 'var(--text-muted)' }}>No pending approvals.</div>;
    }

    return (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '15px' }}>
            {requests.map(r => (
                <div key={r.id} className="panel" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <div>
                        <strong>{r.type}</strong>
                        <div style={{ fontSize: '0.9rem', color: 'var(--text-muted)', marginTop: '5px' }}>
                            Company: {r.company} | Requested By: {r.requestedBy}
                        </div>
                    </div>
                    <div style={{ display: 'flex', gap: '10px' }}>
                        <button className="button" style={{ display: 'flex', alignItems: 'center', gap: '5px' }} onClick={() => handleApprove(r.id)}>
                            <CheckSquare size={16} /> Approve
                        </button>
                        <button className="button danger" style={{ display: 'flex', alignItems: 'center', gap: '5px' }} onClick={() => handleReject(r.id)}>
                            <XSquare size={16} /> Reject
                        </button>
                    </div>
                </div>
            ))}
        </div>
    );
}
