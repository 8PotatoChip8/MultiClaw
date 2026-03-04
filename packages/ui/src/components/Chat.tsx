'use client';
import { useEffect, useState, useRef } from 'react';
import { api } from '../lib/api';
import { useMultiClawEvents } from '../lib/ws';
import { Message } from '../lib/types';
import { Send, Loader2 } from 'lucide-react';

interface ChatProps {
    threadId: string;
    initialMessages: any[];
}

export default function Chat({ threadId, initialMessages }: ChatProps) {
    const [messages, setMessages] = useState<Message[]>([]);
    const [input, setInput] = useState('');
    const [sending, setSending] = useState(false);
    const [agentTyping, setAgentTyping] = useState(false);
    const bottomRef = useRef<HTMLDivElement>(null);
    const event = useMultiClawEvents();

    useEffect(() => {
        api.getMessages(threadId).then(d => {
            setMessages(Array.isArray(d) ? d : []);
        });
    }, [threadId]);

    // Listen for new_message events via WebSocket
    useEffect(() => {
        if (!event) return;
        try {
            const data = typeof event === 'string' ? JSON.parse(event) : event;
            if (data.type === 'new_message' && data.message?.thread_id === threadId) {
                const newMsg = data.message as Message;
                setMessages(prev => {
                    // Avoid duplicates
                    if (prev.some(m => m.id === newMsg.id)) return prev;
                    return [...prev, newMsg];
                });
                // If agent responded, stop typing indicator
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

        // Optimistic add
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
            // Refresh to get the real message with proper ID
            const updated = await api.getMessages(threadId);
            setMessages(Array.isArray(updated) ? updated : []);
        } catch (e) {
            console.error(e);
            setAgentTyping(false);
            setSending(false);
        }

        // Start polling for agent response (fallback if WebSocket misses it)
        const startTime = Date.now();
        const pollInterval = setInterval(async () => {
            if (Date.now() - startTime > 120000) { // 2 min timeout
                clearInterval(pollInterval);
                setAgentTyping(false);
                setSending(false);
                return;
            }
            try {
                const msgs = await api.getMessages(threadId);
                if (Array.isArray(msgs)) {
                    setMessages(msgs);
                    // Check if agent responded
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

    const getContent = (msg: Message) => {
        if (typeof msg.content === 'string') return msg.content;
        if (msg.content?.text) return msg.content.text;
        return JSON.stringify(msg.content);
    };

    const getSenderLabel = (msg: Message) => {
        if (msg.sender_type === 'USER') return 'You';
        if (msg.sender_type === 'AGENT') return '🤖 Agent';
        return msg.sender_type;
    };

    return (
        <div className="panel" style={{ height: '100%', display: 'flex', flexDirection: 'column', padding: '0' }}>
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
                        <div style={{
                            maxWidth: '70%',
                            padding: '10px 16px',
                            borderRadius: msg.sender_type === 'USER' ? '16px 16px 4px 16px' : '16px 16px 16px 4px',
                            background: msg.sender_type === 'USER' ? 'linear-gradient(135deg, var(--primary), var(--accent))' : 'rgba(30,40,68,0.9)',
                            fontSize: '14px',
                            lineHeight: '1.5',
                        }}>
                            <div style={{ fontSize: '11px', color: 'rgba(255,255,255,0.6)', marginBottom: '4px', fontWeight: 600 }}>
                                {getSenderLabel(msg)}
                            </div>
                            <div style={{ whiteSpace: 'pre-wrap' }}>{getContent(msg)}</div>
                        </div>
                    </div>
                ))}
                {agentTyping && (
                    <div style={{
                        display: 'flex',
                        justifyContent: 'flex-start',
                        marginBottom: '12px',
                    }}>
                        <div style={{
                            padding: '10px 16px',
                            borderRadius: '16px 16px 16px 4px',
                            background: 'rgba(30,40,68,0.9)',
                            fontSize: '14px',
                            display: 'flex',
                            alignItems: 'center',
                            gap: '8px',
                        }}>
                            <div style={{ fontSize: '11px', color: 'rgba(255,255,255,0.6)', fontWeight: 600 }}>
                                🤖 Agent
                            </div>
                            <Loader2 size={14} style={{ animation: 'spin 1s linear infinite', color: 'var(--primary)' }} />
                            <span style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Thinking...</span>
                        </div>
                    </div>
                )}
                <div ref={bottomRef} />
            </div>
            <div style={{ padding: '16px', borderTop: '1px solid var(--border)', display: 'flex', gap: '8px' }}>
                <input
                    value={input}
                    onChange={e => setInput(e.target.value)}
                    onKeyDown={e => e.key === 'Enter' && handleSend()}
                    placeholder="Type a message..."
                    disabled={sending}
                    style={{ flex: 1 }}
                />
                <button className="button" onClick={handleSend} disabled={sending || !input.trim()} style={{ padding: '10px 16px' }}>
                    {sending ? <Loader2 size={18} style={{ animation: 'spin 1s linear infinite' }} /> : <Send size={18} />}
                </button>
            </div>
            <style>{`
                @keyframes spin {
                    from { transform: rotate(0deg); }
                    to { transform: rotate(360deg); }
                }
            `}</style>
        </div>
    );
}
