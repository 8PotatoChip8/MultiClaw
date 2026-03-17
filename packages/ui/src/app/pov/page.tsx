'use client';
import { useEffect, useState, useRef, useMemo, useCallback } from 'react';
import { api } from '../../lib/api';
import { Agent, Company, Thread, Message, Meeting, Request as MCRequest, FileTransfer } from '../../lib/types';
import {
    Eye, User, Shield, Briefcase, Wrench, MessageSquare, Calendar, FileText,
    ArrowDownToLine, Users, Clock, Send, Inbox, HardDrive, Activity, Radio
} from 'lucide-react';
import MarkdownText from '../../components/MarkdownText';
import { useMultiClawEvents, useAgentPresence } from '../../lib/ws';
import AgentStatus from '../../components/AgentStatus';

interface ActivityBlock {
    status: 'WORKING' | 'IDLE';
    task: string | null;
    start: number;
    end: number | null;
}

type RightTab = 'conversations' | 'recent' | 'requests' | 'files' | 'meetings' | 'vms';

const roleIcons: Record<string, any> = { MAIN: Shield, CEO: Briefcase, MANAGER: User, WORKER: Wrench };
const roleColors: Record<string, string> = { MAIN: 'var(--accent)', CEO: 'var(--primary)', MANAGER: 'var(--success)', WORKER: 'var(--text-muted)' };

function timeAgo(dateStr: string): string {
    const diff = Date.now() - new Date(dateStr).getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return 'just now';
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `${hrs}h ago`;
    return `${Math.floor(hrs / 24)}d ago`;
}

function durationSince(dateStr: string): string {
    const diff = Date.now() - new Date(dateStr).getTime();
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m ${secs % 60}s`;
    const hrs = Math.floor(mins / 60);
    return `${hrs}h ${mins % 60}m`;
}

export default function AgentPOVPage() {
    const [agents, setAgents] = useState<Agent[]>([]);
    const [companies, setCompanies] = useState<Company[]>([]);
    const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);

    const [agentThreads, setAgentThreads] = useState<Thread[]>([]);
    const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
    const [threadMessages, setThreadMessages] = useState<Message[]>([]);
    const [recentMessages, setRecentMessages] = useState<Message[]>([]);
    const [requests, setRequests] = useState<MCRequest[]>([]);
    const [fileTransfers, setFileTransfers] = useState<FileTransfer[]>([]);
    const [meetings, setMeetings] = useState<Meeting[]>([]);
    const [vmInfo, setVmInfo] = useState<Record<string, any>>({});

    const [rightTab, setRightTab] = useState<RightTab>('conversations');
    const [autoScroll, setAutoScroll] = useState(true);
    const feedRef = useRef<HTMLDivElement>(null);
    const selectedAgentIdRef = useRef<string | null>(null);
    const selectedThreadIdRef = useRef<string | null>(null);
    selectedAgentIdRef.current = selectedAgentId;
    selectedThreadIdRef.current = selectedThreadId;

    const [activityTimeline, setActivityTimeline] = useState<Record<string, ActivityBlock[]>>({});

    const agentMap = useMemo(() => new Map(agents.map(a => [a.id, a])), [agents]);
    const companyMap = useMemo(() => new Map(companies.map(c => [c.id, c])), [companies]);
    const selectedAgent = selectedAgentId ? agentMap.get(selectedAgentId) ?? null : null;
    const presenceMap = useAgentPresence(agents);
    const lastEvent = useMultiClawEvents();

    // --- Initial load ---
    useEffect(() => {
        Promise.all([api.getAgents(), api.getCompanies()]).then(([a, c]) => {
            const agentList = Array.isArray(a) ? a : [];
            setAgents(agentList);
            setCompanies(Array.isArray(c) ? c : []);
            setLoading(false);
        }).catch(() => setLoading(false));
    }, []);

    // --- Load data for selected agent (no dependency on agents array) ---
    const loadAgentData = useCallback((agentId: string) => {
        api.getAgentThreads(agentId).then(d => setAgentThreads(Array.isArray(d) ? d : []));
        api.getAgentRecentMessages(agentId, 50).then(d => setRecentMessages(Array.isArray(d) ? d : []));
        api.getAgentFileTransfers(agentId).then(d => setFileTransfers(Array.isArray(d) ? d : []));
        api.getRequests().then(d => setRequests(Array.isArray(d) ? d : []));
        api.getMeetings().then(d => setMeetings(Array.isArray(d) ? d : []));

        // Fetch agent detail for VM info (avoids depending on agents state)
        api.getAgent(agentId).then(a => {
            if (a && !a.error) {
                if (a.vm_id) api.vmInfo(agentId, 'desktop').then((d: any) => setVmInfo(prev => ({ ...prev, desktop: d?.error ? null : d })));
                else setVmInfo(prev => ({ ...prev, desktop: null }));
                if (a.sandbox_vm_id) api.vmInfo(agentId, 'sandbox').then((d: any) => setVmInfo(prev => ({ ...prev, sandbox: d?.error ? null : d })));
                else setVmInfo(prev => ({ ...prev, sandbox: null }));
            }
        });
    }, []);

    // Reset thread selection only when switching agents
    useEffect(() => {
        if (selectedAgentId) {
            loadAgentData(selectedAgentId);
            setSelectedThreadId(null);
            setThreadMessages([]);
        }
    }, [selectedAgentId, loadAgentData]);

    // --- Load thread messages ---
    useEffect(() => {
        if (!selectedThreadId) { setThreadMessages([]); return; }
        api.getMessages(selectedThreadId).then(d => setThreadMessages(Array.isArray(d) ? d : []));
        const interval = setInterval(() => {
            api.getMessages(selectedThreadId).then(d => setThreadMessages(Array.isArray(d) ? d : []));
        }, 10000);
        return () => clearInterval(interval);
    }, [selectedThreadId]);

    useEffect(() => {
        if (autoScroll && feedRef.current) feedRef.current.scrollTo(0, feedRef.current.scrollHeight);
    }, [threadMessages, autoScroll]);

    // --- Real-time events ---
    useEffect(() => {
        if (!lastEvent) return;
        const curAgentId = selectedAgentIdRef.current;
        const curThreadId = selectedThreadIdRef.current;

        if (lastEvent.type === 'new_message' && lastEvent.message) {
            const msg = lastEvent.message;
            if (curThreadId && msg.thread_id === curThreadId) {
                setThreadMessages(prev => {
                    if (prev.some(m => m.id === msg.id)) return prev;
                    return [...prev, msg];
                });
            }
            if (curAgentId && msg.sender_id === curAgentId) {
                api.getAgentRecentMessages(curAgentId, 50).then(d =>
                    setRecentMessages(Array.isArray(d) ? d : []));
                // Refresh thread list too (new threads may appear)
                api.getAgentThreads(curAgentId).then(d =>
                    setAgentThreads(Array.isArray(d) ? d : []));
            }
        }

        if (lastEvent.type === 'agent_activity_changed' && lastEvent.agent_id) {
            const aid = lastEvent.agent_id;
            const now = Date.now();
            setActivityTimeline(prev => {
                const existing = prev[aid] || [];
                const updated = [...existing];
                if (updated.length > 0 && updated[updated.length - 1].end === null) {
                    updated[updated.length - 1] = { ...updated[updated.length - 1], end: now };
                }
                updated.push({
                    status: lastEvent.status === 'WORKING' ? 'WORKING' : 'IDLE',
                    task: lastEvent.task || null,
                    start: now,
                    end: null,
                });
                return { ...prev, [aid]: updated };
            });
            // Refresh agent data to get updated activity field
            api.getAgent(aid).then(a => {
                if (a && !a.error) {
                    setAgents(prev => prev.map(ag => ag.id === aid ? a : ag));
                }
            });
        }

        if (lastEvent.type === 'new_request' || lastEvent.type === 'request_approved' || lastEvent.type === 'request_rejected') {
            api.getRequests().then(d => setRequests(Array.isArray(d) ? d : []));
        }

        if (lastEvent.type === 'meeting_created' || lastEvent.type === 'meeting_closed' || lastEvent.type === 'meeting_started') {
            api.getMeetings().then(d => setMeetings(Array.isArray(d) ? d : []));
        }

        // New agent hired — refresh agent list so sidebar updates
        if (lastEvent.type === 'ceo_hired' || lastEvent.type === 'agent_hired') {
            api.getAgents().then(a => {
                if (Array.isArray(a)) setAgents(a);
            });
        }
    }, [lastEvent]);

    // --- Derived data ---
    const agentRequests = useMemo(() => {
        if (!selectedAgentId) return { submitted: [] as MCRequest[], pending: [] as MCRequest[] };
        return {
            submitted: requests.filter(r => r.created_by_agent_id === selectedAgentId),
            pending: requests.filter(r => r.current_approver_id === selectedAgentId && r.status === 'PENDING'),
        };
    }, [requests, selectedAgentId]);

    const agentMeetings = useMemo(() => {
        if (!selectedAgentId) return [];
        const agentThreadIds = new Set(agentThreads.filter(t => t.type === 'MEETING').map(t => t.id));
        return meetings.filter(m => agentThreadIds.has(m.thread_id) || m.organizer_id === selectedAgentId);
    }, [meetings, agentThreads, selectedAgentId]);

    const commGraph = useMemo(() => {
        if (!selectedAgentId || recentMessages.length === 0) return [];
        const threadCounts: Record<string, number> = {};
        for (const m of recentMessages) {
            threadCounts[m.thread_id] = (threadCounts[m.thread_id] || 0) + 1;
        }
        return Object.entries(threadCounts)
            .map(([tid, count]) => ({
                threadId: tid,
                title: agentThreads.find(t => t.id === tid)?.title || 'Thread',
                count,
            }))
            .sort((a, b) => b.count - a.count)
            .slice(0, 8);
    }, [recentMessages, agentThreads, selectedAgentId]);

    const messageVolume = useMemo(() => {
        if (recentMessages.length === 0) return [];
        const now = Date.now();
        const hours: number[] = new Array(12).fill(0);
        for (const m of recentMessages) {
            const age = now - new Date(m.created_at).getTime();
            const hourIndex = Math.floor(age / 3600000);
            if (hourIndex < 12) hours[11 - hourIndex]++;
        }
        return hours;
    }, [recentMessages]);

    const getParentName = (agent: Agent): string => {
        if (!agent.parent_agent_id) return agent.role === 'MAIN' ? 'Operator (you)' : '—';
        return agentMap.get(agent.parent_agent_id)?.name || '—';
    };

    const getCompanyName = (agent: Agent): string => {
        if (!agent.company_id) return agent.role === 'MAIN' ? 'Holding' : '—';
        return companyMap.get(agent.company_id)?.name || '—';
    };

    const threadIcon = (type: string) => {
        if (type === 'DM') return <MessageSquare size={12} style={{ color: 'var(--accent)' }} />;
        if (type === 'MEETING') return <Calendar size={12} style={{ color: 'var(--accent)' }} />;
        return <Users size={12} style={{ color: 'var(--accent)' }} />;
    };

    return (
        <div className="animate-in">
            <div style={{ marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '4px' }}>
                    <h1 style={{ fontSize: '28px', fontWeight: 700 }}>Agent POV</h1>
                    <span style={{
                        fontSize: '10px', padding: '3px 10px', borderRadius: '12px',
                        background: 'rgba(99,102,241,0.15)', color: 'var(--primary)',
                        fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.05em',
                    }}>
                        <Eye size={10} style={{ marginRight: '4px', verticalAlign: 'middle' }} />
                        Read-Only
                    </span>
                </div>
                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Observe any agent&apos;s activity, conversations, and work status in real-time</p>
            </div>

            <div style={{ display: 'flex', gap: '16px', height: 'calc(100vh - 180px)' }}>
                {/* Agent Selector Sidebar */}
                <div className="panel" style={{ width: '220px', minWidth: '220px', overflowY: 'auto' }}>
                    <h3 style={{
                        fontSize: '13px', fontWeight: 600, color: 'var(--text-muted)', marginBottom: '12px',
                        textTransform: 'uppercase', letterSpacing: '0.05em',
                    }}>
                        Agents ({agents.length})
                    </h3>
                    {loading ? (
                        <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Loading...</p>
                    ) : agents.map(a => {
                        const Icon = roleIcons[a.role] || User;
                        return (
                            <div key={a.id} onClick={() => setSelectedAgentId(a.id)} style={{
                                padding: '8px 10px', borderRadius: '8px', cursor: 'pointer', marginBottom: '2px',
                                background: selectedAgentId === a.id ? 'var(--primary-glow)' : 'transparent',
                                borderLeft: selectedAgentId === a.id ? '3px solid var(--accent)' : '3px solid transparent',
                                transition: 'all 0.15s',
                                display: 'flex', alignItems: 'center', gap: '8px',
                            }}>
                                <Icon size={14} style={{ color: roleColors[a.role], flexShrink: 0 }} />
                                <div style={{ flex: 1, minWidth: 0 }}>
                                    <div style={{ fontSize: '13px', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.name}</div>
                                    <div style={{ fontSize: '10px', color: 'var(--text-muted)' }}>{a.role}</div>
                                </div>
                                <AgentStatus presence={presenceMap[a.id]?.presenceStatus ?? 'Active'} showLabel={false} size={7} />
                            </div>
                        );
                    })}
                </div>

                {/* POV Content */}
                {selectedAgent ? (
                    <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: '12px' }}>
                        {/* Identity Card */}
                        <div className="panel" style={{ padding: '20px' }}>
                            <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '8px' }}>
                                <h2 style={{ fontSize: '22px', fontWeight: 700 }}>{selectedAgent.name}</h2>
                                <span className={`badge ${selectedAgent.role === 'CEO' ? 'external' : selectedAgent.role === 'MANAGER' ? 'internal' : 'active'}`}>
                                    {selectedAgent.role}
                                </span>
                                <AgentStatus presence={presenceMap[selectedAgent.id]?.presenceStatus ?? 'Active'} showLabel={true} size={9} />
                            </div>
                            <div style={{ display: 'flex', gap: '16px', fontSize: '12px', color: 'var(--text-muted)', flexWrap: 'wrap' }}>
                                <span>Company: <strong style={{ color: 'var(--text)' }}>{getCompanyName(selectedAgent)}</strong></span>
                                <span>Reports to: <strong style={{ color: 'var(--text)' }}>{getParentName(selectedAgent)}</strong></span>
                                <span>Model: <strong style={{ color: 'var(--text)' }}>{selectedAgent.effective_model}</strong></span>
                                {selectedAgent.handle && <span>Handle: <code style={{ color: 'var(--accent)', fontSize: '12px' }}>{selectedAgent.handle}</code></span>}
                                {selectedAgent.specialty && <span>Specialty: <strong style={{ color: 'var(--text)' }}>{selectedAgent.specialty}</strong></span>}
                            </div>
                        </div>

                        {/* Activity Status Widget */}
                        {selectedAgent.activity && (
                            <div className="panel" style={{
                                padding: '12px 20px',
                                borderLeft: `3px solid ${selectedAgent.activity.status === 'WORKING' ? 'var(--warning)' : 'var(--success)'}`,
                            }}>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
                                    <div style={{
                                        width: '10px', height: '10px', borderRadius: '50%',
                                        background: selectedAgent.activity.status === 'WORKING' ? 'var(--warning)' : 'var(--success)',
                                        animation: selectedAgent.activity.status === 'WORKING' ? 'pulse 2s cubic-bezier(0.4,0,0.6,1) infinite' : 'none',
                                        flexShrink: 0,
                                    }} />
                                    <div style={{ flex: 1 }}>
                                        <span style={{ fontSize: '13px', fontWeight: 600 }}>
                                            {selectedAgent.activity.status === 'WORKING' ? 'Working' : 'Idle'}
                                        </span>
                                        {selectedAgent.activity.task && (
                                            <span style={{ fontSize: '13px', color: 'var(--text-muted)', marginLeft: '8px' }}>
                                                — {selectedAgent.activity.task}
                                            </span>
                                        )}
                                    </div>
                                    <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                        for {durationSince(selectedAgent.activity.since)}
                                    </span>
                                </div>
                            </div>
                        )}

                        {/* Main two-column layout */}
                        <div style={{ display: 'flex', gap: '12px', flex: 1, minHeight: 0 }}>
                            {/* Left column: Visualizations */}
                            <div style={{ width: '300px', minWidth: '300px', display: 'flex', flexDirection: 'column', gap: '12px' }}>
                                {/* Activity Timeline */}
                                <div className="panel" style={{ padding: '16px' }}>
                                    <h4 style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: '12px' }}>
                                        <Activity size={12} style={{ marginRight: '6px', verticalAlign: 'middle' }} />
                                        Activity Timeline
                                    </h4>
                                    <div style={{ display: 'flex', height: '24px', borderRadius: '4px', overflow: 'hidden', background: 'rgba(0,0,0,0.3)' }}>
                                        {(activityTimeline[selectedAgentId!] || []).map((block, i) => {
                                            const blocks = activityTimeline[selectedAgentId!] || [];
                                            const totalSpan = Date.now() - (blocks[0]?.start || Date.now());
                                            const blockDuration = (block.end || Date.now()) - block.start;
                                            const widthPct = totalSpan > 0 ? (blockDuration / totalSpan) * 100 : 100;
                                            return (
                                                <div key={i} title={`${block.status}${block.task ? ': ' + block.task : ''}`} style={{
                                                    width: `${Math.max(widthPct, 1)}%`,
                                                    background: block.status === 'WORKING' ? 'var(--warning)' : 'var(--success)',
                                                    opacity: 0.7,
                                                    transition: 'width 0.3s',
                                                }} />
                                            );
                                        })}
                                    </div>
                                    {(activityTimeline[selectedAgentId!] || []).length === 0 && (
                                        <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '6px', textAlign: 'center' }}>
                                            Activity will appear as events arrive during this session
                                        </div>
                                    )}
                                    <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: '6px', fontSize: '10px', color: 'var(--text-muted)' }}>
                                        <span style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                            <span style={{ width: '8px', height: '8px', borderRadius: '2px', background: 'var(--success)', opacity: 0.7 }} /> Idle
                                        </span>
                                        <span style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                            <span style={{ width: '8px', height: '8px', borderRadius: '2px', background: 'var(--warning)', opacity: 0.7 }} /> Working
                                        </span>
                                    </div>
                                </div>

                                {/* Communication Graph */}
                                <div className="panel" style={{ padding: '16px' }}>
                                    <h4 style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: '12px' }}>
                                        Communication Partners
                                    </h4>
                                    {commGraph.length === 0 ? (
                                        <p style={{ fontSize: '11px', color: 'var(--text-muted)', textAlign: 'center' }}>No recent communication data</p>
                                    ) : (
                                        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                                            {commGraph.map(item => {
                                                const maxCount = commGraph[0].count;
                                                const widthPct = (item.count / maxCount) * 100;
                                                return (
                                                    <div key={item.threadId}>
                                                        <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '11px', marginBottom: '2px' }}>
                                                            <span style={{ color: 'var(--text)', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: '200px' }}>{item.title}</span>
                                                            <span style={{ color: 'var(--text-muted)', flexShrink: 0 }}>{item.count}</span>
                                                        </div>
                                                        <div style={{ height: '6px', borderRadius: '3px', background: 'rgba(0,0,0,0.3)' }}>
                                                            <div style={{
                                                                height: '100%', borderRadius: '3px', width: `${widthPct}%`,
                                                                background: 'linear-gradient(90deg, var(--primary), var(--accent))',
                                                                transition: 'width 0.3s',
                                                            }} />
                                                        </div>
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    )}
                                </div>

                                {/* Message Volume Sparkline */}
                                <div className="panel" style={{ padding: '16px' }}>
                                    <h4 style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: '12px' }}>
                                        Message Volume (12h)
                                    </h4>
                                    {messageVolume.length === 0 ? (
                                        <p style={{ fontSize: '11px', color: 'var(--text-muted)', textAlign: 'center' }}>No message data</p>
                                    ) : (
                                        <>
                                            <div style={{ display: 'flex', alignItems: 'flex-end', gap: '3px', height: '48px' }}>
                                                {messageVolume.map((count, i) => {
                                                    const maxVal = Math.max(...messageVolume, 1);
                                                    const heightPct = (count / maxVal) * 100;
                                                    return (
                                                        <div key={i} title={`${12 - i}h ago: ${count} messages`} style={{
                                                            flex: 1, borderRadius: '2px 2px 0 0',
                                                            height: `${Math.max(heightPct, 4)}%`,
                                                            background: count > 0 ? 'var(--accent)' : 'rgba(255,255,255,0.06)',
                                                            opacity: count > 0 ? 0.7 : 0.3,
                                                            transition: 'height 0.3s',
                                                        }} />
                                                    );
                                                })}
                                            </div>
                                            <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '9px', color: 'var(--text-muted)', marginTop: '4px' }}>
                                                <span>12h ago</span>
                                                <span>now</span>
                                            </div>
                                        </>
                                    )}
                                </div>
                            </div>

                            {/* Right column: Tabbed content */}
                            <div className="panel" style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
                                {/* Tab bar */}
                                <div style={{ display: 'flex', gap: '4px', marginBottom: '12px', flexWrap: 'wrap' }}>
                                    {([
                                        { key: 'conversations' as RightTab, label: 'Conversations', icon: MessageSquare },
                                        { key: 'recent' as RightTab, label: 'Recent', icon: Send },
                                        { key: 'requests' as RightTab, label: 'Requests', icon: Inbox },
                                        { key: 'files' as RightTab, label: 'Files', icon: FileText },
                                        { key: 'meetings' as RightTab, label: 'Meetings', icon: Calendar },
                                        { key: 'vms' as RightTab, label: 'VMs', icon: HardDrive },
                                    ]).map(t => (
                                        <button key={t.key} onClick={() => setRightTab(t.key)} style={{
                                            padding: '6px 12px', borderRadius: '6px', fontSize: '12px',
                                            fontWeight: 600, border: 'none', cursor: 'pointer',
                                            background: rightTab === t.key ? 'var(--primary-glow)' : 'transparent',
                                            color: rightTab === t.key ? 'var(--accent)' : 'var(--text-muted)',
                                            display: 'flex', alignItems: 'center', gap: '5px',
                                        }}>
                                            <t.icon size={12} /> {t.label}
                                        </button>
                                    ))}
                                </div>

                                {/* Tab content */}
                                <div style={{ flex: 1, overflowY: 'auto', minHeight: 0 }}>
                                    {rightTab === 'conversations' && (
                                        <div style={{ display: 'flex', gap: '12px', height: '100%' }}>
                                            {/* Thread list */}
                                            <div style={{ width: '200px', minWidth: '200px', overflowY: 'auto', borderRight: '1px solid var(--border)', paddingRight: '12px' }}>
                                                {agentThreads.length === 0 ? (
                                                    <p style={{ color: 'var(--text-muted)', fontSize: '12px', textAlign: 'center', padding: '20px 0' }}>No threads</p>
                                                ) : agentThreads.map(t => (
                                                    <div key={t.id} onClick={() => setSelectedThreadId(t.id)} style={{
                                                        padding: '8px 10px', borderRadius: '6px', cursor: 'pointer', marginBottom: '2px',
                                                        background: selectedThreadId === t.id ? 'rgba(99,102,241,0.15)' : 'transparent',
                                                        borderLeft: selectedThreadId === t.id ? '2px solid var(--accent)' : '2px solid transparent',
                                                        transition: 'all 0.15s',
                                                    }}>
                                                        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                                            {threadIcon(t.type)}
                                                            <span style={{ fontSize: '12px', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                                                {t.title || 'Thread'}
                                                            </span>
                                                        </div>
                                                        <div style={{ fontSize: '10px', color: 'var(--text-muted)', marginTop: '2px' }}>{t.type}</div>
                                                    </div>
                                                ))}
                                            </div>

                                            {/* Message feed */}
                                            <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
                                                {selectedThreadId ? (
                                                    <>
                                                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
                                                            <span style={{ fontSize: '13px', fontWeight: 600 }}>
                                                                {agentThreads.find(t => t.id === selectedThreadId)?.title || 'Thread'}
                                                            </span>
                                                            <button onClick={() => setAutoScroll(!autoScroll)} style={{
                                                                display: 'flex', alignItems: 'center', gap: '4px', fontSize: '11px',
                                                                padding: '2px 8px', borderRadius: '4px', border: 'none', cursor: 'pointer',
                                                                background: autoScroll ? 'rgba(99,102,241,0.15)' : 'transparent',
                                                                color: autoScroll ? 'var(--accent)' : 'var(--text-muted)',
                                                            }}>
                                                                <ArrowDownToLine size={10} /> Auto-scroll
                                                            </button>
                                                        </div>
                                                        <div ref={feedRef} style={{ flex: 1, overflowY: 'auto' }}>
                                                            {threadMessages.length === 0 ? (
                                                                <p style={{ color: 'var(--text-muted)', fontSize: '12px', textAlign: 'center', padding: '20px' }}>No messages</p>
                                                            ) : threadMessages.map(m => {
                                                                const sender = agentMap.get(m.sender_id);
                                                                const isSystem = m.sender_type === 'SYSTEM';
                                                                const isSelected = m.sender_id === selectedAgentId;
                                                                const text = typeof m.content === 'object' ? m.content?.text || JSON.stringify(m.content) : String(m.content);
                                                                return (
                                                                    <div key={m.id} style={{
                                                                        padding: '8px 12px', marginBottom: '4px', borderRadius: '8px',
                                                                        background: isSelected ? 'rgba(139,92,246,0.08)' : isSystem ? 'rgba(255,200,0,0.05)' : 'rgba(0,0,0,0.2)',
                                                                        borderLeft: `3px solid ${isSelected ? 'var(--accent)' : isSystem ? 'var(--warning, #f59e0b)' : 'transparent'}`,
                                                                    }}>
                                                                        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '2px' }}>
                                                                            <span style={{ fontSize: '12px', fontWeight: 600, color: isSelected ? 'var(--accent)' : isSystem ? 'var(--warning, #f59e0b)' : 'var(--text)' }}>
                                                                                {isSystem ? 'SYSTEM' : sender?.name || m.sender_type}
                                                                                {sender?.role && <span style={{ fontWeight: 400, color: 'var(--text-muted)', marginLeft: '6px', fontSize: '10px' }}>[{sender.role}]</span>}
                                                                            </span>
                                                                            <span style={{ fontSize: '10px', color: 'var(--text-muted)' }}>{new Date(m.created_at).toLocaleTimeString()}</span>
                                                                        </div>
                                                                        <div style={{ fontSize: '13px', color: 'var(--text-muted)' }}>
                                                                            <MarkdownText>{text}</MarkdownText>
                                                                        </div>
                                                                    </div>
                                                                );
                                                            })}
                                                        </div>
                                                    </>
                                                ) : (
                                                    <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                                                        <div style={{ textAlign: 'center' }}>
                                                            <Radio size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                                            <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Select a thread to view messages</p>
                                                        </div>
                                                    </div>
                                                )}
                                            </div>
                                        </div>
                                    )}

                                    {rightTab === 'recent' && (
                                        <div>
                                            {recentMessages.length === 0 ? (
                                                <div style={{ textAlign: 'center', padding: '40px' }}>
                                                    <Send size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No recent messages from this agent</p>
                                                </div>
                                            ) : recentMessages.map(m => {
                                                const text = typeof m.content === 'object' ? m.content?.text || JSON.stringify(m.content) : String(m.content);
                                                return (
                                                    <div key={m.id} style={{
                                                        padding: '10px 14px', marginBottom: '4px',
                                                        background: 'rgba(0,0,0,0.2)', borderRadius: '8px',
                                                        borderLeft: '3px solid var(--accent)',
                                                    }}>
                                                        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '4px' }}>
                                                            <span style={{ fontSize: '11px', color: 'var(--accent)', fontWeight: 600 }}>
                                                                {agentThreads.find(t => t.id === m.thread_id)?.title || 'Thread'}
                                                            </span>
                                                            <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{timeAgo(m.created_at)}</span>
                                                        </div>
                                                        <div style={{ fontSize: '13px', color: 'var(--text-muted)' }}>
                                                            <MarkdownText>{text}</MarkdownText>
                                                        </div>
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    )}

                                    {rightTab === 'requests' && (
                                        <div>
                                            <h4 style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: '12px' }}>
                                                Awaiting Approval ({agentRequests.pending.length})
                                            </h4>
                                            {agentRequests.pending.length === 0 ? (
                                                <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '20px' }}>No pending approvals</p>
                                            ) : agentRequests.pending.map(r => (
                                                <div key={r.id} style={{
                                                    padding: '10px 14px', marginBottom: '4px', background: 'rgba(0,0,0,0.2)',
                                                    borderRadius: '8px', borderLeft: '3px solid var(--warning)',
                                                }}>
                                                    <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '4px' }}>
                                                        <span style={{ fontSize: '12px', fontWeight: 600 }}>{r.type}</span>
                                                        <span className="badge pending">{r.status}</span>
                                                    </div>
                                                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
                                                        From: {r.created_by_agent_id ? agentMap.get(r.created_by_agent_id)?.name || 'Agent' : 'User'}
                                                    </div>
                                                    {r.payload?.description && (
                                                        <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '4px' }}>{r.payload.description}</div>
                                                    )}
                                                    <div style={{ fontSize: '10px', color: 'var(--text-muted)', marginTop: '4px' }}>{timeAgo(r.created_at)}</div>
                                                </div>
                                            ))}

                                            <h4 style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: '12px', marginTop: '20px' }}>
                                                Submitted ({agentRequests.submitted.length})
                                            </h4>
                                            {agentRequests.submitted.length === 0 ? (
                                                <p style={{ fontSize: '12px', color: 'var(--text-muted)' }}>No submitted requests</p>
                                            ) : agentRequests.submitted.map(r => (
                                                <div key={r.id} style={{
                                                    padding: '10px 14px', marginBottom: '4px', background: 'rgba(0,0,0,0.2)',
                                                    borderRadius: '8px', borderLeft: `3px solid ${r.status === 'APPROVED' ? 'var(--success)' : r.status === 'PENDING' ? 'var(--warning)' : 'var(--danger)'}`,
                                                }}>
                                                    <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '4px' }}>
                                                        <span style={{ fontSize: '12px', fontWeight: 600 }}>{r.type}</span>
                                                        <span className={`badge ${r.status === 'APPROVED' ? 'active' : r.status === 'PENDING' ? 'pending' : 'quarantined'}`}>{r.status}</span>
                                                    </div>
                                                    {r.payload?.description && (
                                                        <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>{r.payload.description}</div>
                                                    )}
                                                    <div style={{ fontSize: '10px', color: 'var(--text-muted)', marginTop: '4px' }}>{timeAgo(r.created_at)}</div>
                                                </div>
                                            ))}
                                        </div>
                                    )}

                                    {rightTab === 'files' && (
                                        <div>
                                            {fileTransfers.length === 0 ? (
                                                <div style={{ textAlign: 'center', padding: '40px' }}>
                                                    <FileText size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No file transfers</p>
                                                </div>
                                            ) : fileTransfers.map(ft => (
                                                <div key={ft.id} style={{
                                                    padding: '10px 14px', marginBottom: '4px', background: 'rgba(0,0,0,0.2)',
                                                    borderRadius: '8px', display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                                                }}>
                                                    <div>
                                                        <div style={{ fontSize: '13px', fontWeight: 500, display: 'flex', alignItems: 'center', gap: '6px' }}>
                                                            <FileText size={12} style={{ color: 'var(--accent)' }} />
                                                            {ft.filename}
                                                            <span style={{ fontSize: '10px', color: 'var(--text-muted)' }}>
                                                                ({ft.size_bytes < 1024 ? `${ft.size_bytes} B` : `${(ft.size_bytes / 1024).toFixed(1)} KB`})
                                                            </span>
                                                        </div>
                                                        <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>
                                                            {agentMap.get(ft.sender_id)?.name || 'Unknown'} → {agentMap.get(ft.receiver_id)?.name || 'Unknown'}
                                                        </div>
                                                    </div>
                                                    <div style={{ textAlign: 'right' }}>
                                                        <span className={`badge ${ft.status === 'COMPLETED' ? 'active' : ft.status === 'FAILED' ? 'quarantined' : 'pending'}`}>
                                                            {ft.status}
                                                        </span>
                                                        <div style={{ fontSize: '10px', color: 'var(--text-muted)', marginTop: '2px' }}>{timeAgo(ft.created_at)}</div>
                                                    </div>
                                                </div>
                                            ))}
                                        </div>
                                    )}

                                    {rightTab === 'meetings' && (
                                        <div>
                                            {agentMeetings.length === 0 ? (
                                                <div style={{ textAlign: 'center', padding: '40px' }}>
                                                    <Calendar size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No meetings for this agent</p>
                                                </div>
                                            ) : (['ACTIVE', 'SCHEDULED', 'CLOSED'] as const).map(status => {
                                                const filtered = agentMeetings.filter(m => m.status === status);
                                                if (filtered.length === 0) return null;
                                                return (
                                                    <div key={status} style={{ marginBottom: '16px' }}>
                                                        <h4 style={{ fontSize: '11px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: '8px' }}>
                                                            {status} ({filtered.length})
                                                        </h4>
                                                        {filtered.map(m => (
                                                            <div key={m.id} style={{
                                                                padding: '10px 14px', marginBottom: '4px', background: 'rgba(0,0,0,0.2)',
                                                                borderRadius: '8px',
                                                                borderLeft: `3px solid ${status === 'ACTIVE' ? 'var(--success)' : status === 'SCHEDULED' ? 'var(--accent)' : 'var(--text-muted)'}`,
                                                            }}>
                                                                <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '4px' }}>
                                                                    <span style={{ fontSize: '13px', fontWeight: 500 }}>{m.topic}</span>
                                                                    <span style={{
                                                                        fontSize: '10px', padding: '1px 6px', borderRadius: '8px',
                                                                        background: status === 'ACTIVE' ? 'rgba(16,185,129,0.2)' : status === 'SCHEDULED' ? 'rgba(139,92,246,0.2)' : 'rgba(123,139,168,0.2)',
                                                                        color: status === 'ACTIVE' ? 'var(--success)' : status === 'SCHEDULED' ? 'var(--accent)' : 'var(--text-muted)',
                                                                        fontWeight: 600,
                                                                    }}>
                                                                        {status}
                                                                    </span>
                                                                </div>
                                                                <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                                                    Organizer: {agentMap.get(m.organizer_id)?.name || 'Unknown'}
                                                                    {m.scheduled_for && <span style={{ marginLeft: '12px' }}>
                                                                        <Clock size={10} style={{ verticalAlign: 'middle', marginRight: '3px' }} />
                                                                        {new Date(m.scheduled_for).toLocaleString()}
                                                                    </span>}
                                                                </div>
                                                            </div>
                                                        ))}
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    )}

                                    {rightTab === 'vms' && (
                                        <div>
                                            {!selectedAgent.vm_id && !selectedAgent.sandbox_vm_id ? (
                                                <div style={{ textAlign: 'center', padding: '40px' }}>
                                                    <HardDrive size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No VMs assigned to this agent</p>
                                                </div>
                                            ) : (
                                                <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                                                    {(['desktop', 'sandbox'] as const).map(target => {
                                                        const hasVm = target === 'desktop' ? selectedAgent.vm_id : selectedAgent.sandbox_vm_id;
                                                        if (!hasVm) return null;
                                                        const info = vmInfo[target];
                                                        return (
                                                            <div key={target} style={{
                                                                padding: '14px 16px', background: 'rgba(0,0,0,0.2)', borderRadius: '8px',
                                                                borderLeft: `3px solid ${target === 'desktop' ? 'var(--primary)' : 'var(--accent)'}`,
                                                            }}>
                                                                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '8px' }}>
                                                                    <HardDrive size={14} style={{ color: target === 'desktop' ? 'var(--primary)' : 'var(--accent)' }} />
                                                                    <span style={{ fontSize: '13px', fontWeight: 600, textTransform: 'capitalize' }}>{target}</span>
                                                                    {info && (
                                                                        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                                                            <div style={{
                                                                                width: '8px', height: '8px', borderRadius: '50%',
                                                                                background: info.status === 'Running' ? '#22c55e' : '#ef4444',
                                                                            }} />
                                                                            <span style={{ fontSize: '12px', fontWeight: 500 }}>{info.status}</span>
                                                                        </div>
                                                                    )}
                                                                </div>
                                                                {info ? (
                                                                    <div style={{ display: 'flex', gap: '16px', fontSize: '12px', color: 'var(--text-muted)' }}>
                                                                        {info.ip_address && <span>IP: <code style={{ color: 'var(--text)' }}>{info.ip_address}</code></span>}
                                                                        {info.memory_usage_bytes != null && info.memory_total_bytes != null && (
                                                                            <span>RAM: {(info.memory_usage_bytes / 1024 / 1024).toFixed(0)}MB / {(info.memory_total_bytes / 1024 / 1024).toFixed(0)}MB</span>
                                                                        )}
                                                                    </div>
                                                                ) : (
                                                                    <p style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Unable to fetch VM info</p>
                                                                )}
                                                            </div>
                                                        );
                                                    })}
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </div>
                            </div>
                        </div>
                    </div>
                ) : (
                    <div className="panel" style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                        <div style={{ textAlign: 'center' }}>
                            <Eye size={36} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                            <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Select an agent to view their POV</p>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
