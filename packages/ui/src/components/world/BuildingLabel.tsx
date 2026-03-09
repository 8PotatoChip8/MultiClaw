import { Html } from '@react-three/drei';
import { COLORS } from './worldConstants';

interface BuildingLabelProps {
    name: string;
    agentCount: number;
    type: string;
    height: number;
}

export default function BuildingLabel({ name, agentCount, type, height }: BuildingLabelProps) {
    return (
        <Html
            position={[0, height / 2 + 0.5, 0]}
            center
            distanceFactor={12}
            style={{ pointerEvents: 'none' }}
        >
            <div style={{
                background: 'rgba(10, 14, 26, 0.85)',
                border: `1px solid ${type === 'INTERNAL' ? COLORS.INTERNAL : COLORS.EXTERNAL}40`,
                borderRadius: '6px',
                padding: '4px 10px',
                whiteSpace: 'nowrap',
                textAlign: 'center',
                backdropFilter: 'blur(8px)',
            }}>
                <div style={{
                    color: COLORS.LABEL,
                    fontSize: '11px',
                    fontWeight: 600,
                    fontFamily: 'Inter, sans-serif',
                }}>
                    {name}
                </div>
                <div style={{
                    color: COLORS.LABEL_MUTED,
                    fontSize: '9px',
                    fontFamily: 'Inter, sans-serif',
                    marginTop: '1px',
                }}>
                    {agentCount} employee{agentCount !== 1 ? 's' : ''} · {type}
                </div>
            </div>
        </Html>
    );
}
