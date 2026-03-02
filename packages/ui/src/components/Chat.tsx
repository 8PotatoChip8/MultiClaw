import { useState } from 'react';
import { Message } from '../lib/types';
import { Send } from 'lucide-react';

export default function Chat({ threadId, initialMessages = [] }: { threadId: string, initialMessages?: Message[] }) {
    const [input, setInput] = useState('');
    const [messages, setMessages] = useState<Message[]>(initialMessages);

    const handleSend = () => {
        if (!input.trim()) return;

        // Optimistic UI updates, in reality call api.sendMessage
        setMessages([...messages, {
            id: Math.random().toString(),
            sender_type: 'USER',
            sender_id: 'me',
            content: input,
            created_at: new Date().toISOString()
        }]);

        setInput('');
    };

    return (
        <div style={{ display: 'flex', flexDirection: 'column', height: '600px', border: '1px solid var(--border)', borderRadius: '8px' }}>
            <div style={{ flex: 1, overflowY: 'auto', padding: '20px', display: 'flex', flexDirection: 'column', gap: '15px' }}>
                {messages.map(m => (
                    <div key={m.id} style={{ alignSelf: m.sender_type === 'USER' ? 'flex-end' : 'flex-start', background: m.sender_type === 'USER' ? 'var(--primary)' : 'var(--panel)', padding: '10px 15px', borderRadius: '8px', maxWidth: '70%' }}>
                        <div style={{ fontSize: '0.8rem', marginBottom: '5px', opacity: 0.8 }}>{m.sender_type}</div>
                        <div>{typeof m.content === 'object' ? JSON.stringify(m.content) : String(m.content)}</div>
                    </div>
                ))}
            </div>
            <div style={{ padding: '15px', borderTop: '1px solid var(--border)', display: 'flex', gap: '10px', background: 'var(--panel)' }}>
                <input
                    style={{ flex: 1, padding: '10px', background: 'transparent', border: '1px solid var(--border)', color: 'white', borderRadius: '4px' }}
                    value={input}
                    onChange={e => setInput(e.target.value)}
                    onKeyDown={e => e.key === 'Enter' && handleSend()}
                    placeholder="Type a message..."
                />
                <button className="button" onClick={handleSend}><Send size={18} /></button>
            </div>
        </div>
    );
}
