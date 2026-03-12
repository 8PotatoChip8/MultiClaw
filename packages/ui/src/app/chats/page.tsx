'use client';
import { useEffect, useState, useMemo } from 'react';
import { api } from '../../lib/api';
import { Thread, Agent } from '../../lib/types';
import { useAgentPresence } from '../../lib/ws';
import AgentStatus from '../../components/AgentStatus';
import Chat from '../../components/Chat';
import { MessageSquare, Plus, Users2, Hash, AtSign, Search, X } from 'lucide-react';

type TabType = 'dms' | 'groups';

export default function ChatsPage() {
    const [threads, setThreads] = useState<Thread[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [selectedThread, setSelectedThread] = useState<string | null>(() => {
        if (typeof window !== 'undefined') return sessionStorage.getItem('multiclaw-selected-thread');
        return null;
    });
    const [activeTab, setActiveTab] = useState<TabType>('dms');
    const [showNewGroup, setShowNewGroup] = useState(false);
    const [showAgentPicker, setShowAgentPicker] = useState(false);
    const [searchQuery, setSearchQuery] = useState('');
    const [newGroupTitle, setNewGroupTitle] = useState('');

    const presenceMap = useAgentPresence(agents);
    const agentByName = useMemo(() => {
        const map: Record<string, Agent> = {};
        for (const a of agents) map[a.name] = a;
        return map;
    }, [agents]);

    // Persist selected thread across navigation
    useEffect(() => {
        if (selectedThread) sessionStorage.setItem('multiclaw-selected-thread', selectedThread);
    }, [selectedThread]);

    // Load threads and agents
    useEffect(() => {
        api.getThreads().then(d => {
            const list = Array.isArray(d) ? d : [];
            setThreads(list);
            if (list.length > 0 && !selectedThread) {
                setSelectedThread(list[0].id);
            } else if (selectedThread && list.length > 0 && !list.some(t => t.id === selectedThread)) {
                setSelectedThread(list[0].id);
            }
        });
        api.getAgents().then(d => setAgents(Array.isArray(d) ? d : []));
    }, []);

    // Start a DM with an agent
    const handleStartDM = async (agent: Agent) => {
        setShowAgentPicker(false);
        try {
            const res = await api.getAgentThread(agent.id);
            if (res?.thread_id) {
                setSelectedThread(res.thread_id);
                // Refresh thread list
                const updated = await api.getThreads();
                setThreads(Array.isArray(updated) ? updated : []);
            }
        } catch (e) {
            console.error('Failed to start DM:', e);
        }
    };

    // Create a group chat
    const handleCreateGroup = async () => {
        if (!newGroupTitle) return;
        const res = await api.createThread({ type: 'GROUP', title: newGroupTitle });
        if (res?.id) {
            setSelectedThread(res.id);
            setShowNewGroup(false);
            setNewGroupTitle('');
            const updated = await api.getThreads();
            setThreads(Array.isArray(updated) ? updated : []);
        }
    };

    const filteredAgents = agents.filter(a =>
        a.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
        (a.handle || '').toLowerCase().includes(searchQuery.toLowerCase())
    );

    const dmThreads = threads.filter(t => t.type === 'DM');
    const groupThreads = threads.filter(t => t.type === 'GROUP' || t.type === 'ENGAGEMENT');

    const roleColors: Record<string, string> = {
        MAIN: 'var(--accent)', CEO: 'var(--primary)', MANAGER: 'var(--success)', WORKER: 'var(--text-muted)'
    };

    return (
        <div className="animate-in" style={{ display: 'flex', height: 'calc(100vh - 96px)', gap: '0' }}>
            {/* Left Panel — Contacts & Threads */}
            <div style={{
                width: '280px', minWidth: '280px',
                background: 'rgba(10, 14, 26, 0.6)',
                borderRight: '1px solid var(--border)',
                display: 'flex', flexDirection: 'column',
                borderRadius: '12px 0 0 12px',
            }}>
                {/* Header */}
                <div style={{
                    padding: '16px',
                    borderBottom: '1px solid var(--border)',
                    display: 'flex', justifyContent: 'space-between', alignItems: 'center'
                }}>
                    <h3 style={{ fontSize: '15px', fontWeight: 700 }}>Messages</h3>
                    <div style={{ display: 'flex', gap: '6px' }}>
                        <button
                            onClick={() => setShowAgentPicker(true)}
                            title="New DM"
                            style={{
                                background: 'none', border: 'none', color: 'var(--primary)',
                                cursor: 'pointer', padding: '4px', borderRadius: '6px',
                                transition: 'background 0.2s'
                            }}
                        >
                            <AtSign size={16} />
                        </button>
                        <button
                            onClick={() => setShowNewGroup(true)}
                            title="New Group"
                            style={{
                                background: 'none', border: 'none', color: 'var(--primary)',
                                cursor: 'pointer', padding: '4px', borderRadius: '6px',
                            }}
                        >
                            <Users2 size={16} />
                        </button>
                    </div>
                </div>

                {/* Tabs */}
                <div style={{ display: 'flex', borderBottom: '1px solid var(--border)' }}>
                    {(['dms', 'groups'] as TabType[]).map(tab => (
                        <button
                            key={tab}
                            onClick={() => setActiveTab(tab)}
                            style={{
                                flex: 1, padding: '10px', border: 'none', cursor: 'pointer',
                                background: activeTab === tab ? 'var(--primary-glow)' : 'transparent',
                                color: activeTab === tab ? 'var(--primary)' : 'var(--text-muted)',
                                fontSize: '12px', fontWeight: 600, textTransform: 'uppercase',
                                letterSpacing: '0.05em',
                                borderBottom: activeTab === tab ? '2px solid var(--primary)' : '2px solid transparent',
                                transition: 'all 0.2s',
                            }}
                        >
                            {tab === 'dms' ? `DMs (${dmThreads.length})` : `Groups (${groupThreads.length})`}
                        </button>
                    ))}
                </div>

                {/* Thread List */}
                <div style={{ flex: 1, overflowY: 'auto', padding: '8px' }}>
                    {activeTab === 'dms' ? (
                        dmThreads.length === 0 ? (
                            <div style={{ textAlign: 'center', padding: '24px 12px' }}>
                                <AtSign size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No DMs yet</p>
                                <button
                                    className="button small"
                                    onClick={() => setShowAgentPicker(true)}
                                    style={{ marginTop: '8px', fontSize: '12px' }}
                                >
                                    Start a conversation
                                </button>
                            </div>
                        ) : dmThreads.map(t => (
                            <div
                                key={t.id}
                                onClick={() => setSelectedThread(t.id)}
                                style={{
                                    padding: '10px 12px', borderRadius: '8px', cursor: 'pointer',
                                    marginBottom: '2px',
                                    background: selectedThread === t.id ? 'var(--primary-glow)' : 'transparent',
                                    borderLeft: selectedThread === t.id ? '3px solid var(--primary)' : '3px solid transparent',
                                    transition: 'all 0.15s',
                                }}
                            >
                                {(() => {
                                    const dmName = (t.title || '').replace(/^DM with /, '');
                                    const agent = agentByName[dmName];
                                    const presence = agent ? presenceMap[agent.id] : undefined;
                                    return (
                                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                                            <div style={{ position: 'relative', flexShrink: 0 }}>
                                                <div style={{
                                                    width: '28px', height: '28px', borderRadius: '50%',
                                                    background: 'linear-gradient(135deg, var(--primary), var(--accent))',
                                                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                                                    fontSize: '11px', fontWeight: 700, color: '#fff',
                                                }}>
                                                    {(dmName || 'D')[0].toUpperCase()}
                                                </div>
                                                {presence && (
                                                    <span style={{
                                                        position: 'absolute', bottom: '-1px', right: '-1px',
                                                        width: '10px', height: '10px', borderRadius: '50%',
                                                        backgroundColor: presence.presenceStatus === 'Busy' ? '#f59e0b' : presence.presenceStatus === 'Active' ? '#22c55e' : '#6b7280',
                                                        border: '2px solid rgba(10, 14, 26, 0.8)',
                                                        animation: presence.presenceStatus === 'Busy' ? 'pulse 2s cubic-bezier(0.4,0,0.6,1) infinite' : 'none',
                                                    }} />
                                                )}
                                            </div>
                                            <div style={{ overflow: 'hidden' }}>
                                                <div style={{ fontSize: '13px', fontWeight: 500, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                                                    {dmName || 'Direct Message'}
                                                </div>
                                            </div>
                                        </div>
                                    );
                                })()}
                            </div>
                        ))
                    ) : (
                        groupThreads.length === 0 ? (
                            <div style={{ textAlign: 'center', padding: '24px 12px' }}>
                                <Hash size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No group chats yet</p>
                                <button
                                    className="button small"
                                    onClick={() => setShowNewGroup(true)}
                                    style={{ marginTop: '8px', fontSize: '12px' }}
                                >
                                    Create a group
                                </button>
                            </div>
                        ) : groupThreads.map(t => (
                            <div
                                key={t.id}
                                onClick={() => setSelectedThread(t.id)}
                                style={{
                                    padding: '10px 12px', borderRadius: '8px', cursor: 'pointer',
                                    marginBottom: '2px',
                                    background: selectedThread === t.id ? 'var(--primary-glow)' : 'transparent',
                                    borderLeft: selectedThread === t.id ? '3px solid var(--primary)' : '3px solid transparent',
                                    transition: 'all 0.15s',
                                }}
                            >
                                <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                                    <Hash size={16} style={{ color: 'var(--accent)' }} />
                                    <div style={{ fontSize: '13px', fontWeight: 500 }}>{t.title || 'Group Chat'}</div>
                                </div>
                            </div>
                        ))
                    )}
                </div>
            </div>

            {/* Right Panel — Chat */}
            <div style={{ flex: 1 }}>
                {selectedThread ? (
                    <Chat
                        threadId={selectedThread}
                        threadType={threads.find(t => t.id === selectedThread)?.type}
                        initialMessages={[]}
                        dmAgent={(() => {
                            const t = threads.find(t => t.id === selectedThread);
                            if (t?.type !== 'DM') return undefined;
                            const title = t.title || '';
                            return agentByName[title] || agentByName[title.replace(/^DM with /, '')];
                        })()}
                        presenceMap={presenceMap}
                    />
                ) : (
                    <div className="panel" style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', borderRadius: '0 12px 12px 0' }}>
                        <div style={{ textAlign: 'center' }}>
                            <MessageSquare size={40} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                            <p style={{ color: 'var(--text-muted)', fontSize: '15px', fontWeight: 500 }}>Select a conversation</p>
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px', marginTop: '4px' }}>or start a new DM with an agent</p>
                        </div>
                    </div>
                )}
            </div>

            {/* Agent Picker Modal */}
            {showAgentPicker && (
                <div className="modal-overlay" onClick={() => setShowAgentPicker(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()} style={{ maxWidth: '420px' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                            <h2 style={{ fontSize: '18px', fontWeight: 700 }}>Message an Agent</h2>
                            <button onClick={() => setShowAgentPicker(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
                                <X size={18} />
                            </button>
                        </div>

                        {/* Search */}
                        <div style={{ position: 'relative', marginBottom: '16px' }}>
                            <Search size={14} style={{ position: 'absolute', left: '10px', top: '50%', transform: 'translateY(-50%)', color: 'var(--text-muted)' }} />
                            <input
                                value={searchQuery}
                                onChange={e => setSearchQuery(e.target.value)}
                                placeholder="Search by name or handle..."
                                style={{ paddingLeft: '32px' }}
                                autoFocus
                            />
                        </div>

                        {/* Agent list */}
                        <div style={{ maxHeight: '320px', overflowY: 'auto' }}>
                            {filteredAgents.map(agent => (
                                <div
                                    key={agent.id}
                                    onClick={() => handleStartDM(agent)}
                                    style={{
                                        padding: '10px 12px', borderRadius: '8px', cursor: 'pointer',
                                        display: 'flex', alignItems: 'center', gap: '12px',
                                        marginBottom: '2px',
                                        transition: 'background 0.15s',
                                    }}
                                    onMouseOver={e => (e.currentTarget.style.background = 'var(--primary-glow)')}
                                    onMouseOut={e => (e.currentTarget.style.background = 'transparent')}
                                >
                                    <div style={{
                                        width: '36px', height: '36px', borderRadius: '50%',
                                        background: `linear-gradient(135deg, ${roleColors[agent.role] || 'var(--primary)'}, var(--accent))`,
                                        display: 'flex', alignItems: 'center', justifyContent: 'center',
                                        fontSize: '14px', fontWeight: 700, color: '#fff', flexShrink: 0,
                                    }}>
                                        {agent.name[0].toUpperCase()}
                                    </div>
                                    <div style={{ flex: 1 }}>
                                        <div style={{ fontWeight: 600, fontSize: '14px' }}>{agent.name}</div>
                                        <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
                                            {agent.handle || `${agent.role}`}
                                        </div>
                                    </div>
                                    <AgentStatus presence={presenceMap[agent.id]?.presenceStatus ?? 'Active'} showLabel={true} size={9} />
                                    <span className={`badge ${agent.role === 'CEO' ? 'external' : agent.role === 'MANAGER' ? 'internal' : 'active'}`} style={{ fontSize: '10px' }}>
                                        {agent.role}
                                    </span>
                                </div>
                            ))}
                            {filteredAgents.length === 0 && (
                                <p style={{ color: 'var(--text-muted)', textAlign: 'center', padding: '16px', fontSize: '13px' }}>
                                    No agents found
                                </p>
                            )}
                        </div>
                    </div>
                </div>
            )}

            {/* New Group Modal */}
            {showNewGroup && (
                <div className="modal-overlay" onClick={() => setShowNewGroup(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '20px' }}>
                            <h2 style={{ fontSize: '18px', fontWeight: 700 }}>Create Group Chat</h2>
                            <button onClick={() => setShowNewGroup(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
                                <X size={18} />
                            </button>
                        </div>
                        <div style={{ marginBottom: '16px' }}>
                            <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Group Name</label>
                            <input value={newGroupTitle} onChange={e => setNewGroupTitle(e.target.value)} placeholder="e.g. Team Standup" autoFocus />
                        </div>
                        <button className="button" onClick={handleCreateGroup} disabled={!newGroupTitle} style={{ width: '100%' }}>
                            Create Group
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}
