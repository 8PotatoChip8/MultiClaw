'use client';
import { useEffect, useState, useRef } from 'react';
import { api } from '../lib/api';
import { Message } from '../lib/types';
import { Send } from 'lucide-react';

interface ChatProps {
    threadId: string;
    initialMessages: any[];
}

export default function Chat({ threadId, initialMessages }: ChatProps) {
    const [messages, setMessages] = useState<Message[]>([]);
    const [input, setInput] = useState('');
    const [sending, setSending] = useState(false);
    const bottomRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        api.getMessages(threadId).then(d => {
            setMessages(Array.isArray(d) ? d : []);
        });
    }, [threadId]);

    useEffect(() => {
        bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [messages]);

    const handleSend = async () => {
        if (!input.trim() || sending) return;
        setSending(true);
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
            const updated = await api.getMessages(threadId);
            setMessages(Array.isArray(updated) ? updated : []);
        } catch (e) { console.error(e); }
        setSending(false);
    };

    const getContent = (msg: Message) => {
        if (typeof msg.content === 'string') return msg.content;
        if (msg.content?.text) return msg.content.text;
        return JSON.stringify(msg.content);
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
                                {msg.sender_type}
                            </div>
                            {getContent(msg)}
                        </div>
                    </div>
                ))}
                <div ref={bottomRef} />
            </div>
            <div style={{ padding: '16px', borderTop: '1px solid var(--border)', display: 'flex', gap: '8px' }}>
                <input
                    value={input}
                    onChange={e => setInput(e.target.value)}
                    onKeyDown={e => e.key === 'Enter' && handleSend()}
                    placeholder="Type a message..."
                    style={{ flex: 1 }}
                />
                <button className="button" onClick={handleSend} disabled={sending || !input.trim()} style={{ padding: '10px 16px' }}>
                    <Send size={18} />
                </button>
            </div>
        </div>
    );
}
