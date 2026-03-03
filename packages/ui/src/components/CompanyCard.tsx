import { Company } from '../lib/types';
import Link from 'next/link';
import { Building2 } from 'lucide-react';

export default function CompanyCard({ company }: { company: Company }) {
    return (
        <Link href={`/companies/${company.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
            <div className="panel" style={{ cursor: 'pointer' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '12px' }}>
                    <Building2 size={20} style={{ color: 'var(--primary)' }} />
                    <h3 style={{ fontSize: '16px', fontWeight: 700 }}>{company.name}</h3>
                </div>
                <div style={{ display: 'flex', gap: '8px', marginBottom: '8px' }}>
                    <span className={`badge ${company.type === 'INTERNAL' ? 'internal' : 'external'}`}>{company.type}</span>
                    <span className={`badge ${company.status === 'ACTIVE' ? 'active' : 'pending'}`}>{company.status}</span>
                </div>
                {company.description && <p style={{ fontSize: '13px', color: 'var(--text-muted)', lineHeight: 1.4 }}>{company.description}</p>}
            </div>
        </Link>
    );
}
