'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Agent } from '../../lib/types';
import Link from 'next/link';
import { User, Shield, Briefcase, Wrench } from 'lucide-react';

const roleIcons: Record<string, any> = { MAIN: Shield, CEO: Briefcase, MANAGER: User, WORKER: Wrench };
const roleColors: Record<string, string> = { MAIN: 'var(--accent)', CEO: 'var(--primary)', MANAGER: 'var(--success)', WORKER: 'var(--text-muted)' };

export default function AgentsListPage() {
    const [agents, setAgents] = useState<Agent[]>([]);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        api.getAgents().then(d => { setAgents(Array.isArray(d) ? d : []); setLoading(false); }).catch(() => setLoading(false));
    }, []);

    return (
        <div className="animate-in">
            <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '8px' }}>All Agents</h1>
            <p style={{ color: 'var(--text-muted)', marginBottom: '24px', fontSize: '14px' }}>{agents.length} agents across all companies</p>

            {loading ? <p style={{ color: 'var(--text-muted)' }}>Loading...</p> :
                agents.length === 0 ? (
                    <div className="panel" style={{ textAlign: 'center', padding: '60px' }}>
                        <p style={{ color: 'var(--text-muted)' }}>No agents yet. Initialize the system to create the MainAgent.</p>
                    </div>
                ) : (
                    <div className="panel">
                        <table>
                            <thead><tr><th>Name</th><th>Role</th><th>Model</th><th>Status</th><th>VM</th><th></th></tr></thead>
                            <tbody>
                                {agents.map(a => {
                                    const Icon = roleIcons[a.role] || User;
                                    return (
                                        <tr key={a.id}>
                                            <td><div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}><Icon size={16} style={{ color: roleColors[a.role] }} /><span style={{ fontWeight: 600 }}>{a.name}</span></div></td>
                                            <td><span className={`badge ${a.role === 'CEO' ? 'external' : a.role === 'MANAGER' ? 'internal' : 'active'}`}>{a.role}</span></td>
                                            <td style={{ fontSize: '13px', color: 'var(--text-muted)' }}>{a.effective_model}</td>
                                            <td><span className={`badge ${a.status === 'ACTIVE' ? 'active' : 'quarantined'}`}>{a.status}</span></td>
                                            <td style={{ fontSize: '12px', color: 'var(--text-muted)' }}>{a.vm_id ? '●' : '—'}</td>
                                            <td><Link href={`/agents/${a.id}`} style={{ fontSize: '13px' }}>Details →</Link></td>
                                        </tr>
                                    );
                                })}
                            </tbody>
                        </table>
                    </div>
                )}
        </div>
    );
}
