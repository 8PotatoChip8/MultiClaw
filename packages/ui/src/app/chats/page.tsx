'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Thread } from '../../lib/types';
import Chat from '../../components/Chat';
import { MessageSquare, Plus } from 'lucide-react';

export default function ChatsPage() {
    const [threads, setThreads] = useState<Thread[]>([]);
    const [selectedThread, setSelectedThread] = useState<string | null>(null);
    const [showCreate, setShowCreate] = useState(false);
    const [newTitle, setNewTitle] = useState('');

    useEffect(() => {
        api.getThreads().then(d => {
            const list = Array.isArray(d) ? d : [];
            setThreads(list);
            if (list.length > 0 && !selectedThread) setSelectedThread(list[0].id);
        });
    }, []);

    const handleCreateThread = async () => {
        const res = await api.createThread({ type: 'DM', title: newTitle || 'New Chat' });
        if (res?.id) {
            setSelectedThread(res.id);
            setShowCreate(false);
            setNewTitle('');
            api.getThreads().then(d => setThreads(Array.isArray(d) ? d : []));
        }
    };

    return (
        <div className="animate-in" style={{ display: 'flex', height: 'calc(100vh - 96px)', gap: '16px' }}>
            <div className="panel" style={{ width: '260px', minWidth: '260px', overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                    <h3 style={{ fontSize: '14px' }}>Threads</h3>
                    <button onClick={() => setShowCreate(true)} style={{ background: 'none', border: 'none', color: 'var(--primary)', cursor: 'pointer' }}><Plus size={18} /></button>
                </div>
                {threads.length === 0 ? (
                    <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>No threads yet</p>
                ) : threads.map(t => (
                    <div key={t.id} onClick={() => setSelectedThread(t.id)} style={{
                        padding: '10px 12px', borderRadius: '8px', cursor: 'pointer', marginBottom: '4px',
                        background: selectedThread === t.id ? 'var(--primary-glow)' : 'transparent',
                        borderLeft: selectedThread === t.id ? '3px solid var(--primary)' : '3px solid transparent',
                        transition: 'all 0.2s',
                    }}>
                        <div style={{ fontSize: '13px', fontWeight: 500 }}>{t.title || 'Untitled'}</div>
                        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{t.type}</div>
                    </div>
                ))}

                {showCreate && (
                    <div style={{ padding: '12px', borderTop: '1px solid var(--border)', marginTop: 'auto' }}>
                        <input value={newTitle} onChange={e => setNewTitle(e.target.value)} placeholder="Thread title" style={{ marginBottom: '8px' }} />
                        <button className="button small" onClick={handleCreateThread} style={{ width: '100%' }}>Create</button>
                    </div>
                )}
            </div>
            <div style={{ flex: 1 }}>
                {selectedThread ? (
                    <Chat threadId={selectedThread} initialMessages={[]} />
                ) : (
                    <div className="panel" style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                        <div style={{ textAlign: 'center' }}>
                            <MessageSquare size={40} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                            <p style={{ color: 'var(--text-muted)' }}>Select or create a thread</p>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
