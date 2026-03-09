import { PresenceStatus } from '../lib/ws';

const PRESENCE_CONFIG: Record<PresenceStatus, { color: string; label: string }> = {
    Active:  { color: '#22c55e', label: 'Active' },
    Busy:    { color: '#f59e0b', label: 'Busy' },
    Offline: { color: '#6b7280', label: 'Offline' },
};

interface AgentStatusProps {
    presence: PresenceStatus;
    showLabel?: boolean;
    size?: number;
}

export default function AgentStatus({ presence, showLabel = true, size = 8 }: AgentStatusProps) {
    const { color, label } = PRESENCE_CONFIG[presence];
    return (
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: '5px' }}>
            <span style={{
                width: `${size}px`, height: `${size}px`,
                borderRadius: '50%',
                backgroundColor: color,
                flexShrink: 0,
                animation: presence === 'Busy' ? 'pulse 2s cubic-bezier(0.4,0,0.6,1) infinite' : 'none',
            }} />
            {showLabel && (
                <span style={{ fontSize: '11px', color, fontWeight: 500 }}>{label}</span>
            )}
        </span>
    );
}
