'use client';

import { Company } from '../../lib/types';
import CompanyCard from '../../components/CompanyCard';
import { api } from '../../lib/api';
import { useEffect, useState } from 'react';
import { Plus, X } from 'lucide-react';

export default function CompaniesPage() {
    const [companies, setCompanies] = useState<Company[]>([]);
    const [loading, setLoading] = useState(true);
    const [showCreate, setShowCreate] = useState(false);
    const [form, setForm] = useState({ name: '', type: 'EXTERNAL', description: '' });
    const [creating, setCreating] = useState(false);

    const loadCompanies = () => {
        api.getCompanies()
            .then(data => { setCompanies(Array.isArray(data) ? data : []); setLoading(false); })
            .catch(() => setLoading(false));
    };

    useEffect(() => { loadCompanies(); }, []);

    const handleCreate = async () => {
        setCreating(true);
        try {
            await api.createCompany({ name: form.name, type: form.type, description: form.description || undefined });
            setShowCreate(false);
            setForm({ name: '', type: 'EXTERNAL', description: '' });
            loadCompanies();
        } catch (e) { console.error(e); }
        setCreating(false);
    };

    if (loading) return <div className="animate-in"><p style={{ color: 'var(--text-muted)' }}>Loading companies...</p></div>;

    return (
        <div className="animate-in">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '30px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>Companies</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>{companies.length} registered companies</p>
                </div>
                <button className="button" onClick={() => setShowCreate(true)} style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <Plus size={16} /> Create Company
                </button>
            </div>

            {companies.length === 0 ? (
                <div className="panel" style={{ textAlign: 'center', padding: '60px 20px' }}>
                    <p style={{ color: 'var(--text-muted)', marginBottom: '16px' }}>No companies yet. Create your first company to get started.</p>
                    <button className="button" onClick={() => setShowCreate(true)}>Create Company</button>
                </div>
            ) : (
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: '16px' }}>
                    {companies.map(c => <CompanyCard key={c.id} company={c} />)}
                </div>
            )}

            {showCreate && (
                <div className="modal-overlay" onClick={() => setShowCreate(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
                            <h2 style={{ fontSize: '20px', fontWeight: 700 }}>Create Company</h2>
                            <button onClick={() => setShowCreate(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}><X size={20} /></button>
                        </div>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Company Name</label>
                                <input value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} placeholder="Acme Corp" />
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Type</label>
                                <select value={form.type} onChange={e => setForm({ ...form, type: e.target.value })}>
                                    <option value="EXTERNAL">External (public-facing)</option>
                                    <option value="INTERNAL">Internal (services provider)</option>
                                </select>
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Description</label>
                                <textarea value={form.description} onChange={e => setForm({ ...form, description: e.target.value })} placeholder="What does this company do?" rows={3} />
                            </div>
                            <button className="button" onClick={handleCreate} disabled={!form.name || creating} style={{ marginTop: '8px' }}>
                                {creating ? 'Creating...' : 'Create Company'}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
