'use client';
import { useEffect, useState, useRef } from 'react';
import { api } from '../lib/api';
import { useMultiClawEvents } from '../lib/ws';
import { Message, Agent } from '../lib/types';
import MarkdownText from './MarkdownText';
import { Send, Loader2, Plus, UserMinus, Users, X, Copy, Check } from 'lucide-react';

interface Participant { thread_id: string; member_type: string; member_id: string; }

interface ChatProps {
    threadId: string;
    threadType?: string;
    initialMessages: any[];
}

export default function Chat({ threadId, threadType, initialMessages }: ChatProps) {
    const [messages, setMessages] = useState<Message[]>([]);
    const [input, setInput] = useState('');
    const [sending, setSending] = useState(false);
    const [agentTyping, setAgentTyping] = useState(false);
    const [participants, setParticipants] = useState<Participant[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [showAddMember, setShowAddMember] = useState(false);
    const [hoveredMsg, setHoveredMsg] = useState<string | null>(null);
    const [copiedId, setCopiedId] = useState<string | null>(null);
    const bottomRef = useRef<HTMLDivElement>(null);
    const event = useMultiClawEvents();
    const agentMap = new Map(agents.map(a => [a.id, a]));
    const isGroup = threadType === 'GROUP';

    useEffect(() => {
        api.getMessages(threadId).then(d => setMessages(Array.isArray(d) ? d : []));
        if (isGroup) {
            api.getThreadParticipants(threadId).then(d => setParticipants(Array.isArray(d) ? d : []));
            api.getAgents().then(d => setAgents(Array.isArray(d) ? d : []));
        }
    }, [threadId, threadType]);

    // Listen for new_message events via WebSocket
    useEffect(() => {
        if (!event) return;
        try {
            const data = typeof event === 'string' ? JSON.parse(event) : event;
            if (data.type === 'new_message' && data.message?.thread_id === threadId) {
                const newMsg = data.message as Message;
                setMessages(prev => {
                    if (prev.some(m => m.id === newMsg.id)) return prev;
                    return [...prev, newMsg];
                });
                if (newMsg.sender_type === 'AGENT') {
                    setAgentTyping(false);
                    setSending(false);
                }
            }
        } catch { }
    }, [event, threadId]);

    useEffect(() => {
        bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [messages]);

    const handleSend = async () => {
        if (!input.trim() || sending) return;
        setSending(true);
        setAgentTyping(true);
        const content = input.trim();
        setInput('');

        const tempMsg: Message = {
            id: 'temp-' + Date.now(),
            thread_id: threadId,
            sender_type: 'USER',
            sender_id: '00000000-0000-0000-0000-000000000000',
            content: { text: content },
            created_at: new Date().toISOString(),
        };
        setMessages(prev => [...prev, tempMsg]);

        try {
            await api.sendMessage(threadId, { content: { text: content }, sender_type: 'USER' });
            const updated = await api.getMessages(threadId);
            setMessages(Array.isArray(updated) ? updated : []);
        } catch (e) {
            console.error(e);
            setAgentTyping(false);
            setSending(false);
        }

        const startTime = Date.now();
        const pollInterval = setInterval(async () => {
            if (Date.now() - startTime > 120000) {
                clearInterval(pollInterval);
                setAgentTyping(false);
                setSending(false);
                return;
            }
            try {
                const msgs = await api.getMessages(threadId);
                if (Array.isArray(msgs)) {
                    setMessages(msgs);
                    const hasAgentResponse = msgs.some((m: Message) =>
                        m.sender_type === 'AGENT' && new Date(m.created_at).getTime() > startTime - 1000
                    );
                    if (hasAgentResponse) {
                        clearInterval(pollInterval);
                        setAgentTyping(false);
                        setSending(false);
                    }
                }
            } catch { }
        }, 2000);
    };

    const handleAddMember = async (agentId: string) => {
        await api.addParticipant(threadId, { member_id: agentId, member_type: 'AGENT' });
        api.getThreadParticipants(threadId).then(d => setParticipants(Array.isArray(d) ? d : []));
        setShowAddMember(false);
    };

    const handleRemoveMember = async (memberId: string) => {
        await api.removeParticipant(threadId, memberId);
        api.getThreadParticipants(threadId).then(d => setParticipants(Array.isArray(d) ? d : []));
    };

    const getContent = (msg: Message) => {
        if (typeof msg.content === 'string') return msg.content;
        if (msg.content?.text) return msg.content.text;
        return JSON.stringify(msg.content);
    };

    const getSenderLabel = (msg: Message) => {
        if (msg.sender_type === 'USER') return 'You';
        const agent = agentMap.get(msg.sender_id);
        if (agent) return `🤖 ${agent.name}`;
        return '🤖 Agent';
    };

    const handleCopy = (msgId: string, text: string) => {
        navigator.clipboard.writeText(text);
        setCopiedId(msgId);
        setTimeout(() => setCopiedId(null), 1500);
    };

    const agentParticipants = participants.filter(p => p.member_type === 'AGENT');
    const nonMemberAgents = agents.filter(a => !agentParticipants.some(p => p.member_id === a.id));

    return (
        <div className="panel no-hover" style={{ height: '100%', display: 'flex', flexDirection: 'column', padding: '0' }}>
            {/* Group header with participants */}
            {isGroup && agentParticipants.length > 0 && (
                <div style={{
                    padding: '10px 16px', borderBottom: '1px solid var(--border)',
                    display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                }}>
                    <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap', alignItems: 'center' }}>
                        <Users size={14} style={{ color: 'var(--accent)', marginRight: '4px' }} />
                        {agentParticipants.map(p => {
                            const a = agentMap.get(p.member_id);
                            return (
                                <span key={p.member_id} style={{
                                    display: 'inline-flex', alignItems: 'center', gap: '4px',
                                    fontSize: '11px', padding: '2px 8px', borderRadius: '12px',
                                    background: 'rgba(255,255,255,0.06)', color: 'var(--text-muted)',
                                }}>
                                    {a?.name || p.member_id.slice(0, 8)}
                                    <button onClick={() => handleRemoveMember(p.member_id)} style={{
                                        background: 'none', border: 'none', color: '#ef4444',
                                        cursor: 'pointer', padding: '0 2px', opacity: 0.6,
                                        display: 'inline-flex', alignItems: 'center',
                                    }} title="Remove"><UserMinus size={10} /></button>
                                </span>
                            );
                        })}
                    </div>
                    <button onClick={() => setShowAddMember(true)} style={{
                        background: 'none', border: '1px solid var(--border)', color: 'var(--primary)',
                        cursor: 'pointer', padding: '4px 10px', borderRadius: '6px',
                        fontSize: '11px', fontWeight: 600, display: 'flex', alignItems: 'center', gap: '4px',
                    }}>
                        <Plus size={12} /> Add
                    </button>
                </div>
            )}
            {isGroup && agentParticipants.length === 0 && (
                <div style={{
                    padding: '10px 16px', borderBottom: '1px solid var(--border)',
                    display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                }}>
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>No participants yet</span>
                    <button onClick={() => { api.getAgents().then(d => setAgents(Array.isArray(d) ? d : [])); setShowAddMember(true); }} style={{
                        background: 'none', border: '1px solid var(--border)', color: 'var(--primary)',
                        cursor: 'pointer', padding: '4px 10px', borderRadius: '6px',
                        fontSize: '11px', fontWeight: 600, display: 'flex', alignItems: 'center', gap: '4px',
                    }}>
                        <Plus size={12} /> Add Agents
                    </button>
                </div>
            )}

            {/* Messages */}
            <div style={{ flex: 1, overflowY: 'auto', padding: '20px' }}>
                {messages.length === 0 && (
                    <p style={{ color: 'var(--text-muted)', textAlign: 'center', marginTop: '40px' }}>No messages yet. Start the conversation!</p>
                )}
                {messages.map(msg => (
                    <div key={msg.id} style={{
                        display: 'flex',
                        justifyContent: msg.sender_type === 'USER' ? 'flex-end' : 'flex-start',
                        marginBottom: '12px',
                    }}>
                        <div
                            style={{ position: 'relative', maxWidth: '70%' }}
                            onMouseEnter={() => setHoveredMsg(msg.id)}
                            onMouseLeave={() => setHoveredMsg(null)}
                        >
                            <div style={{
                                padding: '10px 16px',
                                borderRadius: msg.sender_type === 'USER' ? '16px 16px 4px 16px' : '16px 16px 16px 4px',
                                background: msg.sender_type === 'USER' ? 'linear-gradient(135deg, var(--primary), var(--accent))' : 'rgba(30,40,68,0.9)',
                                fontSize: '14px',
                                lineHeight: '1.5',
                            }}>
                                <div style={{ fontSize: '11px', color: 'rgba(255,255,255,0.6)', marginBottom: '4px', fontWeight: 600 }}>
                                    {getSenderLabel(msg)}
                                </div>
                                <MarkdownText>{getContent(msg)}</MarkdownText>
                            </div>
                            {hoveredMsg === msg.id && (
                                <button
                                    onClick={() => handleCopy(msg.id, getContent(msg))}
                                    title="Copy message"
                                    style={{
                                        position: 'absolute', top: '6px', right: '-32px',
                                        background: 'rgba(30,40,68,0.9)', border: '1px solid var(--border)',
                                        borderRadius: '6px', padding: '4px', cursor: 'pointer',
                                        color: copiedId === msg.id ? 'var(--success)' : 'var(--text-muted)',
                                        display: 'flex', alignItems: 'center', justifyContent: 'center',
                                        transition: 'color 0.2s',
                                    }}
                                >
                                    {copiedId === msg.id ? <Check size={14} /> : <Copy size={14} />}
                                </button>
                            )}
                        </div>
                    </div>
                ))}
                {agentTyping && (
                    <div style={{ display: 'flex', justifyContent: 'flex-start', marginBottom: '12px' }}>
                        <div style={{
                            padding: '10px 16px',
                            borderRadius: '16px 16px 16px 4px',
                            background: 'rgba(30,40,68,0.9)', fontSize: '14px',
                            display: 'flex', alignItems: 'center', gap: '8px',
                        }}>
                            <div style={{ fontSize: '11px', color: 'rgba(255,255,255,0.6)', fontWeight: 600 }}>🤖 Agent</div>
                            <Loader2 size={14} style={{ animation: 'spin 1s linear infinite', color: 'var(--primary)' }} />
                            <span style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Thinking...</span>
                        </div>
                    </div>
                )}
                <div ref={bottomRef} />
            </div>

            {/* Input */}
            <div style={{ padding: '16px', borderTop: '1px solid var(--border)', display: 'flex', gap: '8px' }}>
                <input
                    value={input}
                    onChange={e => setInput(e.target.value)}
                    onKeyDown={e => e.key === 'Enter' && handleSend()}
                    placeholder={isGroup ? "Message the group..." : "Type a message..."}
                    disabled={sending}
                    style={{ flex: 1 }}
                />
                <button className="button" onClick={handleSend} disabled={sending || !input.trim()} style={{ padding: '10px 16px' }}>
                    {sending ? <Loader2 size={18} style={{ animation: 'spin 1s linear infinite' }} /> : <Send size={18} />}
                </button>
            </div>

            {/* Add Member Modal */}
            {showAddMember && (
                <div className="modal-overlay" onClick={() => setShowAddMember(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()} style={{ maxWidth: '400px' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                            <h2 style={{ fontSize: '18px', fontWeight: 700 }}>Add Agent to Chat</h2>
                            <button onClick={() => setShowAddMember(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
                                <X size={18} />
                            </button>
                        </div>
                        {nonMemberAgents.length === 0 ? (
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>All agents are already in this chat</p>
                        ) : (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: '4px', maxHeight: '300px', overflowY: 'auto' }}>
                                {nonMemberAgents.map(a => (
                                    <button key={a.id} onClick={() => handleAddMember(a.id)}
                                        style={{
                                            background: 'rgba(255,255,255,0.03)', border: '1px solid var(--border)',
                                            color: 'var(--text)', cursor: 'pointer', padding: '10px 14px',
                                            borderRadius: '8px', textAlign: 'left',
                                            display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                                            transition: 'background 0.15s',
                                        }}
                                        onMouseOver={e => (e.currentTarget.style.background = 'var(--primary-glow)')}
                                        onMouseOut={e => (e.currentTarget.style.background = 'rgba(255,255,255,0.03)')}
                                    >
                                        <div>
                                            <div style={{ fontSize: '13px', fontWeight: 500 }}>{a.name}</div>
                                            <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{a.role} — {a.specialty || 'general'}</div>
                                        </div>
                                        <Plus size={14} style={{ color: 'var(--primary)' }} />
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            )}

            <style>{`
                @keyframes spin {
                    from { transform: rotate(0deg); }
                    to { transform: rotate(360deg); }
                }
            `}</style>
        </div>
    );
}
