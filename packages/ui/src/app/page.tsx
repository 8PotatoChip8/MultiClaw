'use client';
import { useMultiClawEvents } from '../lib/ws';

export default function Home() {
    const event = useMultiClawEvents();

    return (
        <div>
            <h1 style={{ marginBottom: '20px' }}>Holding Company Overview</h1>
            <p style={{ color: 'var(--text-muted)' }}>Welcome to your autonomous holding company.</p>

            <div style={{ display: 'flex', gap: '20px', marginTop: '40px' }}>
                <div className="panel" style={{ flex: 1 }}>
                    <h3>MainAgent Status</h3>
                    <p style={{ color: 'var(--success)' }}>Online</p>
                </div>
                <div className="panel" style={{ flex: 1 }}>
                    <h3>Pending Approvals</h3>
                    <p>0 Actions required</p>
                </div>
                <div className="panel" style={{ flex: 1 }}>
                    <h3>Active VMs</h3>
                    <p>0 Agents running</p>
                </div>
            </div>

            <div className="panel" style={{ marginTop: '40px' }}>
                <h3>Latest System Events</h3>
                {event ? (
                    <pre style={{ background: '#000', padding: '10px', borderRadius: '4px' }}>
                        {JSON.stringify(event, null, 2)}
                    </pre>
                ) : (
                    <p style={{ color: 'var(--text-muted)' }}>Waiting for ws://localhost:8080/v1/events...</p>
                )}
            </div>
        </div>
    );
}
