import { OrgNode } from '../lib/types';
import AgentStatus from './AgentStatus';
import { derivePresence } from '../lib/ws';

export default function OrgTree({ data }: { data: OrgNode }) {
    if (!data) return <div>Loading...</div>;

    const presence = derivePresence(data.agent.status, data.agent.activity);

    return (
        <div style={{ marginLeft: '20px', borderLeft: '1px solid var(--border)', paddingLeft: '20px' }}>
            <div style={{ marginBottom: '10px' }}>
                <strong>{data.agent.name}</strong>
                <span style={{ marginLeft: '10px', fontSize: '0.8rem', color: 'var(--text-muted)' }}>{data.agent.role}</span>
                <AgentStatus presence={presence.presenceStatus} />
            </div>

            {data.children && data.children.length > 0 && (
                <div style={{ marginTop: '10px' }}>
                    {data.children.map(child => (
                        <OrgTree key={child.agent.id} data={child} />
                    ))}
                </div>
            )}
        </div>
    );
}
