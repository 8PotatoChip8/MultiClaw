'use client';
import { useEffect, useState } from 'react';
import { api } from '../../../lib/api';
import { Company, Agent } from '../../../lib/types';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { Users2, Plus } from 'lucide-react';

export default function CompanyDetailPage() {
    const params = useParams();
    const id = params?.id as string;
    const [company, setCompany] = useState<Company | null>(null);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [showHireCeo, setShowHireCeo] = useState(false);
    const [ceoName, setCeoName] = useState('');

    useEffect(() => {
        if (!id) return;
        api.getCompany(id).then(d => { if (d && !d.error) setCompany(d); });
        api.getOrgTree(id).then(d => { if (d?.tree) setAgents(d.tree); });
    }, [id]);

    const handleHireCeo = async () => {
        if (!ceoName) return;
        await api.hireCeo(id, { name: ceoName });
        setCeoName('');
        setShowHireCeo(false);
        api.getOrgTree(id).then(d => { if (d?.tree) setAgents(d.tree); });
    };

    if (!company) return <div className="animate-in"><p style={{ color: 'var(--text-muted)' }}>Loading...</p></div>;

    const ceos = agents.filter(a => a.role === 'CEO');
    const managers = agents.filter(a => a.role === 'MANAGER');
    const workers = agents.filter(a => a.role === 'WORKER');

    return (
        <div className="animate-in">
            <div style={{ marginBottom: '24px' }}>
                <Link href="/companies" style={{ fontSize: '13px', color: 'var(--text-muted)' }}>← Companies</Link>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '32px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '8px' }}>{company.name}</h1>
                    <div style={{ display: 'flex', gap: '8px' }}>
                        <span className={`badge ${company.type === 'INTERNAL' ? 'internal' : 'external'}`}>{company.type}</span>
                        <span className="badge active">{company.status}</span>
                    </div>
                    {company.description && <p style={{ color: 'var(--text-muted)', marginTop: '12px', fontSize: '14px' }}>{company.description}</p>}
                </div>
                <button className="button" onClick={() => setShowHireCeo(true)} style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <Plus size={16} /> Hire CEO
                </button>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '16px', marginBottom: '24px' }}>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '24px', fontWeight: 700 }}>{ceos.length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>CEOs</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '24px', fontWeight: 700 }}>{managers.length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Managers</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '24px', fontWeight: 700 }}>{workers.length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Workers</div>
                </div>
            </div>

            <div className="panel">
                <h3 style={{ marginBottom: '16px' }}>Staff</h3>
                {agents.length === 0 ? (
                    <p style={{ color: 'var(--text-muted)' }}>No agents yet. Hire a CEO to get started.</p>
                ) : (
                    <table>
                        <thead><tr><th>Name</th><th>Role</th><th>Model</th><th>Status</th><th></th></tr></thead>
                        <tbody>
                            {agents.map(a => (
                                <tr key={a.id}>
                                    <td style={{ fontWeight: 600 }}>{a.name}</td>
                                    <td><span className={`badge ${a.role === 'CEO' ? 'external' : a.role === 'MANAGER' ? 'internal' : 'active'}`}>{a.role}</span></td>
                                    <td style={{ fontSize: '13px', color: 'var(--text-muted)' }}>{a.effective_model}</td>
                                    <td><span className={`badge ${a.status === 'ACTIVE' ? 'active' : 'quarantined'}`}>{a.status}</span></td>
                                    <td><Link href={`/agents/${a.id}`} style={{ fontSize: '13px' }}>Details →</Link></td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                )}
            </div>

            {showHireCeo && (
                <div className="modal-overlay" onClick={() => setShowHireCeo(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()}>
                        <h2 style={{ fontSize: '20px', fontWeight: 700, marginBottom: '20px' }}>Hire CEO</h2>
                        <div style={{ marginBottom: '16px' }}>
                            <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>CEO Name</label>
                            <input value={ceoName} onChange={e => setCeoName(e.target.value)} placeholder="Agent name" />
                        </div>
                        <button className="button" onClick={handleHireCeo} disabled={!ceoName}>Hire CEO</button>
                    </div>
                </div>
            )}
        </div>
    );
}
