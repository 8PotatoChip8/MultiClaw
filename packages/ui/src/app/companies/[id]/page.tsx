'use client';

import { useEffect, useState } from 'react';
import { Company, OrgNode } from '../../../lib/types';
import { api } from '../../../lib/api';
import React from 'react';

export default function CompanyViewPage({ params }: { params: { id: string } }) {
    const [company, setCompany] = useState<Company | null>(null);
    const [orgTree, setOrgTree] = useState<OrgNode | null>(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        async function fetchData() {
            try {
                // In a real implementation this would fetch by ID, but we map the list for now
                const companies = await api.getCompanies();
                const matched = companies.find((c: Company) => c.id === params.id);
                setCompany(matched || null);

                if (matched) {
                    const treeData = await api.getOrgTree(params.id);
                    setOrgTree(treeData.tree);
                }
            } catch (e) {
                console.error("Failed to fetch company", e);
            } finally {
                setLoading(false);
            }
        }
        fetchData();
    }, [params.id]);

    if (loading) return <div>Loading company...</div>;
    if (!company) return <div>Company not found</div>;

    return (
        <div>
            <h1>{company.name}</h1>
            <div style={{ display: 'flex', gap: '10px', marginBottom: '30px' }}>
                <span className="badge">{company.type}</span>
                <span className="badge">{company.status}</span>
            </div>

            <div className="panel" style={{ marginBottom: '20px' }}>
                <h3>Actions</h3>
                <div style={{ display: 'flex', gap: '10px' }}>
                    <button className="button" onClick={() => api.hireCeo(company.id)}>Hire CEO</button>
                    {/* Additional actions would go here based on policy & role */}
                </div>
            </div>

            <div className="panel">
                <h3>Organization Tree</h3>
                {orgTree ? (
                    <pre>{JSON.stringify(orgTree, null, 2)}</pre>
                ) : (
                    <p>No organization data currently available. (Ensure a CEO is hired)</p>
                )}
            </div>
        </div>
    );
}
