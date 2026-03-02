import { Company } from '../lib/types';
import Link from 'next/link';

export default function CompanyCard({ company }: { company: Company }) {
    return (
        <div className="panel" style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                <h3 style={{ margin: 0 }}>{company.name}</h3>
                <span style={{ fontSize: '0.7rem', background: 'var(--bg)', padding: '4px 8px', borderRadius: '4px', border: '1px solid var(--border)' }}>
                    {company.type}
                </span>
            </div>
            <div style={{ color: 'var(--text-muted)', fontSize: '0.9rem' }}>
                Status: {company.status}
            </div>
            <div style={{ marginTop: '10px' }}>
                <Link href={`/companies/${company.id}`} className="button" style={{ display: 'inline-block' }}>View Details</Link>
            </div>
        </div>
    );
}
