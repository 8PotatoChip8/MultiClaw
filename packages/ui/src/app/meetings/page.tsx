'use client';
import { useEffect, useState, useRef } from 'react';
import { api } from '../../lib/api';
import { Meeting, Message, Agent } from '../../lib/types';
import { Calendar, Users, Lock, Clock, FileText, ArrowDownToLine } from 'lucide-react';
import MarkdownText from '../../components/MarkdownText';
import { useMultiClawEvents } from '../../lib/ws';

interface Participant { thread_id: string; member_type: string; member_id: string; }

export default function MeetingsPage() {
    const [meetings, setMeetings] = useState<Meeting[]>([]);
    const [selectedMeeting, setSelectedMeeting] = useState<Meeting | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);
    const [participants, setParticipants] = useState<Participant[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [tab, setTab] = useState<'SCHEDULED' | 'ACTIVE' | 'CLOSED'>('ACTIVE');
    const [loading, setLoading] = useState(true);
    const [autoScroll, setAutoScroll] = useState(true);
    const feedRef = useRef<HTMLDivElement>(null);
    const agentMap = new Map(agents.map(a => [a.id, a]));
    const lastEvent = useMultiClawEvents();

    const loadMeetings = () => {
        Promise.all([api.getMeetings(), api.getAgents()]).then(([m, a]) => {
            setAgents(Array.isArray(a) ? a : []);
            setMeetings(Array.isArray(m) ? m : []);
            setLoading(false);
        }).catch(() => setLoading(false));
    };

    useEffect(() => { loadMeetings(); }, []);

    // Real-time updates
    useEffect(() => {
        if (!lastEvent) return;
        if (lastEvent.type === 'meeting_created' || lastEvent.type === 'meeting_closed' || lastEvent.type === 'meeting_started') {
            loadMeetings();
            // Refresh selected meeting if it was affected
            if (selectedMeeting && lastEvent.meeting?.id === selectedMeeting.id) {
                api.getMeeting(selectedMeeting.id).then(m => setSelectedMeeting(m));
            }
        }
        if (lastEvent.type === 'new_message' && selectedMeeting) {
            const msg = lastEvent.message;
            if (msg && msg.thread_id === selectedMeeting.thread_id) {
                setMessages(prev => {
                    if (prev.some(m => m.id === msg.id)) return prev;
                    return [...prev, msg];
                });
            }
        }
    }, [lastEvent]);

    // Load messages when meeting is selected
    useEffect(() => {
        if (!selectedMeeting) { setMessages([]); setParticipants([]); return; }
        api.getMessages(selectedMeeting.thread_id).then(d => setMessages(Array.isArray(d) ? d : []));
        api.getThreadParticipants(selectedMeeting.thread_id).then(d => setParticipants(Array.isArray(d) ? d : []));
        const interval = setInterval(() => {
            if (selectedMeeting.status === 'ACTIVE') {
                api.getMessages(selectedMeeting.thread_id).then(d => setMessages(Array.isArray(d) ? d : []));
            }
        }, 10000);
        return () => clearInterval(interval);
    }, [selectedMeeting?.id]);

    useEffect(() => {
        if (autoScroll) feedRef.current?.scrollTo(0, feedRef.current.scrollHeight);
    }, [messages, autoScroll]);

    const filtered = meetings.filter(m => m.status === tab);
    const participantAgents = participants.filter(p => p.member_type === 'AGENT');

    const statusColor = (s: string) => {
        if (s === 'ACTIVE') return 'var(--success)';
        if (s === 'SCHEDULED') return 'var(--accent)';
        return 'var(--text-muted)';
    };

    const statusIcon = (s: string) => {
        if (s === 'ACTIVE') return <Users size={10} />;
        if (s === 'SCHEDULED') return <Clock size={10} />;
        return <Lock size={10} />;
    };

    return (
        <div className="animate-in">
            <div style={{ marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '4px' }}>
                    <h1 style={{ fontSize: '28px', fontWeight: 700 }}>Meetings</h1>
                </div>
                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Monitor agent meetings, summaries, and transcripts</p>
            </div>

            <div style={{ display: 'flex', gap: '16px', height: 'calc(100vh - 160px)' }}>
                {/* Left panel: meeting list */}
                <div className="panel" style={{ width: '300px', minWidth: '300px', display: 'flex', flexDirection: 'column' }}>
                    {/* Tabs */}
                    <div style={{ display: 'flex', gap: '4px', marginBottom: '12px' }}>
                        {(['ACTIVE', 'SCHEDULED', 'CLOSED'] as const).map(t => (
                            <button key={t} onClick={() => setTab(t)} style={{
                                flex: 1, padding: '6px 8px', borderRadius: '6px', fontSize: '12px',
                                fontWeight: 600, border: 'none', cursor: 'pointer',
                                background: tab === t ? 'var(--primary-glow)' : 'transparent',
                                color: tab === t ? 'var(--accent)' : 'var(--text-muted)',
                            }}>
                                {t === 'ACTIVE' ? 'Active' : t === 'SCHEDULED' ? 'Scheduled' : 'Closed'}
                                <span style={{ marginLeft: '4px', fontSize: '10px', opacity: 0.7 }}>
                                    ({meetings.filter(m => m.status === t).length})
                                </span>
                            </button>
                        ))}
                    </div>

                    <div style={{ flex: 1, overflowY: 'auto' }}>
                        {loading ? (
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px', padding: '12px' }}>Loading...</p>
                        ) : filtered.length === 0 ? (
                            <div style={{ textAlign: 'center', padding: '24px 12px' }}>
                                <Calendar size={24} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No {tab.toLowerCase()} meetings</p>
                            </div>
                        ) : filtered.map(m => (
                            <div key={m.id} onClick={() => {
                                setSelectedMeeting(m);
                            }} style={{
                                padding: '10px 12px', borderRadius: '8px', cursor: 'pointer', marginBottom: '2px',
                                background: selectedMeeting?.id === m.id ? 'var(--primary-glow)' : 'transparent',
                                borderLeft: selectedMeeting?.id === m.id ? '3px solid var(--accent)' : '3px solid transparent',
                                transition: 'all 0.15s',
                            }}>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                    {statusIcon(m.status)}
                                    <span style={{ fontSize: '13px', fontWeight: 500, flex: 1 }}>{m.topic}</span>
                                </div>
                                <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: '4px' }}>
                                    <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                        {agentMap.get(m.organizer_id)?.name || 'Unknown'}
                                    </span>
                                    <span style={{
                                        fontSize: '10px', padding: '1px 6px', borderRadius: '8px',
                                        background: `${statusColor(m.status)}20`, color: statusColor(m.status),
                                        fontWeight: 600,
                                    }}>
                                        {m.status}
                                    </span>
                                </div>
                                <div style={{ fontSize: '10px', color: 'var(--text-muted)', marginTop: '2px' }}>
                                    {m.status === 'SCHEDULED' && m.scheduled_for
                                        ? `Scheduled: ${new Date(m.scheduled_for).toLocaleString()}`
                                        : new Date(m.created_at).toLocaleString()
                                    }
                                </div>
                            </div>
                        ))}
                    </div>
                </div>

                {/* Right panel: meeting detail */}
                <div className="panel" style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
                    {selectedMeeting ? (
                        <>
                            {/* Header */}
                            <div style={{ paddingBottom: '12px', borderBottom: '1px solid var(--border)', marginBottom: '12px' }}>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
                                    <Calendar size={16} style={{ color: 'var(--accent)' }} />
                                    <h2 style={{ fontSize: '16px', fontWeight: 600 }}>{selectedMeeting.topic}</h2>
                                    <span style={{
                                        fontSize: '10px', padding: '2px 8px', borderRadius: '8px',
                                        background: `${statusColor(selectedMeeting.status)}20`,
                                        color: statusColor(selectedMeeting.status), fontWeight: 600,
                                    }}>
                                        {selectedMeeting.status}
                                    </span>
                                </div>
                                <div style={{ fontSize: '12px', color: 'var(--text-muted)', display: 'flex', gap: '16px' }}>
                                    <span>Organizer: {agentMap.get(selectedMeeting.organizer_id)?.name || 'Unknown'}</span>
                                    <span>Participants: {participantAgents.map(p => agentMap.get(p.member_id)?.name || '?').join(', ')}</span>
                                </div>
                                {selectedMeeting.status === 'SCHEDULED' && selectedMeeting.scheduled_for && (
                                    <div style={{ fontSize: '12px', color: 'var(--accent)', marginTop: '4px' }}>
                                        <Clock size={12} style={{ verticalAlign: 'middle', marginRight: '4px' }} />
                                        Scheduled for: {new Date(selectedMeeting.scheduled_for).toLocaleString()}
                                    </div>
                                )}
                                {selectedMeeting.status === 'CLOSED' && selectedMeeting.closed_at && (
                                    <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '4px' }}>
                                        <Lock size={12} style={{ verticalAlign: 'middle', marginRight: '4px' }} />
                                        Closed: {new Date(selectedMeeting.closed_at).toLocaleString()}
                                    </div>
                                )}
                            </div>

                            {/* Summary (if closed) */}
                            {selectedMeeting.status === 'CLOSED' && selectedMeeting.summary && (
                                <div style={{
                                    padding: '12px 16px', borderRadius: '8px', marginBottom: '12px',
                                    background: 'rgba(99,102,241,0.08)', border: '1px solid rgba(99,102,241,0.2)',
                                }}>
                                    <div style={{ display: 'flex', alignItems: 'center', gap: '6px', marginBottom: '8px' }}>
                                        <FileText size={14} style={{ color: 'var(--accent)' }} />
                                        <span style={{ fontSize: '13px', fontWeight: 600, color: 'var(--accent)' }}>Meeting Summary</span>
                                    </div>
                                    <div style={{ fontSize: '13px', lineHeight: '1.5' }}>
                                        <MarkdownText text={selectedMeeting.summary} />
                                    </div>
                                </div>
                            )}

                            {/* Auto-scroll toggle */}
                            {selectedMeeting.status === 'ACTIVE' && (
                                <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: '4px' }}>
                                    <button onClick={() => setAutoScroll(!autoScroll)} style={{
                                        display: 'flex', alignItems: 'center', gap: '4px',
                                        fontSize: '11px', padding: '2px 8px', borderRadius: '4px',
                                        border: 'none', cursor: 'pointer',
                                        background: autoScroll ? 'rgba(99,102,241,0.15)' : 'transparent',
                                        color: autoScroll ? 'var(--accent)' : 'var(--text-muted)',
                                    }}>
                                        <ArrowDownToLine size={10} />
                                        Auto-scroll
                                    </button>
                                </div>
                            )}

                            {/* Message feed */}
                            <div ref={feedRef} style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: '4px' }}>
                                {selectedMeeting.status === 'SCHEDULED' ? (
                                    <div style={{ textAlign: 'center', padding: '40px 20px' }}>
                                        <Clock size={32} style={{ color: 'var(--accent)', marginBottom: '12px' }} />
                                        <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>This meeting hasn&apos;t started yet.</p>
                                        {selectedMeeting.scheduled_for && (
                                            <p style={{ color: 'var(--accent)', fontSize: '13px', marginTop: '4px' }}>
                                                Scheduled for {new Date(selectedMeeting.scheduled_for).toLocaleString()}
                                            </p>
                                        )}
                                    </div>
                                ) : messages.length === 0 ? (
                                    <p style={{ color: 'var(--text-muted)', fontSize: '13px', padding: '12px' }}>No messages yet</p>
                                ) : messages.map(msg => {
                                    const sender = agentMap.get(msg.sender_id);
                                    const text = typeof msg.content === 'object' ? msg.content?.text : msg.content;
                                    const isSystem = msg.sender_type === 'SYSTEM';
                                    return (
                                        <div key={msg.id} style={{
                                            padding: '8px 12px', borderRadius: '8px',
                                            background: isSystem ? 'rgba(99,102,241,0.06)' : 'rgba(255,255,255,0.02)',
                                            borderLeft: isSystem ? '3px solid var(--accent)' : '3px solid transparent',
                                        }}>
                                            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '2px' }}>
                                                <span style={{
                                                    fontSize: '12px', fontWeight: 600,
                                                    color: isSystem ? 'var(--accent)' : 'var(--text)',
                                                }}>
                                                    {isSystem ? 'SYSTEM' : sender?.name || 'Unknown'}
                                                    {sender && (
                                                        <span style={{ fontWeight: 400, color: 'var(--text-muted)', marginLeft: '6px', fontSize: '11px' }}>
                                                            {sender.role}
                                                        </span>
                                                    )}
                                                </span>
                                                <span style={{ fontSize: '10px', color: 'var(--text-muted)' }}>
                                                    {new Date(msg.created_at).toLocaleTimeString()}
                                                </span>
                                            </div>
                                            <div style={{ fontSize: '13px', lineHeight: '1.4' }}>
                                                <MarkdownText text={text || ''} />
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>
                        </>
                    ) : (
                        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%' }}>
                            <div style={{ textAlign: 'center' }}>
                                <Calendar size={32} style={{ color: 'var(--text-muted)', marginBottom: '8px' }} />
                                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Select a meeting to view details</p>
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
