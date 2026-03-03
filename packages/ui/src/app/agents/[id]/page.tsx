'use client';
import { useEffect, useState } from 'react';
import { api } from '../../../lib/api';
import { Agent } from '../../../lib/types';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { Shield, Play, Square, RefreshCw, AlertTriangle, Plus } from 'lucide-react';

export default function AgentDetailPage() {
    const params = useParams();
    const id = params?.id as string;
    const [agent, setAgent] = useState<Agent | null>(null);
    const [showHire, setShowHire] = useState<'manager' | 'worker' | null>(null);
    const [hireName, setHireName] = useState('');
    const [hireSpecialty, setHireSpecialty] = useState('');

    const load = () => { api.getAgent(id).then(d => { if (d && !d.error) setAgent(d); }); };
    useEffect(() => { if (id) load(); }, [id]);

    const handleHire = async () => {
        if (!hireName) return;
        if (showHire === 'manager') await api.hireManager(id, { name: hireName, specialty: hireSpecialty || undefined });
        else await api.hireWorker(id, { name: hireName, specialty: hireSpecialty || undefined });
        setShowHire(null); setHireName(''); setHireSpecialty('');
        load();
    };

    const handlePanic = async () => { if (confirm('PANIC: Quarantine this agent? This will stop its VM and revoke credentials.')) { await api.panic(id); load(); } };

    if (!agent) return <div className="animate-in"><p style={{ color: 'var(--text-muted)' }}>Loading agent...</p></div>;

    return (
        <div className="animate-in" style={{ maxWidth: '800px' }}>
            <div style={{ marginBottom: '24px' }}>
                <Link href="/org" style={{ fontSize: '13px', color: 'var(--text-muted)' }}>← Org Tree</Link>
            </div>

            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '32px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '8px' }}>{agent.name}</h1>
                    <div style={{ display: 'flex', gap: '8px' }}>
                        <span className={`badge ${agent.role === 'CEO' ? 'external' : agent.role === 'MANAGER' ? 'internal' : 'active'}`}>{agent.role}</span>
                        <span className={`badge ${agent.status === 'ACTIVE' ? 'active' : 'quarantined'}`}>{agent.status}</span>
                    </div>
                </div>
                {agent.status !== 'QUARANTINED' && (
                    <button className="button danger small" onClick={handlePanic} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                        <AlertTriangle size={14} /> Panic
                    </button>
                )}
            </div>

            <div className="panel" style={{ marginBottom: '16px' }}>
                <h3 style={{ marginBottom: '16px' }}>Details</h3>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
                    {[
                        ['Model', agent.effective_model], ['Specialty', agent.specialty || '—'],
                        ['VM', agent.vm_id || 'None'], ['Created', new Date(agent.created_at).toLocaleDateString()],
                    ].map(([label, value]) => (
                        <div key={label as string}>
                            <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px' }}>{label}</div>
                            <div style={{ fontSize: '14px', fontWeight: 500 }}>{value}</div>
                        </div>
                    ))}
                </div>
            </div>

            {agent.vm_id && (
                <div className="panel" style={{ marginBottom: '16px' }}>
                    <h3 style={{ marginBottom: '16px' }}>VM Controls</h3>
                    <div style={{ display: 'flex', gap: '8px' }}>
                        <button className="button small secondary" onClick={() => api.vmStart(id)} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}><Play size={14} /> Start</button>
                        <button className="button small secondary" onClick={() => api.vmStop(id)} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}><Square size={14} /> Stop</button>
                        <button className="button small secondary" onClick={() => api.vmRebuild(id)} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}><RefreshCw size={14} /> Rebuild</button>
                    </div>
                </div>
            )}

            {(agent.role === 'CEO' || agent.role === 'MANAGER') && (
                <div className="panel">
                    <h3 style={{ marginBottom: '12px' }}>Hire Staff</h3>
                    <div style={{ display: 'flex', gap: '8px' }}>
                        {agent.role === 'CEO' && (
                            <button className="button small" onClick={() => setShowHire('manager')} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                <Plus size={14} /> Hire Manager
                            </button>
                        )}
                        {agent.role === 'MANAGER' && (
                            <button className="button small" onClick={() => setShowHire('worker')} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                <Plus size={14} /> Hire Worker
                            </button>
                        )}
                    </div>
                </div>
            )}

            {showHire && (
                <div className="modal-overlay" onClick={() => setShowHire(null)}>
                    <div className="modal" onClick={e => e.stopPropagation()}>
                        <h2 style={{ fontSize: '20px', fontWeight: 700, marginBottom: '20px' }}>Hire {showHire === 'manager' ? 'Manager' : 'Worker'}</h2>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Name</label>
                                <input value={hireName} onChange={e => setHireName(e.target.value)} placeholder="Agent name" />
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Specialty</label>
                                <input value={hireSpecialty} onChange={e => setHireSpecialty(e.target.value)} placeholder="e.g. Sales, Engineering" />
                            </div>
                            <button className="button" onClick={handleHire} disabled={!hireName}>Hire</button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
