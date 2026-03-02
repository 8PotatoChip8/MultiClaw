'use client';

import { useState, useEffect } from 'react';
import OrgTree from '../../components/OrgTree';
import { api } from '../../lib/api';

export default function OrgPage() {
    const [treeData, setTreeData] = useState<any>(null);

    useEffect(() => {
        // In MVP hook up to dummy or actual API. We'll simulate for UI completeness.
        setTreeData({
            agent: { id: '1', name: 'MainAgent', role: 'MAIN', status: 'ONLINE' },
            children: [
                {
                    agent: { id: '2', name: 'Alpha CEO', role: 'CEO', status: 'ONLINE', company_id: 'c1' },
                    children: [
                        { agent: { id: '3', name: 'Sales Mgr', role: 'MANAGER', status: 'ONLINE' }, children: [] }
                    ]
                }
            ]
        });
    }, []);

    return (
        <div>
            <h1 style={{ marginBottom: '30px' }}>Organization Tree</h1>
            <div className="panel" style={{ background: 'var(--bg)' }}>
                {treeData ? <OrgTree data={treeData} /> : <p>Loading hierarchical org tree...</p>}
            </div>
        </div>
    );
}
