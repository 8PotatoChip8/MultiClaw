'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Company, LedgerEntry } from '../../lib/types';
import { Wallet, ArrowUpRight, ArrowDownRight } from 'lucide-react';

export default function LedgerPage() {
    const [companies, setCompanies] = useState<Company[]>([]);
    const [selectedCompany, setSelectedCompany] = useState<string>('');
    const [entries, setEntries] = useState<LedgerEntry[]>([]);
    const [loading, setLoading] = useState(false);

    useEffect(() => {
        api.getCompanies().then(d => {
            const list = Array.isArray(d) ? d : [];
            setCompanies(list);
            if (list.length > 0) setSelectedCompany(list[0].id);
        });
    }, []);

    useEffect(() => {
        if (!selectedCompany) return;
        setLoading(true);
        api.getLedger(selectedCompany).then(d => { setEntries(Array.isArray(d) ? d : []); setLoading(false); });
    }, [selectedCompany]);

    const totalRevenue = entries.filter(e => e.type === 'REVENUE').reduce((s, e) => s + (e.amount || 0), 0);
    const totalExpenses = entries.filter(e => e.type === 'EXPENSE').reduce((s, e) => s + (e.amount || 0), 0);

    return (
        <div className="animate-in">
            <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '8px' }}>Financial Ledger</h1>
            <p style={{ color: 'var(--text-muted)', marginBottom: '24px', fontSize: '14px' }}>Track virtual currency flows between companies.</p>

            <div style={{ marginBottom: '24px' }}>
                <select value={selectedCompany} onChange={e => setSelectedCompany(e.target.value)} style={{ maxWidth: '300px' }}>
                    {companies.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
                </select>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '16px', marginBottom: '24px' }}>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <ArrowDownRight size={24} style={{ color: 'var(--success)', marginBottom: '4px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Revenue</div>
                    <div style={{ fontSize: '20px', fontWeight: 700, color: 'var(--success)' }}>${totalRevenue.toFixed(2)}</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <ArrowUpRight size={24} style={{ color: 'var(--danger)', marginBottom: '4px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Expenses</div>
                    <div style={{ fontSize: '20px', fontWeight: 700, color: 'var(--danger)' }}>${totalExpenses.toFixed(2)}</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <Wallet size={24} style={{ color: 'var(--primary)', marginBottom: '4px' }} />
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Net</div>
                    <div style={{ fontSize: '20px', fontWeight: 700 }}>${(totalRevenue - totalExpenses).toFixed(2)}</div>
                </div>
            </div>

            <div className="panel">
                {loading ? <p style={{ color: 'var(--text-muted)' }}>Loading...</p> :
                    entries.length === 0 ? (
                        <div style={{ textAlign: 'center', padding: '40px' }}>
                            <p style={{ color: 'var(--text-muted)' }}>No ledger entries yet for this company.</p>
                        </div>
                    ) : (
                        <table>
                            <thead><tr><th>Date</th><th>Type</th><th>Amount</th><th>Currency</th><th>Memo</th></tr></thead>
                            <tbody>
                                {entries.map(e => (
                                    <tr key={e.id}>
                                        <td style={{ fontSize: '13px' }}>{new Date(e.created_at).toLocaleDateString()}</td>
                                        <td><span className={`badge ${e.type === 'REVENUE' ? 'active' : e.type === 'EXPENSE' ? 'quarantined' : 'internal'}`}>{e.type}</span></td>
                                        <td style={{ fontWeight: 600, color: e.type === 'REVENUE' ? 'var(--success)' : 'var(--danger)' }}>${e.amount}</td>
                                        <td>{e.currency}</td>
                                        <td style={{ color: 'var(--text-muted)', fontSize: '13px' }}>{e.memo || '—'}</td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
            </div>
        </div>
    );
}
