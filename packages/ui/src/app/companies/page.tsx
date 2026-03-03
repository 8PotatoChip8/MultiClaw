'use client';

import { Company } from '../../lib/types';
import CompanyCard from '../../components/CompanyCard';
import { api } from '../../lib/api';
import { useEffect, useState } from 'react';

export default function CompaniesPage() {
    const [companies, setCompanies] = useState<Company[]>([]);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        api.getCompanies()
            .then(data => {
                setCompanies(Array.isArray(data) ? data : []);
                setLoading(false);
            })
            .catch(err => {
                console.error("Failed to fetch companies", err);
                setLoading(false);
            });
    }, []);

    if (loading) return <div>Loading companies...</div>;

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
