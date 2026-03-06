'use client';
import { useEffect, useState, useRef } from 'react';
import { api } from '../../lib/api';
import { Thread, Message, Agent } from '../../lib/types';
import { Radio, Eye, Users, MessageSquare } from 'lucide-react';
import { useMultiClawEvents } from '../../lib/ws';

interface Participant { thread_id: string; member_type: string; member_id: string; }

export default function AgentCommsPage() {
    const [threads, setThreads] = useState<Thread[]>([]);
    const [selectedThread, setSelectedThread] = useState<string | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);
    const [participants, setParticipants] = useState<Participant[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [loading, setLoading] = useState(true);
    const feedRef = useRef<HTMLDivElement>(null);
    const agentMap = new Map(agents.map(a => [a.id, a]));
    const lastEvent = useMultiClawEvents();

    const loadThreads = () => {
        Promise.all([api.getAgentOnlyThreads(), api.getAgents()]).then(([t, a]) => {
            setAgents(Array.isArray(a) ? a : []);
            setThreads(Array.isArray(t) ? t : []);
            setLoading(false);
        }).catch(() => setLoading(false));
    };

    useEffect(() => { loadThreads(); }, []);

    // Real-time updates via WebSocket
    useEffect(() => {
        if (!lastEvent || lastEvent.type !== 'new_message') return;
        const msg = lastEvent.message;
        if (!msg) return;

        // Append to current thread if it matches
        if (msg.thread_id === selectedThread) {
            setMessages(prev => {
                if (prev.some(m => m.id === msg.id)) return prev;
                return [...prev, msg];
            });
        }

        // Refresh thread list to pick up new threads
        loadThreads();
    }, [lastEvent]);

    useEffect(() => {
        if (!selectedThread) { setMessages([]); setParticipants([]); return; }
        api.getMessages(selectedThread).then(d => setMessages(Array.isArray(d) ? d : []));
        api.getThreadParticipants(selectedThread).then(d => setParticipants(Array.isArray(d) ? d : []));
        // Keep polling as fallback (every 10s)
        const interval = setInterval(() => {
            api.getMessages(selectedThread).then(d => setMessages(Array.isArray(d) ? d : []));
        }, 10000);
        return () => clearInterval(interval);
    }, [selectedThread]);

    useEffect(() => { feedRef.current?.scrollTo(0, feedRef.current.scrollHeight); }, [messages]);

    const agentParticipants = participants.filter(p => p.member_type === 'AGENT');

    return (
        <div className="animate-in">
            <div style={{ marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '4px' }}>
                    <h1 style={{ fontSize: '28px', fontWeight: 700 }}>Agent Comms</h1>
                    <span style={{
                        fontSize: '10px', padding: '3px 10px', borderRadius: '12px',
                        background: 'rgba(99,102,241,0.15)', color: 'var(--primary)',
                        fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.05em',
                    }}>
                        <Eye size={10} style={{ marginRight: '4px', verticalAlign: 'middle' }} />
                        Read-Only
                    </span>
                </div>
                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Monitor agent-to-agent conversations and group chats</p>
            </div>

            <div style={{ display: 'flex', gap: '16px', height: 'calc(100vh - 200px)' }}>
                {/* Thread List */}
                <div className="panel" style={{ width: '260px', minWidth: '260px', overflowY: 'auto' }}>
                    <h3 style={{
                        fontSize: '13px', fontWeight: 600, color: 'var(--text-muted)', marginBottom: '12px',
                        textTransform: 'uppercase', letterSpacing: '0.05em',
                    }}>
                        Agent Threads ({threads.length})
                    </h3>
                    {loading ? (
                        <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Loading...</p>
                    ) : threads.length === 0 ? (
                        <div style={{ textAlign: 'center', padding: '24px 12px' }}>
                            <Radio size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No agent conversations yet</p>
                        </div>
                    ) : threads.map(t => (
                        <div key={t.id} onClick={() => setSelectedThread(t.id)} style={{
                            padding: '10px 12px', borderRadius: '8px', cursor: 'pointer', marginBottom: '2px',
                            background: selectedThread === t.id ? 'var(--primary-glow)' : 'transparent',
                            borderLeft: selectedThread === t.id ? '3px solid var(--accent)' : '3px solid transparent',
                            transition: 'all 0.15s',
                        }}>
                            <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                {t.type === 'DM' ? (
                                    <MessageSquare size={12} style={{ color: 'var(--accent)' }} />
                                ) : (
                                    <Users size={12} style={{ color: 'var(--accent)' }} />
                                )}
                                <span style={{ fontSize: '13px', fontWeight: 500 }}>{t.title || 'Agent Thread'}</span>
                            </div>
                            <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>{t.type}</div>
                        </div>
                    ))}
                </div>

                {/* Message Feed (Read-Only) */}
                <div className="panel" style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
                    {selectedThread ? (
                        <>
                            {/* Header */}
                            <div style={{ paddingBottom: '12px', borderBottom: '1px solid var(--border)', marginBottom: '12px' }}>
                                <h3 style={{ fontSize: '14px', fontWeight: 600, display: 'flex', alignItems: 'center', gap: '8px' }}>
                                    <Radio size={14} style={{ color: 'var(--accent)' }} />
                                    {threads.find(t => t.id === selectedThread)?.title || 'Agent Thread'}
                                </h3>
                                {agentParticipants.length > 0 && (
                                    <div style={{ display: 'flex', gap: '6px', marginTop: '6px', flexWrap: 'wrap' }}>
                                        {agentParticipants.map(p => {
                                            const a = agentMap.get(p.member_id);
                                            return (
                                                <span key={p.member_id} style={{
                                                    fontSize: '11px', padding: '2px 8px', borderRadius: '12px',
                                                    background: 'rgba(255,255,255,0.06)', color: 'var(--text-muted)',
                                                }}>
                                                    {a?.name || p.member_id.slice(0, 8)}
                                                    {a?.role && <span style={{ marginLeft: '4px', opacity: 0.6 }}>({a.role})</span>}
                                                </span>
                                            );
                                        })}
                                    </div>
                                )}
                            </div>

                            {/* Messages */}
                            <div ref={feedRef} style={{ flex: 1, overflowY: 'auto' }}>
                                {messages.length === 0 ? (
                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px', textAlign: 'center', padding: '40px' }}>
                                        No messages in this thread yet
                                    </p>
                                ) : messages.map(m => {
                                    const senderAgent = agentMap.get(m.sender_id);
                                    const isSystem = m.sender_type === 'SYSTEM';
                                    return (
                                        <div key={m.id} style={{
                                            padding: '10px 14px', marginBottom: '6px',
                                            background: isSystem ? 'rgba(255,200,0,0.05)' : 'rgba(0,0,0,0.2)',
                                            borderRadius: '8px',
                                            borderLeft: `3px solid ${isSystem ? 'var(--warning, #f59e0b)' : 'var(--accent)'}`,
                                        }}>
                                            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '4px' }}>
                                                <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--accent)' }}>
                                                    {senderAgent?.name || m.sender_type}
                                                    {senderAgent?.role && (
                                                        <span style={{ marginLeft: '6px', fontSize: '10px', opacity: 0.6 }}>[{senderAgent.role}]</span>
                                                    )}
                                                </span>
                                                <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                                    {new Date(m.created_at).toLocaleTimeString()}
                                                </span>
                                            </div>
                                            <div style={{ fontSize: '13px', lineHeight: '1.6', whiteSpace: 'pre-wrap', color: 'var(--text-muted)' }}>
                                                {typeof m.content === 'object' ? m.content?.text || JSON.stringify(m.content) : String(m.content)}
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>

                            {/* Read-only footer */}
                            <div style={{
                                borderTop: '1px solid var(--border)', padding: '10px 14px',
                                display: 'flex', alignItems: 'center', gap: '8px',
                                color: 'var(--text-muted)', fontSize: '12px',
                            }}>
                                <Eye size={14} />
                                Monitoring mode — live updates via WebSocket
                            </div>
                        </>
                    ) : (
                        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                            <div style={{ textAlign: 'center' }}>
                                <Radio size={36} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Select a thread to monitor agent conversations</p>
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
