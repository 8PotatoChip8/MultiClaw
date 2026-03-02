import { Agent } from '../lib/types';

export default function AgentStatus({ agent }: { agent: Agent }) {
    const isOnline = agent.status === 'ONLINE';
    const color = isOnline ? 'var(--success)' : 'var(--text-muted)';

    return (
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: '6px', fontSize: '0.8rem', marginLeft: '10px' }}>
            <span style={{ width: '8px', height: '8px', borderRadius: '50%', backgroundColor: color }}></span>
            <span style={{ color }}>{agent.status}</span>
        </span>
    );
}
