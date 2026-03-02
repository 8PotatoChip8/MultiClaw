import { OrgNode } from '../lib/types';
import AgentStatus from './AgentStatus';

export default function OrgTree({ data }: { data: OrgNode }) {
    if (!data) return <div>Loading...</div>;

    return (
        <div style={{ marginLeft: '20px', borderLeft: '1px solid var(--border)', paddingLeft: '20px' }}>
            <div style={{ marginBottom: '10px' }}>
                <strong>{data.agent.name}</strong>
                <span style={{ marginLeft: '10px', fontSize: '0.8rem', color: 'var(--text-muted)' }}>{data.agent.role}</span>
                <AgentStatus agent={data.agent} />
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
