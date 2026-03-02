'use client';

import Chat from '../../components/Chat';

export default function ChatsPage() {
    return (
        <div style={{ display: 'flex', height: 'calc(100vh - 120px)', gap: '20px' }}>
            <div className="panel" style={{ width: '250px', overflowY: 'auto' }}>
                <h3>Threads</h3>
                <ul style={{ listStyle: 'none', padding: 0 }}>
                    <li style={{ padding: '10px', background: 'var(--bg)', borderRadius: '4px', cursor: 'pointer', marginBottom: '5px' }}>MainAgent DM</li>
                    <li style={{ padding: '10px', cursor: 'pointer', marginBottom: '5px' }}>Alpha Sales Team</li>
                </ul>
            </div>
            <div style={{ flex: 1 }}>
                <Chat threadId="1" initialMessages={[
                    { id: 'msg1', sender_id: '1', sender_type: 'AGENT', content: 'Hello! I am MainAgent. How can I assist the holding company today?', created_at: new Date().toISOString() }
                ]} />
            </div>
        </div>
    );
}
