'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Thread, Message } from '../../lib/types';
import { Radio, MessageSquare } from 'lucide-react';

export default function MessagingPage() {
    const [threads, setThreads] = useState<Thread[]>([]);
    const [selectedThread, setSelectedThread] = useState<string | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        api.getThreads().then(d => {
            const list = Array.isArray(d) ? d : [];
            setThreads(list);
            setLoading(false);
        }).catch(() => setLoading(false));
    }, []);

    useEffect(() => {
        if (!selectedThread) { setMessages([]); return; }
        api.getMessages(selectedThread).then(d => {
            setMessages(Array.isArray(d) ? d : []);
        });
    }, [selectedThread]);

    // Filter to show threads that have agent-to-agent messages
    const agentMessages = messages.filter(m => m.sender_type === 'AGENT');

    return (
        <div className="animate-in">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>Agent Communications</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Monitor inter-agent conversations and activity</p>
                </div>
            </div>

            <div style={{ display: 'flex', gap: '16px', height: 'calc(100vh - 200px)' }}>
                {/* Thread List */}
                <div className="panel" style={{ width: '260px', minWidth: '260px', overflowY: 'auto' }}>
                    <h3 style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-muted)', marginBottom: '12px', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                        All Threads
                    </h3>
                    {loading ? (
                        <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Loading...</p>
                    ) : threads.length === 0 ? (
                        <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No threads found</p>
                    ) : threads.map(t => (
                        <div
                            key={t.id}
                            onClick={() => setSelectedThread(t.id)}
                            style={{
                                padding: '10px 12px', borderRadius: '8px', cursor: 'pointer',
                                marginBottom: '2px',
                                background: selectedThread === t.id ? 'var(--primary-glow)' : 'transparent',
                                borderLeft: selectedThread === t.id ? '3px solid var(--accent)' : '3px solid transparent',
                                transition: 'all 0.15s',
                            }}
                        >
                            <div style={{ fontSize: '13px', fontWeight: 500 }}>{t.title || 'Thread'}</div>
                            <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>{t.type}</div>
                        </div>
                    ))}
                </div>

                {/* Message Feed */}
                <div className="panel" style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
                    {selectedThread ? (
                        <>
                            <h3 style={{ fontSize: '14px', fontWeight: 600, marginBottom: '16px', paddingBottom: '12px', borderBottom: '1px solid var(--border)' }}>
                                <Radio size={14} style={{ color: 'var(--accent)', marginRight: '8px' }} />
                                Message Feed
                            </h3>
                            <div style={{ flex: 1, overflowY: 'auto' }}>
                                {agentMessages.length === 0 ? (
                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px', textAlign: 'center', padding: '40px' }}>
                                        No agent messages in this thread
                                    </p>
                                ) : agentMessages.map(m => (
                                    <div key={m.id} style={{
                                        padding: '12px', marginBottom: '8px',
                                        background: 'rgba(0,0,0,0.2)', borderRadius: '8px',
                                        borderLeft: '3px solid var(--accent)',
                                    }}>
                                        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '6px' }}>
                                            <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--accent)' }}>
                                                Agent {m.sender_id.slice(0, 8)}
                                            </span>
                                            <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                                {new Date(m.created_at).toLocaleTimeString()}
                                            </span>
                                        </div>
                                        <div style={{ fontSize: '13px', lineHeight: '1.5' }}>
                                            {typeof m.content === 'object' ? m.content?.text || JSON.stringify(m.content) : String(m.content)}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        </>
                    ) : (
                        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                            <div style={{ textAlign: 'center' }}>
                                <MessageSquare size={36} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Select a thread to view agent messages</p>
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
