'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Company, LedgerEntry } from '../../lib/types';
import { Wallet, ArrowUpRight, ArrowDownRight, Plus, Landmark } from 'lucide-react';

interface CurrencyBalance {
    revenue: number;
    expenses: number;
    capital: number;
    net: number;
}

function formatAmount(amount: number, currency: string): string {
    const fiatSymbols: Record<string, string> = { USD: '$', EUR: '\u20AC', GBP: '\u00A3', JPY: '\u00A5' };
    const symbol = fiatSymbols[currency.toUpperCase()];
    if (symbol) return `${symbol}${amount.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
    return `${amount.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 8 })} ${currency}`;
}

function typeBadgeClass(type: string): string {
    switch (type) {
        case 'REVENUE': return 'active';
        case 'EXPENSE': return 'quarantined';
        case 'CAPITAL_INJECTION': return 'active';
        default: return 'internal';
    }
}

function typeAmountColor(type: string): string {
    switch (type) {
        case 'REVENUE': case 'CAPITAL_INJECTION': return 'var(--success)';
        case 'EXPENSE': case 'INTERNAL_TRANSFER': return 'var(--danger)';
        default: return 'inherit';
    }
}

export default function LedgerPage() {
    const [companies, setCompanies] = useState<Company[]>([]);
    const [selectedCompany, setSelectedCompany] = useState<string>('');
    const [entries, setEntries] = useState<LedgerEntry[]>([]);
    const [balances, setBalances] = useState<Record<string, CurrencyBalance>>({});
    const [loading, setLoading] = useState(false);
    const [showForm, setShowForm] = useState(false);
    const [formType, setFormType] = useState('CAPITAL_INJECTION');
    const [formAmount, setFormAmount] = useState('');
    const [formCurrency, setFormCurrency] = useState('USD');
    const [formMemo, setFormMemo] = useState('');
    const [formCounterparty, setFormCounterparty] = useState('');
    const [formSaving, setFormSaving] = useState(false);

    useEffect(() => {
        api.getCompanies().then(d => {
            const list = Array.isArray(d) ? d : [];
            setCompanies(list);
            if (list.length > 0) setSelectedCompany(list[0].id);
        });
    }, []);

    const loadData = (companyId: string) => {
        if (!companyId) return;
        setLoading(true);
        Promise.all([
            api.getLedger(companyId),
            api.getBalance(companyId),
        ]).then(([ledger, bal]) => {
            setEntries(Array.isArray(ledger) ? ledger : []);
            setBalances(bal && typeof bal === 'object' && !Array.isArray(bal) ? bal : {});
            setLoading(false);
        });
    };

    useEffect(() => { loadData(selectedCompany); }, [selectedCompany]);

    const handleCreate = async () => {
        const amount = parseFloat(formAmount);
        if (!amount || amount <= 0) return;
        setFormSaving(true);
        await api.createLedgerEntry(selectedCompany, {
            type: formType,
            amount,
            currency: formCurrency,
            memo: formMemo || undefined,
            counterparty_company_id: formType === 'INTERNAL_TRANSFER' ? formCounterparty || undefined : undefined,
        });
        setFormAmount('');
        setFormMemo('');
        setFormCounterparty('');
        setFormSaving(false);
        setShowForm(false);
        loadData(selectedCompany);
    };

    const currencies = Object.keys(balances);

    return (
        <div className="animate-in">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
                <h1 style={{ fontSize: '28px', fontWeight: 700 }}>Financial Ledger</h1>
                <button onClick={() => setShowForm(!showForm)} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                    <Plus size={16} /> Record Entry
                </button>
            </div>
            <p style={{ color: 'var(--text-muted)', marginBottom: '24px', fontSize: '14px' }}>Track currency flows between companies. Supports fiat and crypto.</p>

            <div style={{ marginBottom: '24px' }}>
                <select value={selectedCompany} onChange={e => setSelectedCompany(e.target.value)} style={{ maxWidth: '300px' }}>
                    {companies.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
                </select>
            </div>

            {/* Record Entry Form */}
            {showForm && (
                <div className="panel" style={{ marginBottom: '24px', padding: '20px' }}>
                    <h3 style={{ fontSize: '16px', fontWeight: 600, marginBottom: '16px' }}>Record Ledger Entry</h3>
                    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '12px', marginBottom: '12px' }}>
                        <div>
                            <label style={{ fontSize: '12px', color: 'var(--text-muted)', display: 'block', marginBottom: '4px' }}>Type</label>
                            <select value={formType} onChange={e => setFormType(e.target.value)} style={{ width: '100%' }}>
                                <option value="CAPITAL_INJECTION">Capital Injection</option>
                                <option value="REVENUE">Revenue</option>
                                <option value="EXPENSE">Expense</option>
                                <option value="INTERNAL_TRANSFER">Internal Transfer</option>
                            </select>
                        </div>
                        <div>
                            <label style={{ fontSize: '12px', color: 'var(--text-muted)', display: 'block', marginBottom: '4px' }}>Amount</label>
                            <input type="number" min="0" step="any" placeholder="0.00" value={formAmount}
                                onChange={e => setFormAmount(e.target.value)} style={{ width: '100%' }} />
                        </div>
                        <div>
                            <label style={{ fontSize: '12px', color: 'var(--text-muted)', display: 'block', marginBottom: '4px' }}>Currency</label>
                            <input type="text" placeholder="USD" value={formCurrency}
                                onChange={e => setFormCurrency(e.target.value.toUpperCase())} style={{ width: '100%' }} />
                        </div>
                    </div>
                    <div style={{ marginBottom: '12px' }}>
                        <label style={{ fontSize: '12px', color: 'var(--text-muted)', display: 'block', marginBottom: '4px' }}>Memo</label>
                        <input type="text" placeholder="Description of the transaction" value={formMemo}
                            onChange={e => setFormMemo(e.target.value)} style={{ width: '100%' }} />
                    </div>
                    {formType === 'INTERNAL_TRANSFER' && (
                        <div style={{ marginBottom: '12px' }}>
                            <label style={{ fontSize: '12px', color: 'var(--text-muted)', display: 'block', marginBottom: '4px' }}>Counterparty Company</label>
                            <select value={formCounterparty} onChange={e => setFormCounterparty(e.target.value)} style={{ width: '100%' }}>
                                <option value="">Select company...</option>
                                {companies.filter(c => c.id !== selectedCompany).map(c =>
                                    <option key={c.id} value={c.id}>{c.name}</option>
                                )}
                            </select>
                        </div>
                    )}
                    <button onClick={handleCreate} disabled={formSaving || !formAmount || parseFloat(formAmount) <= 0}>
                        {formSaving ? 'Saving...' : 'Save Entry'}
                    </button>
                </div>
            )}

            {/* Balance Summary Cards — grouped by currency */}
            {currencies.length > 0 && (
                <div style={{ marginBottom: '24px' }}>
                    {currencies.map(currency => {
                        const b = balances[currency];
                        return (
                            <div key={currency} style={{ marginBottom: '12px' }}>
                                <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-muted)', marginBottom: '6px' }}>{currency}</div>
                                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '12px' }}>
                                    <div className="panel" style={{ textAlign: 'center', padding: '12px' }}>
                                        <Landmark size={18} style={{ color: 'var(--primary)', marginBottom: '2px' }} />
                                        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>Capital</div>
                                        <div style={{ fontSize: '17px', fontWeight: 700, color: 'var(--primary)' }}>{formatAmount(b.capital, currency)}</div>
                                    </div>
                                    <div className="panel" style={{ textAlign: 'center', padding: '12px' }}>
                                        <ArrowDownRight size={18} style={{ color: 'var(--success)', marginBottom: '2px' }} />
                                        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>Revenue</div>
                                        <div style={{ fontSize: '17px', fontWeight: 700, color: 'var(--success)' }}>{formatAmount(b.revenue, currency)}</div>
                                    </div>
                                    <div className="panel" style={{ textAlign: 'center', padding: '12px' }}>
                                        <ArrowUpRight size={18} style={{ color: 'var(--danger)', marginBottom: '2px' }} />
                                        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>Expenses</div>
                                        <div style={{ fontSize: '17px', fontWeight: 700, color: 'var(--danger)' }}>{formatAmount(b.expenses, currency)}</div>
                                    </div>
                                    <div className="panel" style={{ textAlign: 'center', padding: '12px' }}>
                                        <Wallet size={18} style={{ color: b.net >= 0 ? 'var(--success)' : 'var(--danger)', marginBottom: '2px' }} />
                                        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>Net</div>
                                        <div style={{ fontSize: '17px', fontWeight: 700, color: b.net >= 0 ? 'var(--success)' : 'var(--danger)' }}>{formatAmount(b.net, currency)}</div>
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}

            {/* Entries Table */}
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
                                        <td><span className={`badge ${typeBadgeClass(e.type)}`}>{e.type.replace('_', ' ')}</span></td>
                                        <td style={{ fontWeight: 600, color: typeAmountColor(e.type) }}>{formatAmount(e.amount, e.currency)}</td>
                                        <td>{e.currency}</td>
                                        <td style={{ color: 'var(--text-muted)', fontSize: '13px' }}>{e.memo || '\u2014'}</td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
            </div>
        </div>
    );
}
