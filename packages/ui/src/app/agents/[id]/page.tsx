'use client';

import { useEffect, useState } from 'react';
import { Agent } from '../../../lib/types';
import { api } from '../../../lib/api';
import React from 'react';

export default function AgentViewPage({ params }: { params: { id: string } }) {
    const [agent, setAgent] = useState<Agent | null>(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        async function fetchAgent() {
            try {
                // This assumes `api.getAgent(id)` is implemented
                const agentData = await api.getAgent(params.id);
                setAgent(agentData);
            } catch (e) {
                console.error("Failed to fetch agent", e);
            } finally {
                setLoading(false);
            }
        }
        fetchAgent();
    }, [params.id]);

    if (loading) return <div>Loading agent details...</div>;
    if (!agent) return <div>Agent not found</div>;

    return (
        <div>
            <h1>Agent: {agent.name}</h1>
            <div style={{ display: 'flex', gap: '10px', marginBottom: '30px' }}>
                <span className="badge">{agent.role}</span>
                <span className="badge">{agent.status}</span>
                <span className="badge" style={{ background: 'var(--accent)' }}>{agent.effective_model}</span>
            </div>

            <div className="panel" style={{ marginBottom: '20px' }}>
                <h3>Controls</h3>
                <div style={{ display: 'flex', gap: '10px' }}>
                    <button className="button" onClick={() => alert("Rebooting VM... (Mock)")}>Restart VM</button>
                    <button className="button" onClick={() => alert("Deprovisioning... (Mock)")} style={{ background: '#551111' }}>Deprovision</button>
                </div>
            </div>

            <div className="panel">
                <h3>System Logs</h3>
                <pre style={{ background: '#000', padding: '10px', borderRadius: '4px', minHeight: '100px' }}>
                    [SYSTEM] Agent {agent.name} initialized.
                    [SYSTEM] Model {agent.effective_model} loaded into memory context.
                </pre>
            </div>
        </div>
    );
}
