'use client';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { useMultiClawEvents } from '../lib/ws';
import { Company, Agent, Request } from '../lib/types';
import Link from 'next/link';
import { Building2, Users2, Activity, Zap } from 'lucide-react';

export default function Home() {
    const event = useMultiClawEvents();
    const [companies, setCompanies] = useState<Company[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [healthy, setHealthy] = useState(false);
    const [pending, setPending] = useState(0);

    useEffect(() => {
        api.health().then(() => setHealthy(true)).catch(() => { });
        api.getCompanies().then(d => setCompanies(Array.isArray(d) ? d : [])).catch(() => { });
        api.getAgents().then(d => setAgents(Array.isArray(d) ? d : [])).catch(() => { });
        api.getRequests('PENDING', 'USER').then(d => setPending(Array.isArray(d) ? d.length : 0)).catch(() => { });
    }, []);

    // Refresh pending count on request events
    useEffect(() => {
        if (event?.type === 'new_request' || event?.type === 'request_approved' || event?.type === 'request_rejected') {
            api.getRequests('PENDING', 'USER').then(d => setPending(Array.isArray(d) ? d.length : 0)).catch(() => { });
        }
    }, [event]);

    const mainAgent = agents.find(a => a.role === 'MAIN');

    return (
        <div className="animate-in">
            <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '8px' }}>Holding Company Overview</h1>
            <p style={{ color: 'var(--text-muted)', marginBottom: '32px' }}>Welcome to your autonomous holding company dashboard.</p>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: '16px', marginBottom: '32px' }}>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <Activity size={28} style={{ color: 'var(--success)', marginBottom: '8px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px' }}>System Status</div>
                    <div style={{ fontSize: '18px', fontWeight: 700, color: healthy ? 'var(--success)' : 'var(--danger)' }}>
                        {healthy ? 'Online' : 'Connecting...'}
                    </div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <Building2 size={28} style={{ color: 'var(--primary)', marginBottom: '8px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px' }}>Companies</div>
                    <div style={{ fontSize: '18px', fontWeight: 700 }}>{companies.length}</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <Users2 size={28} style={{ color: 'var(--accent)', marginBottom: '8px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px' }}>Agents</div>
                    <div style={{ fontSize: '18px', fontWeight: 700 }}>{agents.length}</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <Zap size={28} style={{ color: 'var(--warning)', marginBottom: '8px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px' }}>Pending Approvals</div>
                    <div style={{ fontSize: '18px', fontWeight: 700 }}>{pending}</div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '20px' }}>
                <div className="panel">
                    <h3 style={{ marginBottom: '16px', fontSize: '16px' }}>MainAgent</h3>
                    {mainAgent ? (
                        <div>
                            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '8px' }}>
                                <span style={{ color: 'var(--text-muted)' }}>Name</span>
                                <span style={{ fontWeight: 600 }}>{mainAgent.name}</span>
                            </div>
                            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '8px' }}>
                                <span style={{ color: 'var(--text-muted)' }}>Model</span>
                                <span className="badge active">{mainAgent.effective_model}</span>
                            </div>
                            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                                <span style={{ color: 'var(--text-muted)' }}>Status</span>
                                <span className="badge active">{mainAgent.status}</span>
                            </div>
                            <Link href="/chats" style={{ display: 'block', marginTop: '16px' }}>
                                <button className="button" style={{ width: '100%' }}>Chat with {mainAgent.name}</button>
                            </Link>
                        </div>
                    ) : <p style={{ color: 'var(--text-muted)' }}>Initializing...</p>}
                </div>

                <div className="panel">
                    <h3 style={{ marginBottom: '16px', fontSize: '16px' }}>Recent Events</h3>
                    {event ? (
                        <pre style={{ background: 'rgba(0,0,0,0.3)', padding: '12px', borderRadius: '8px', fontSize: '12px', overflowX: 'auto' }}>
                            {JSON.stringify(event, null, 2)}
                        </pre>
                    ) : (
                        <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Waiting for events stream...</p>
                    )}
                </div>
            </div>
        </div>
    );
}
