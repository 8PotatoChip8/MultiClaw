'use client';

import { Company } from '../../lib/types';
import CompanyCard from '../../components/CompanyCard';

export default function CompaniesPage() {
    const companies: Company[] = [
        { id: '1', name: 'Alpha Software', type: 'EXTERNAL', status: 'ACTIVE' },
        { id: '2', name: 'Omega CyberServices', type: 'INTERNAL', status: 'ACTIVE' },
    ];

    return (
        <div>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '30px' }}>
                <h1 style={{ margin: 0 }}>Companies</h1>
                <button className="button">Create Company</button>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))', gap: '20px' }}>
                {companies.map(c => <CompanyCard key={c.id} company={c} />)}
            </div>
        </div>
    );
}
