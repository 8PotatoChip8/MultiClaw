import { COLORS, FLOOR } from './worldConstants';
import { Agent } from '../../lib/types';

interface BreakRoomProps {
    position: [number, number, number];
    width: number;
    depth: number;
    idleAgents: Agent[];
}

export default function BreakRoom({ position, width, depth, idleAgents }: BreakRoomProps) {
    return (
        <group position={position}>
            {/* Floor area */}
            <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.005, 0]}>
                <planeGeometry args={[width, depth]} />
                <meshStandardMaterial color={COLORS.BREAK_ROOM} roughness={0.9} />
            </mesh>

            {/* Table */}
            <mesh position={[0, 0.3, 0]} castShadow>
                <cylinderGeometry args={[0.3, 0.3, 0.04, 16]} />
                <meshStandardMaterial color="#3a4454" roughness={0.5} />
            </mesh>
            <mesh position={[0, 0.15, 0]}>
                <cylinderGeometry args={[0.05, 0.05, 0.3, 8]} />
                <meshStandardMaterial color="#3a4454" />
            </mesh>

            {/* Couch */}
            <mesh position={[0, 0.18, -depth / 2 + 0.25]} castShadow>
                <boxGeometry args={[width * 0.7, 0.2, 0.3]} />
                <meshStandardMaterial color="#2a3a5a" roughness={0.8} />
            </mesh>
            {/* Couch back */}
            <mesh position={[0, 0.35, -depth / 2 + 0.12]}>
                <boxGeometry args={[width * 0.7, 0.2, 0.08]} />
                <meshStandardMaterial color="#2a3a5a" roughness={0.8} />
            </mesh>

            {/* Idle agent figures */}
            {idleAgents.map((agent, i) => {
                const angle = (i / Math.max(idleAgents.length, 1)) * Math.PI * 2;
                const radius = 0.5;
                const x = Math.cos(angle) * radius;
                const z = Math.sin(angle) * radius;

                const roleColor = agent.role === 'CEO' ? COLORS.CEO
                    : agent.role === 'MANAGER' ? COLORS.MANAGER
                    : COLORS.WORKER;

                return (
                    <group key={agent.id} position={[x, 0, z]}>
                        {/* Body */}
                        <mesh position={[0, FLOOR.DESK_Y - 0.05, 0]}>
                            <boxGeometry args={[FLOOR.AGENT_BODY_WIDTH, FLOOR.AGENT_BODY_HEIGHT, 0.12]} />
                            <meshStandardMaterial color={roleColor} roughness={0.7} />
                        </mesh>
                        {/* Head */}
                        <mesh position={[0, FLOOR.DESK_Y + FLOOR.AGENT_BODY_HEIGHT / 2 - 0.03, 0]}>
                            <sphereGeometry args={[FLOOR.AGENT_HEAD_RADIUS, 12, 12]} />
                            <meshStandardMaterial color="#d4a574" roughness={0.8} />
                        </mesh>
                    </group>
                );
            })}
        </group>
    );
}
