'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { ServiceCatalogItem } from '../../lib/types';
import { ShoppingBag, Plus } from 'lucide-react';

export default function ServicesPage() {
    const [services, setServices] = useState<ServiceCatalogItem[]>([]);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        api.getServices().then(d => { setServices(Array.isArray(d) ? d : []); setLoading(false); }).catch(() => setLoading(false));
    }, []);

    return (
        <div className="animate-in">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '30px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>Internal Services Marketplace</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Services offered by internal companies to the holding.</p>
                </div>
            </div>

            {loading ? <p style={{ color: 'var(--text-muted)' }}>Loading...</p> :
                services.length === 0 ? (
                    <div className="panel" style={{ textAlign: 'center', padding: '60px 20px' }}>
                        <ShoppingBag size={40} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                        <p style={{ color: 'var(--text-muted)' }}>No services published yet. Internal companies can publish services for other companies to hire.</p>
                    </div>
                ) : (
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: '16px' }}>
                        {services.map(s => (
                            <div key={s.id} className="panel">
                                <h3 style={{ fontSize: '16px', fontWeight: 700, marginBottom: '8px' }}>{s.name}</h3>
                                {s.description && <p style={{ fontSize: '13px', color: 'var(--text-muted)', marginBottom: '12px' }}>{s.description}</p>}
                                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '12px' }}>
                                    <span style={{ color: 'var(--text-muted)' }}>Model: {s.pricing_model}</span>
                                    <span className="badge active">{s.active ? 'Active' : 'Inactive'}</span>
                                </div>
                            </div>
                        ))}
                    </div>
                )}
        </div>
    );
}
