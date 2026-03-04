'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Company, Agent } from '../../lib/types';
import Link from 'next/link';
import { ChevronRight, User, Shield, Briefcase, Wrench } from 'lucide-react';

const roleIcons: Record<string, any> = { MAIN: Shield, CEO: Briefcase, MANAGER: User, WORKER: Wrench };
const roleColors: Record<string, string> = { MAIN: 'var(--accent)', CEO: 'var(--primary)', MANAGER: 'var(--success)', WORKER: 'var(--text-muted)' };

export default function OrgPage() {
    const [companies, setCompanies] = useState<Company[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [mainAgent, setMainAgent] = useState<Agent | null>(null);

    useEffect(() => {
        api.getCompanies().then(d => setCompanies(Array.isArray(d) ? d : []));
        api.getAgents().then(d => {
            const list = Array.isArray(d) ? d : [];
            setAgents(list);
            setMainAgent(list.find((a: Agent) => a.role === 'MAIN') || null);
        });
    }, []);

    const getAgentsForCompany = (companyId: string) => agents.filter(a => a.company_id === companyId);

    const AgentNode = ({ agent, depth = 0, companyScope }: { agent: Agent; depth?: number; companyScope?: string }) => {
        const Icon = roleIcons[agent.role] || User;
        // Only show children that belong to the same company scope (prevents subsidiary agents from showing under holding)
        const children = agents.filter(a => a.parent_agent_id === agent.id && (!companyScope || a.company_id === companyScope || !a.company_id));
        return (
            <div style={{ marginLeft: depth * 28 }}>
                <Link href={`/agents/${agent.id}`} style={{ display: 'flex', alignItems: 'center', gap: '10px', padding: '8px 12px', borderRadius: '8px', transition: 'background 0.2s', color: 'var(--text)' }}>
                    <Icon size={16} style={{ color: roleColors[agent.role] }} />
                    <span style={{ fontWeight: 500, fontSize: '14px' }}>{agent.name}</span>
                    <span className={`badge ${agent.role === 'CEO' ? 'external' : agent.role === 'MANAGER' ? 'internal' : 'active'}`} style={{ fontSize: '10px' }}>{agent.role}</span>
                    <span style={{ fontSize: '11px', color: 'var(--text-muted)', marginLeft: 'auto' }}>{agent.effective_model}</span>
                </Link>
                {children.map(c => <AgentNode key={c.id} agent={c} depth={depth + 1} companyScope={companyScope} />)}
            </div>
        );
    };

    return (
        <div className="animate-in">
            <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '32px' }}>Organization Tree</h1>

            {mainAgent && (
                <div className="panel" style={{ marginBottom: '20px' }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '12px' }}>
                        <Shield size={20} style={{ color: 'var(--accent)' }} />
                        <span style={{ fontWeight: 700, fontSize: '16px' }}>Holding Company</span>
                    </div>
                    <AgentNode agent={mainAgent} companyScope={mainAgent.company_id || '__holding__'} />
                </div>
            )}

            {companies.map(company => {
                const companyAgents = getAgentsForCompany(company.id);
                const ceos = companyAgents.filter(a => a.role === 'CEO');
                return (
                    <div key={company.id} className="panel" style={{ marginBottom: '12px' }}>
                        <Link href={`/companies/${company.id}`} style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '12px', color: 'var(--text)' }}>
                            <Briefcase size={18} style={{ color: 'var(--primary)' }} />
                            <span style={{ fontWeight: 700 }}>{company.name}</span>
                            <span className={`badge ${company.type === 'INTERNAL' ? 'internal' : 'external'}`}>{company.type}</span>
                            <ChevronRight size={16} style={{ marginLeft: 'auto', color: 'var(--text-muted)' }} />
                        </Link>
                        {companyAgents.length === 0 ? (
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px', paddingLeft: '28px' }}>No agents assigned</p>
                        ) : (
                            ceos.map(ceo => <AgentNode key={ceo.id} agent={ceo} depth={1} companyScope={company.id} />)
                        )}
                    </div>
                );
            })}

            {companies.length === 0 && !mainAgent && (
                <div className="panel" style={{ textAlign: 'center', padding: '60px' }}>
                    <p style={{ color: 'var(--text-muted)' }}>No organizations yet. Initialize the system first.</p>
                </div>
            )}
        </div>
    );
}
