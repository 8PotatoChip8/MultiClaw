'use client';

import ApprovalsInbox from '../../components/ApprovalsInbox';

export default function ApprovalsPage() {
    return (
        <div style={{ maxWidth: '800px', margin: '0 auto' }}>
            <h1 style={{ marginBottom: '30px' }}>Approvals Inbox</h1>
            <p style={{ color: 'var(--text-muted)', marginBottom: '30px' }}>
                Review structural and resource changes requested by agents per the holding policy.
            </p>
            <ApprovalsInbox />
        </div>
    );
}
