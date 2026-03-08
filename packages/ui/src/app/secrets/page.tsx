'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Key, Plus, Trash2, Eye, EyeOff } from 'lucide-react';

interface SecretMeta {
    id: string;
    name: string;
    description: string;
    scope_type: string;
    scope_id: string;
    created_at: string;
}

type ScopeType = 'agent' | 'manager' | 'company' | 'holding';

export default function SecretsPage() {
    const [secrets, setSecrets] = useState<SecretMeta[]>([]);
    const [agents, setAgents] = useState<{ id: string; name: string; role?: string }[]>([]);
    const [companies, setCompanies] = useState<{ id: string; name: string }[]>([]);
    const [loading, setLoading] = useState(true);
    const [showCreate, setShowCreate] = useState(false);
    const [deleting, setDeleting] = useState<string | null>(null);

    // Form state
    const [scopeType, setScopeType] = useState<ScopeType>('agent');
    const [scopeId, setScopeId] = useState('');
    const [secretName, setSecretName] = useState('');
    const [secretValue, setSecretValue] = useState('');
    const [secretDescription, setSecretDescription] = useState('');
    const [showValue, setShowValue] = useState(false);
    const [creating, setCreating] = useState(false);

    const nameMap: Record<string, string> = {};
    agents.forEach(a => { nameMap[a.id] = a.name; });
    companies.forEach(c => { nameMap[c.id] = c.name; });

    const loadData = async () => {
        setLoading(true);
        const [secretsData, agentsData, companiesData] = await Promise.all([
            api.getSecrets(),
            api.getAgents(),
            api.getCompanies(),
        ]);
        setSecrets(Array.isArray(secretsData) ? secretsData : []);
        setAgents(Array.isArray(agentsData) ? agentsData : []);
        setCompanies(Array.isArray(companiesData) ? companiesData : []);
        setLoading(false);
    };

    useEffect(() => { loadData(); }, []);

    const handleCreate = async () => {
        if (!secretName.trim() || !secretValue.trim()) return;
        if (scopeType !== 'holding' && !scopeId) return;

        setCreating(true);
        const finalScopeId = scopeType === 'holding'
            ? (companies.find(c => nameMap[c.id])?.id || companies[0]?.id || '')
            : scopeId;
        await api.createSecret({
            scope_type: scopeType,
            scope_id: finalScopeId,
            name: secretName.trim(),
            value: secretValue,
            description: secretDescription.trim() || undefined,
        });
        setShowCreate(false);
        setScopeType('agent');
        setScopeId('');
        setSecretName('');
        setSecretValue('');
        setSecretDescription('');
        setShowValue(false);
        setCreating(false);
        await loadData();
    };

    const handleDelete = async (id: string) => {
        setDeleting(id);
        await api.deleteSecret(id);
        setDeleting(null);
        await loadData();
    };

    const managers = agents.filter(a => a.role === 'MANAGER');

    const scopeLabel = (s: SecretMeta) => {
        if (s.scope_type === 'holding') return 'All Agents';
        if (s.scope_type === 'manager') {
            const name = nameMap[s.scope_id];
            return name ? `${name}'s dept` : s.scope_id.slice(0, 8);
        }
        return nameMap[s.scope_id] || s.scope_id.slice(0, 8);
    };

    const scopeBadgeColor = (type: string) => {
        switch (type) {
            case 'agent': return 'var(--primary)';
            case 'manager': return '#f59e0b';
            case 'company': return 'var(--accent)';
            case 'holding': return 'var(--success)';
            default: return 'var(--text-muted)';
        }
    };

    return (
        <div className="animate-in">
            <div style={{ marginBottom: '32px', display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between' }}>
                <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '4px' }}>
                        <Key size={24} style={{ color: 'var(--primary)' }} />
                        <h1 style={{ fontSize: '28px', fontWeight: 700 }}>Secrets</h1>
                    </div>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>
                        Manage encrypted secrets for your agents. Values are never displayed after creation.
                    </p>
                </div>
                <button className="button" onClick={() => setShowCreate(true)}
                    style={{ display: 'flex', alignItems: 'center', gap: '6px', whiteSpace: 'nowrap' }}>
                    <Plus size={16} /> Add Secret
                </button>
            </div>

            {/* Secrets Table */}
            <div className="panel" style={{ overflow: 'hidden' }}>
                {loading ? (
                    <p style={{ color: 'var(--text-muted)', padding: '20px', textAlign: 'center' }}>Loading...</p>
                ) : secrets.length === 0 ? (
                    <div style={{ padding: '40px 20px', textAlign: 'center' }}>
                        <Key size={32} style={{ color: 'var(--text-muted)', marginBottom: '12px', opacity: 0.4 }} />
                        <p style={{ color: 'var(--text-muted)', fontSize: '14px', marginBottom: '4px' }}>No secrets yet</p>
                        <p style={{ color: 'var(--text-muted)', fontSize: '12px' }}>
                            Click "Add Secret" to store API keys, credentials, or tokens for your agents.
                        </p>
                    </div>
                ) : (
                    <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13px' }}>
                        <thead>
                            <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                <th style={{ textAlign: 'left', padding: '12px 16px', color: 'var(--text-muted)', fontWeight: 600, fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Name</th>
                                <th style={{ textAlign: 'left', padding: '12px 16px', color: 'var(--text-muted)', fontWeight: 600, fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Description</th>
                                <th style={{ textAlign: 'left', padding: '12px 16px', color: 'var(--text-muted)', fontWeight: 600, fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Scope</th>
                                <th style={{ textAlign: 'left', padding: '12px 16px', color: 'var(--text-muted)', fontWeight: 600, fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Target</th>
                                <th style={{ textAlign: 'left', padding: '12px 16px', color: 'var(--text-muted)', fontWeight: 600, fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Created</th>
                                <th style={{ width: '60px', padding: '12px 16px' }}></th>
                            </tr>
                        </thead>
                        <tbody>
                            {secrets.map(s => (
                                <tr key={s.id} style={{ borderBottom: '1px solid rgba(59, 130, 246, 0.08)' }}>
                                    <td style={{ padding: '12px 16px', fontFamily: 'monospace', fontWeight: 600 }}>{s.name}</td>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-muted)', fontSize: '12px', maxWidth: '200px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                                        title={s.description || undefined}>
                                        {s.description || <span style={{ opacity: 0.4 }}>--</span>}
                                    </td>
                                    <td style={{ padding: '12px 16px' }}>
                                        <span style={{
                                            fontSize: '10px', padding: '2px 8px', borderRadius: '10px',
                                            background: `${scopeBadgeColor(s.scope_type)}20`,
                                            color: scopeBadgeColor(s.scope_type),
                                            fontWeight: 600, textTransform: 'uppercase',
                                        }}>{s.scope_type}</span>
                                    </td>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-muted)' }}>{scopeLabel(s)}</td>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-muted)', fontSize: '12px' }}>
                                        {new Date(s.created_at).toLocaleDateString()}
                                    </td>
                                    <td style={{ padding: '12px 16px', textAlign: 'right' }}>
                                        <button
                                            onClick={() => handleDelete(s.id)}
                                            disabled={deleting === s.id}
                                            title="Delete secret"
                                            style={{
                                                background: 'none', border: 'none', cursor: 'pointer',
                                                color: 'var(--text-muted)', padding: '4px',
                                                opacity: deleting === s.id ? 0.3 : 0.6,
                                                transition: 'opacity 0.2s',
                                            }}
                                            onMouseEnter={e => (e.currentTarget.style.opacity = '1', e.currentTarget.style.color = 'var(--danger)')}
                                            onMouseLeave={e => (e.currentTarget.style.opacity = '0.6', e.currentTarget.style.color = 'var(--text-muted)')}
                                        >
                                            <Trash2 size={15} />
                                        </button>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                )}
            </div>

            {/* Create Secret Modal */}
            {showCreate && (
                <div className="modal-overlay" onClick={() => setShowCreate(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()} style={{ width: '480px' }}>
                        <h2 style={{ fontSize: '20px', fontWeight: 700, marginBottom: '20px', display: 'flex', alignItems: 'center', gap: '8px' }}>
                            <Key size={20} style={{ color: 'var(--primary)' }} />
                            Add Secret
                        </h2>

                        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                            {/* Scope Type */}
                            <div>
                                <label style={{ display: 'block', fontSize: '13px', color: 'var(--text-muted)', marginBottom: '8px', fontWeight: 500 }}>
                                    Scope
                                </label>
                                <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
                                    {(['agent', 'manager', 'company', 'holding'] as ScopeType[]).map(st => (
                                        <button key={st} onClick={() => { setScopeType(st); setScopeId(''); }}
                                            style={{
                                                flex: 1, padding: '8px 12px', borderRadius: '8px', fontSize: '13px',
                                                fontWeight: 600, cursor: 'pointer', textTransform: 'capitalize',
                                                border: scopeType === st ? `2px solid ${scopeBadgeColor(st)}` : '2px solid var(--border)',
                                                background: scopeType === st ? `${scopeBadgeColor(st)}15` : 'transparent',
                                                color: scopeType === st ? scopeBadgeColor(st) : 'var(--text-muted)',
                                                transition: 'all 0.2s',
                                            }}>
                                            {st}
                                        </button>
                                    ))}
                                </div>
                                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '6px' }}>
                                    {scopeType === 'agent' && 'Available only to the selected agent.'}
                                    {scopeType === 'manager' && 'Available to the selected manager and all workers in their department.'}
                                    {scopeType === 'company' && 'Available to all agents in the selected company.'}
                                    {scopeType === 'holding' && 'Available to all agents across all companies.'}
                                </p>
                            </div>

                            {/* Scope Target */}
                            {scopeType !== 'holding' && (
                                <div>
                                    <label style={{ display: 'block', fontSize: '13px', color: 'var(--text-muted)', marginBottom: '6px', fontWeight: 500 }}>
                                        {scopeType === 'agent' ? 'Agent' : scopeType === 'manager' ? 'Manager (Department)' : 'Company'}
                                    </label>
                                    <select value={scopeId} onChange={e => setScopeId(e.target.value)}
                                        style={{
                                            width: '100%', padding: '10px 12px', borderRadius: '8px',
                                            background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                            color: 'var(--text)', fontSize: '13px',
                                        }}>
                                        <option value="">Select {scopeType === 'manager' ? 'manager' : scopeType}...</option>
                                        {scopeType === 'agent'
                                            ? agents.map(a => <option key={a.id} value={a.id}>{a.name}</option>)
                                            : scopeType === 'manager'
                                            ? managers.map(m => <option key={m.id} value={m.id}>{m.name}</option>)
                                            : companies.map(c => <option key={c.id} value={c.id}>{c.name}</option>)
                                        }
                                    </select>
                                </div>
                            )}

                            {/* Secret Name */}
                            <div>
                                <label style={{ display: 'block', fontSize: '13px', color: 'var(--text-muted)', marginBottom: '6px', fontWeight: 500 }}>
                                    Name
                                </label>
                                <input value={secretName} onChange={e => setSecretName(e.target.value)}
                                    placeholder="e.g. COINEX_API_KEY"
                                    style={{
                                        width: '100%', padding: '10px 12px', borderRadius: '8px',
                                        background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                        color: 'var(--text)', fontSize: '13px', fontFamily: 'monospace',
                                    }} />
                            </div>

                            {/* Description */}
                            <div>
                                <label style={{ display: 'block', fontSize: '13px', color: 'var(--text-muted)', marginBottom: '6px', fontWeight: 500 }}>
                                    Description <span style={{ fontWeight: 400, opacity: 0.6 }}>(optional)</span>
                                </label>
                                <input value={secretDescription} onChange={e => setSecretDescription(e.target.value)}
                                    placeholder="e.g. Read-only API key for market data"
                                    style={{
                                        width: '100%', padding: '10px 12px', borderRadius: '8px',
                                        background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                        color: 'var(--text)', fontSize: '13px',
                                    }} />
                                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '6px' }}>
                                    Helps agents choose the right credential when they have multiple secrets for the same service.
                                </p>
                            </div>

                            {/* Secret Value */}
                            <div>
                                <label style={{ display: 'block', fontSize: '13px', color: 'var(--text-muted)', marginBottom: '6px', fontWeight: 500 }}>
                                    Value
                                </label>
                                <div style={{ position: 'relative' }}>
                                    <input value={secretValue}
                                        onChange={e => setSecretValue(e.target.value)}
                                        type={showValue ? 'text' : 'password'}
                                        placeholder="Your secret value"
                                        style={{
                                            width: '100%', padding: '10px 40px 10px 12px', borderRadius: '8px',
                                            background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                            color: 'var(--text)', fontSize: '13px', fontFamily: 'monospace',
                                        }} />
                                    <button onClick={() => setShowValue(!showValue)}
                                        style={{
                                            position: 'absolute', right: '8px', top: '50%', transform: 'translateY(-50%)',
                                            background: 'none', border: 'none', cursor: 'pointer',
                                            color: 'var(--text-muted)', padding: '4px',
                                        }}>
                                        {showValue ? <EyeOff size={16} /> : <Eye size={16} />}
                                    </button>
                                </div>
                                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '6px' }}>
                                    Encrypted at rest with AES-GCM. Cannot be viewed after creation.
                                </p>
                            </div>

                            {/* Actions */}
                            <div style={{ display: 'flex', gap: '8px', justifyContent: 'flex-end', marginTop: '8px' }}>
                                <button className="button secondary" onClick={() => setShowCreate(false)}>Cancel</button>
                                <button className="button" onClick={handleCreate}
                                    disabled={creating || !secretName.trim() || !secretValue.trim() || (scopeType !== 'holding' && !scopeId)}>
                                    {creating ? 'Creating...' : 'Create Secret'}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
