import { useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import * as THREE from 'three';
import { COLORS, FLOOR } from './worldConstants';
import { VmState } from './worldTypes';

interface AgentDeskProps {
    position: [number, number, number];
    scale?: number;
    agentName: string;
    role: string;
    status: string;
    vmState: VmState | null;
    isOccupied: boolean;
    isWorking: boolean;
}

export default function AgentDesk({
    position,
    scale = 1,
    agentName,
    role,
    status,
    vmState,
    isOccupied,
    isWorking,
}: AgentDeskProps) {
    const agentHeadRef = useRef<THREE.Mesh>(null!);
    const timeRef = useRef(Math.random() * Math.PI * 2);

    const desktopOn = vmState?.desktop === 'RUNNING';
    const sandboxOn = vmState?.sandbox === 'RUNNING';
    const isQuarantined = status === 'QUARANTINED';

    // Subtle typing animation when working
    useFrame((_, delta) => {
        if (!agentHeadRef.current || !isOccupied || !isWorking) return;
        timeRef.current += delta * 3;
        agentHeadRef.current.position.y =
            FLOOR.DESK_Y + FLOOR.AGENT_BODY_HEIGHT + FLOOR.AGENT_HEAD_RADIUS + Math.sin(timeRef.current) * 0.015;
    });

    const deskW = FLOOR.DESK_WIDTH * scale;
    const deskD = FLOOR.DESK_DEPTH * scale;

    const bodyColor = isQuarantined
        ? COLORS.QUARANTINED
        : role === 'CEO' ? COLORS.CEO
        : role === 'MANAGER' ? COLORS.MANAGER
        : COLORS.WORKER;

    return (
        <group position={position}>
            {/* Desk surface */}
            <mesh position={[0, FLOOR.DESK_Y, 0]} castShadow>
                <boxGeometry args={[deskW, FLOOR.DESK_HEIGHT, deskD]} />
                <meshStandardMaterial color={isQuarantined ? '#3a2020' : COLORS.DESK} roughness={0.6} />
            </mesh>

            {/* Desk legs */}
            {[[-deskW / 2 + 0.05, FLOOR.DESK_Y / 2, -deskD / 2 + 0.05],
              [deskW / 2 - 0.05, FLOOR.DESK_Y / 2, -deskD / 2 + 0.05],
              [-deskW / 2 + 0.05, FLOOR.DESK_Y / 2, deskD / 2 - 0.05],
              [deskW / 2 - 0.05, FLOOR.DESK_Y / 2, deskD / 2 - 0.05]].map((pos, i) => (
                <mesh key={`leg-${i}`} position={pos as [number, number, number]}>
                    <boxGeometry args={[0.04, FLOOR.DESK_Y, 0.04]} />
                    <meshStandardMaterial color={COLORS.DESK} roughness={0.7} />
                </mesh>
            ))}

            {/* Desktop monitor (left) */}
            <mesh position={[-0.18 * scale, FLOOR.DESK_Y + FLOOR.MONITOR_HEIGHT / 2 + 0.02, -deskD / 2 + 0.08]}>
                <boxGeometry args={[FLOOR.MONITOR_WIDTH, FLOOR.MONITOR_HEIGHT, FLOOR.MONITOR_DEPTH]} />
                <meshStandardMaterial
                    color={desktopOn ? '#0a2a1a' : COLORS.MONITOR_OFF}
                    emissive={desktopOn ? COLORS.DESKTOP_ON : '#000000'}
                    emissiveIntensity={desktopOn ? 0.6 : 0}
                    roughness={0.3}
                />
            </mesh>

            {/* Sandbox monitor (right) */}
            <mesh position={[0.18 * scale, FLOOR.DESK_Y + FLOOR.MONITOR_HEIGHT / 2 + 0.02, -deskD / 2 + 0.08]}>
                <boxGeometry args={[FLOOR.MONITOR_WIDTH, FLOOR.MONITOR_HEIGHT, FLOOR.MONITOR_DEPTH]} />
                <meshStandardMaterial
                    color={sandboxOn ? '#0a1a2a' : COLORS.MONITOR_OFF}
                    emissive={sandboxOn ? COLORS.SANDBOX_ON : '#000000'}
                    emissiveIntensity={sandboxOn ? 0.6 : 0}
                    roughness={0.3}
                />
            </mesh>

            {/* Monitor stands */}
            {[-0.18 * scale, 0.18 * scale].map((x, i) => (
                <mesh key={`stand-${i}`} position={[x, FLOOR.DESK_Y + 0.01, -deskD / 2 + 0.08]}>
                    <boxGeometry args={[0.04, 0.02, 0.06]} />
                    <meshStandardMaterial color="#222" />
                </mesh>
            ))}

            {/* Chair */}
            <mesh position={[0, FLOOR.CHAIR_HEIGHT / 2, deskD / 2 + 0.15]}>
                <cylinderGeometry args={[FLOOR.CHAIR_RADIUS, FLOOR.CHAIR_RADIUS * 0.8, FLOOR.CHAIR_HEIGHT, 8]} />
                <meshStandardMaterial
                    color={role === 'CEO' ? '#2a3a5a' : role === 'MANAGER' ? '#2a4a3a' : COLORS.CHAIR}
                    roughness={0.6}
                />
            </mesh>

            {/* Agent figure (only if at desk) */}
            {isOccupied && (
                <group position={[0, 0, deskD / 2 + 0.15]}>
                    {/* Body */}
                    <mesh position={[0, FLOOR.DESK_Y, 0]}>
                        <boxGeometry args={[FLOOR.AGENT_BODY_WIDTH, FLOOR.AGENT_BODY_HEIGHT, 0.12]} />
                        <meshStandardMaterial color={bodyColor} roughness={0.7} />
                    </mesh>
                    {/* Head */}
                    <mesh
                        ref={agentHeadRef}
                        position={[0, FLOOR.DESK_Y + FLOOR.AGENT_BODY_HEIGHT / 2 + FLOOR.AGENT_HEAD_RADIUS + 0.02, 0]}
                    >
                        <sphereGeometry args={[FLOOR.AGENT_HEAD_RADIUS, 12, 12]} />
                        <meshStandardMaterial color={isQuarantined ? '#8a6666' : '#d4a574'} roughness={0.8} />
                    </mesh>
                </group>
            )}

            {/* Name plate */}
            <Html
                position={[0, 0.02, deskD / 2 + 0.35]}
                center
                distanceFactor={6}
                style={{ pointerEvents: 'none' }}
            >
                <div style={{
                    color: isQuarantined ? COLORS.QUARANTINED : COLORS.LABEL,
                    fontSize: '8px',
                    fontFamily: 'Inter, sans-serif',
                    fontWeight: 500,
                    whiteSpace: 'nowrap',
                    textAlign: 'center',
                    opacity: 0.85,
                }}>
                    {agentName}
                    {isQuarantined && <span style={{ display: 'block', fontSize: '7px', color: COLORS.QUARANTINED }}>QUARANTINED</span>}
                </div>
            </Html>
        </group>
    );
}
