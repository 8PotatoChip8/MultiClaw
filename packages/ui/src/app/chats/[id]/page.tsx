'use client';

import { useEffect, useState } from 'react';
import Chat from '../../../components/Chat';
import { api } from '../../../lib/api';
import React from 'react';

export default function ChatViewPage({ params }: { params: { id: string } }) {
    const [messages, setMessages] = useState<any[]>([]);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        async function fetchMessages() {
            try {
                const data = await api.getThreadMessages(params.id);
                setMessages(Array.isArray(data) ? data : []);
            } catch (e) {
                console.error("Failed to fetch messages", e);
            } finally {
                setLoading(false);
            }
        }
        fetchMessages();
    }, [params.id]);

    if (loading) return <div>Loading thread...</div>;

    return (
        <div style={{ display: 'flex', height: 'calc(100vh - 120px)', gap: '20px' }}>
            <div className="panel" style={{ width: '250px', overflowY: 'auto' }}>
                <h3>Threads</h3>
                <ul style={{ listStyle: 'none', padding: 0 }}>
                    <li style={{ padding: '10px', background: 'var(--bg)', borderRadius: '4px', cursor: 'pointer', marginBottom: '5px' }}>Current Thread: {params.id}</li>
                </ul>
            </div>
            <div style={{ flex: 1 }}>
                <Chat threadId={params.id} initialMessages={messages} />
            </div>
        </div>
    );
}
