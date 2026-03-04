'use client';
import { useEffect, useState, useRef } from 'react';
import { api } from '../../lib/api';
import { Thread, Message, Agent } from '../../lib/types';
import { Radio, MessageSquare, Plus, Users, X, Send, UserMinus } from 'lucide-react';

interface Participant { thread_id: string; member_type: string; member_id: string; }

export default function MessagingPage() {
    const [threads, setThreads] = useState<Thread[]>([]);
    const [selectedThread, setSelectedThread] = useState<string | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);
    const [participants, setParticipants] = useState<Participant[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [loading, setLoading] = useState(true);
    const [inputText, setInputText] = useState('');
    const [sending, setSending] = useState(false);
    const [showCreateGroup, setShowCreateGroup] = useState(false);
    const [showAddMember, setShowAddMember] = useState(false);
    const [groupTitle, setGroupTitle] = useState('');
    const [selectedAgents, setSelectedAgents] = useState<string[]>([]);
    const feedRef = useRef<HTMLDivElement>(null);
    const agentMap = new Map(agents.map(a => [a.id, a]));

    useEffect(() => {
        api.getThreads().then(d => { setThreads(Array.isArray(d) ? d : []); setLoading(false); }).catch(() => setLoading(false));
        api.getAgents().then(d => setAgents(Array.isArray(d) ? d : []));
    }, []);

    useEffect(() => {
        if (!selectedThread) { setMessages([]); setParticipants([]); return; }
        api.getMessages(selectedThread).then(d => setMessages(Array.isArray(d) ? d : []));
        api.getThreadParticipants(selectedThread).then(d => setParticipants(Array.isArray(d) ? d : []));
    }, [selectedThread]);

    useEffect(() => { feedRef.current?.scrollTo(0, feedRef.current.scrollHeight); }, [messages]);

    const refreshMessages = () => {
        if (selectedThread) {
            api.getMessages(selectedThread).then(d => setMessages(Array.isArray(d) ? d : []));
        }
    };

    const handleSend = async () => {
        if (!inputText.trim() || !selectedThread || sending) return;
        setSending(true);
        await api.sendMessage(selectedThread, { content: { text: inputText }, sender_type: 'USER' });
        setInputText('');
        // Poll for response
        refreshMessages();
        setTimeout(refreshMessages, 3000);
        setTimeout(refreshMessages, 8000);
        setTimeout(refreshMessages, 15000);
        setTimeout(() => setSending(false), 2000);
    };

    const handleCreateGroup = async () => {
        if (!groupTitle || selectedAgents.length === 0) return;
        const res = await api.createThread({ type: 'GROUP', title: groupTitle });
        if (res?.id) {
            for (const aid of selectedAgents) {
                await api.addParticipant(res.id, { member_id: aid, member_type: 'AGENT' });
            }
            setShowCreateGroup(false);
            setGroupTitle('');
            setSelectedAgents([]);
            const updated = await api.getThreads();
            setThreads(Array.isArray(updated) ? updated : []);
            setSelectedThread(res.id);
        }
    };

    const handleAddMember = async (agentId: string) => {
        if (!selectedThread) return;
        await api.addParticipant(selectedThread, { member_id: agentId, member_type: 'AGENT' });
        api.getThreadParticipants(selectedThread).then(d => setParticipants(Array.isArray(d) ? d : []));
        setShowAddMember(false);
    };

    const handleRemoveMember = async (memberId: string) => {
        if (!selectedThread) return;
        await api.removeParticipant(selectedThread, memberId);
        api.getThreadParticipants(selectedThread).then(d => setParticipants(Array.isArray(d) ? d : []));
    };

    const selectedThreadData = threads.find(t => t.id === selectedThread);
    const agentParticipants = participants.filter(p => p.member_type === 'AGENT');
    const nonMemberAgents = agents.filter(a => !agentParticipants.some(p => p.member_id === a.id));

    return (
        <div className="animate-in">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>Agent Communications</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Chat with agents and manage group conversations</p>
                </div>
                <button className="button small" onClick={() => setShowCreateGroup(true)}
                    style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                    <Users size={14} /> New Group Chat
                </button>
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
                        <div key={t.id} onClick={() => setSelectedThread(t.id)} style={{
                            padding: '10px 12px', borderRadius: '8px', cursor: 'pointer', marginBottom: '2px',
                            background: selectedThread === t.id ? 'var(--primary-glow)' : 'transparent',
                            borderLeft: selectedThread === t.id ? '3px solid var(--accent)' : '3px solid transparent',
                            transition: 'all 0.15s',
                        }}>
                            <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                {t.type === 'GROUP' && <Users size={12} style={{ color: 'var(--accent)' }} />}
                                <span style={{ fontSize: '13px', fontWeight: 500 }}>{t.title || 'Thread'}</span>
                            </div>
                            <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>{t.type}</div>
                        </div>
                    ))}
                </div>

                {/* Message Feed + Input */}
                <div className="panel" style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
                    {selectedThread ? (
                        <>
                            {/* Thread Header */}
                            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', paddingBottom: '12px', borderBottom: '1px solid var(--border)', marginBottom: '12px' }}>
                                <div>
                                    <h3 style={{ fontSize: '14px', fontWeight: 600, display: 'flex', alignItems: 'center', gap: '8px' }}>
                                        <Radio size={14} style={{ color: 'var(--accent)' }} />
                                        {selectedThreadData?.title || 'Thread'}
                                        {selectedThreadData?.type === 'GROUP' && (
                                            <span style={{ fontSize: '10px', padding: '2px 8px', borderRadius: '10px', background: 'var(--accent)', color: '#fff', fontWeight: 600 }}>GROUP</span>
                                        )}
                                    </h3>
                                    {agentParticipants.length > 0 && (
                                        <div style={{ display: 'flex', gap: '6px', marginTop: '6px', flexWrap: 'wrap' }}>
                                            {agentParticipants.map(p => {
                                                const a = agentMap.get(p.member_id);
                                                return (
                                                    <div key={p.member_id} style={{
                                                        display: 'flex', alignItems: 'center', gap: '4px',
                                                        fontSize: '11px', padding: '2px 8px', borderRadius: '12px',
                                                        background: 'rgba(255,255,255,0.06)', color: 'var(--text-muted)',
                                                    }}>
                                                        {a?.name || p.member_id.slice(0, 8)}
                                                        {selectedThreadData?.type === 'GROUP' && (
                                                            <button onClick={() => handleRemoveMember(p.member_id)} style={{
                                                                background: 'none', border: 'none', color: '#ef4444',
                                                                cursor: 'pointer', padding: '0 2px', opacity: 0.6,
                                                            }} title="Remove from chat"><UserMinus size={10} /></button>
                                                        )}
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    )}
                                </div>
                                {selectedThreadData?.type === 'GROUP' && (
                                    <button className="button small secondary" onClick={() => setShowAddMember(true)}
                                        style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                        <Plus size={12} /> Add
                                    </button>
                                )}
                            </div>

                            {/* Messages */}
                            <div ref={feedRef} style={{ flex: 1, overflowY: 'auto', paddingBottom: '12px' }}>
                                {messages.length === 0 ? (
                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px', textAlign: 'center', padding: '40px' }}>No messages yet — send one below</p>
                                ) : messages.map(m => {
                                    const isUser = m.sender_type === 'USER';
                                    const senderAgent = !isUser ? agentMap.get(m.sender_id) : null;
                                    return (
                                        <div key={m.id} style={{
                                            padding: '12px', marginBottom: '8px',
                                            background: isUser ? 'rgba(99,102,241,0.1)' : 'rgba(0,0,0,0.2)',
                                            borderRadius: '8px',
                                            borderLeft: `3px solid ${isUser ? 'var(--primary)' : 'var(--accent)'}`,
                                        }}>
                                            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '6px' }}>
                                                <span style={{ fontSize: '12px', fontWeight: 600, color: isUser ? 'var(--primary)' : 'var(--accent)' }}>
                                                    {isUser ? 'You' : senderAgent?.name || `Agent ${m.sender_id.slice(0, 8)}`}
                                                    {senderAgent?.role && (
                                                        <span style={{ marginLeft: '6px', fontSize: '10px', opacity: 0.6 }}>[{senderAgent.role}]</span>
                                                    )}
                                                </span>
                                                <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                                    {new Date(m.created_at).toLocaleTimeString()}
                                                </span>
                                            </div>
                                            <div style={{ fontSize: '13px', lineHeight: '1.6', whiteSpace: 'pre-wrap' }}>
                                                {typeof m.content === 'object' ? m.content?.text || JSON.stringify(m.content) : String(m.content)}
                                            </div>
                                        </div>
                                    );
                                })}
                                {sending && (
                                    <div style={{ padding: '12px', textAlign: 'center' }}>
                                        <span style={{ color: 'var(--text-muted)', fontSize: '13px' }}>⏳ Waiting for agent response...</span>
                                    </div>
                                )}
                            </div>

                            {/* Input */}
                            <div style={{ borderTop: '1px solid var(--border)', paddingTop: '12px', display: 'flex', gap: '8px' }}>
                                <input
                                    value={inputText}
                                    onChange={e => setInputText(e.target.value)}
                                    onKeyDown={e => e.key === 'Enter' && !e.shiftKey && handleSend()}
                                    placeholder={selectedThreadData?.type === 'GROUP' ? 'Message the group (mention an agent name to direct)...' : 'Type a message...'}
                                    disabled={sending}
                                    style={{ flex: 1 }}
                                />
                                <button className="button" onClick={handleSend} disabled={sending || !inputText.trim()}
                                    style={{ display: 'flex', alignItems: 'center', gap: '6px', minWidth: '80px', justifyContent: 'center' }}>
                                    <Send size={14} /> Send
                                </button>
                            </div>
                        </>
                    ) : (
                        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                            <div style={{ textAlign: 'center' }}>
                                <MessageSquare size={36} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Select a thread or create a group chat</p>
                            </div>
                        </div>
                    )}
                </div>
            </div>

            {/* Create Group Modal */}
            {showCreateGroup && (
                <div className="modal-overlay" onClick={() => setShowCreateGroup(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()} style={{ maxWidth: '480px' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '20px' }}>
                            <h2 style={{ fontSize: '20px', fontWeight: 700 }}>New Group Chat</h2>
                            <button onClick={() => setShowCreateGroup(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}><X size={20} /></button>
                        </div>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Group Name</label>
                                <input value={groupTitle} onChange={e => setGroupTitle(e.target.value)} placeholder="e.g. Strategy Planning" />
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Select Agents</label>
                                <div style={{ maxHeight: '200px', overflowY: 'auto', borderRadius: '8px', border: '1px solid var(--border)' }}>
                                    {agents.map(a => (
                                        <label key={a.id} style={{
                                            display: 'flex', alignItems: 'center', gap: '10px',
                                            padding: '10px 12px', cursor: 'pointer',
                                            background: selectedAgents.includes(a.id) ? 'var(--primary-glow)' : 'transparent',
                                            borderBottom: '1px solid var(--border)',
                                        }}>
                                            <input type="checkbox" checked={selectedAgents.includes(a.id)}
                                                onChange={e => {
                                                    if (e.target.checked) setSelectedAgents([...selectedAgents, a.id]);
                                                    else setSelectedAgents(selectedAgents.filter(id => id !== a.id));
                                                }} />
                                            <div>
                                                <div style={{ fontSize: '13px', fontWeight: 500 }}>{a.name}</div>
                                                <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{a.role} — {a.specialty || 'general'}</div>
                                            </div>
                                        </label>
                                    ))}
                                </div>
                            </div>
                            <button className="button" onClick={handleCreateGroup}
                                disabled={!groupTitle || selectedAgents.length === 0}>
                                Create Group ({selectedAgents.length} agents)
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Add Member Modal */}
            {showAddMember && (
                <div className="modal-overlay" onClick={() => setShowAddMember(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()} style={{ maxWidth: '400px' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '20px' }}>
                            <h2 style={{ fontSize: '18px', fontWeight: 700 }}>Add Agent to Chat</h2>
                            <button onClick={() => setShowAddMember(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}><X size={20} /></button>
                        </div>
                        {nonMemberAgents.length === 0 ? (
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>All agents are already in this chat</p>
                        ) : (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                                {nonMemberAgents.map(a => (
                                    <button key={a.id} onClick={() => handleAddMember(a.id)}
                                        className="button secondary small" style={{
                                            textAlign: 'left', display: 'flex', justifyContent: 'space-between',
                                            alignItems: 'center', padding: '10px 14px',
                                        }}>
                                        <div>
                                            <div style={{ fontSize: '13px', fontWeight: 500 }}>{a.name}</div>
                                            <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{a.role}</div>
                                        </div>
                                        <Plus size={14} />
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
