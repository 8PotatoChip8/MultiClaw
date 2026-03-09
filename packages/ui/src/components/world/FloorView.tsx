import { useMemo } from 'react';
import { Canvas } from '@react-three/fiber';
import { OrbitControls, Html } from '@react-three/drei';
import { COLORS, FLOOR } from './worldConstants';
import { WorldSnapshot } from './worldTypes';
import { Company, Agent } from '../../lib/types';
import { computeFloorLayout } from './FloorLayout';
import FloorAgent from './FloorAgent';
import BreakRoom from './BreakRoom';

interface FloorViewProps {
    company: Company;
    snapshot: WorldSnapshot;
    onBack: () => void;
}

function FloorShell({ width, depth }: { width: number; depth: number }) {
    const h = FLOOR.ROOM_HEIGHT;
    const t = FLOOR.WALL_THICKNESS;

    return (
        <group>
            {/* Floor */}
            <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0, 0]} receiveShadow>
                <planeGeometry args={[width + 1, depth + 1]} />
                <meshStandardMaterial color={COLORS.FLOOR} roughness={0.9} />
            </mesh>

            {/* Ceiling */}
            <mesh rotation={[Math.PI / 2, 0, 0]} position={[0, h, 0]}>
                <planeGeometry args={[width + 1, depth + 1]} />
                <meshStandardMaterial color="#1a2030" roughness={0.9} />
            </mesh>

            {/* Back wall */}
            <mesh position={[0, h / 2, -depth / 2 - 0.5]}>
                <boxGeometry args={[width + 1, h, t]} />
                <meshStandardMaterial color={COLORS.WALL} roughness={0.8} />
            </mesh>

            {/* Left wall */}
            <mesh position={[-width / 2 - 0.5, h / 2, 0]}>
                <boxGeometry args={[t, h, depth + 1]} />
                <meshStandardMaterial color={COLORS.WALL} roughness={0.8} />
            </mesh>

            {/* Right wall */}
            <mesh position={[width / 2 + 0.5, h / 2, 0]}>
                <boxGeometry args={[t, h, depth + 1]} />
                <meshStandardMaterial color={COLORS.WALL} roughness={0.8} />
            </mesh>

            {/* No front wall — cutaway view */}
        </group>
    );
}

export default function FloorView({ company, snapshot, onBack }: FloorViewProps) {
    const companyAgents = useMemo(
        () => snapshot.agents.filter(a => a.company_id === company.id),
        [snapshot.agents, company.id]
    );

    const layout = useMemo(
        () => computeFloorLayout(companyAgents),
        [companyAgents]
    );

    const idleAgents = useMemo(
        () => companyAgents.filter(a => {
            const activity = snapshot.activities[a.id];
            return !activity || activity.status === 'IDLE';
        }),
        [companyAgents, snapshot.activities]
    );

    if (companyAgents.length === 0) {
        return (
            <div style={{
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                height: '100%',
                minHeight: 'calc(100vh - 64px)',
                gap: '16px',
            }}>
                <p style={{ color: 'var(--text-muted)', fontSize: '16px' }}>
                    {company.name} has no employees yet.
                </p>
                <button
                    className="button secondary small"
                    onClick={onBack}
                    style={{ cursor: 'pointer' }}
                >
                    ← Back to City
                </button>
            </div>
        );
    }

    return (
        <div style={{ position: 'relative', width: '100%', height: 'calc(100vh - 64px)' }}>
            {/* Back button overlay */}
            <div style={{
                position: 'absolute',
                top: '16px',
                left: '16px',
                zIndex: 10,
            }}>
                <button
                    className="button secondary small"
                    onClick={onBack}
                    style={{
                        cursor: 'pointer',
                        background: 'rgba(10, 14, 26, 0.85)',
                        backdropFilter: 'blur(8px)',
                    }}
                >
                    ← Back to City
                </button>
            </div>

            {/* Company name overlay */}
            <div style={{
                position: 'absolute',
                top: '16px',
                left: '50%',
                transform: 'translateX(-50%)',
                zIndex: 10,
                background: 'rgba(10, 14, 26, 0.85)',
                border: '1px solid var(--border)',
                borderRadius: '8px',
                padding: '6px 16px',
                backdropFilter: 'blur(8px)',
            }}>
                <span style={{
                    color: 'var(--text)',
                    fontSize: '14px',
                    fontWeight: 600,
                    fontFamily: 'Inter, sans-serif',
                }}>
                    {company.name}
                </span>
                <span style={{
                    color: 'var(--text-muted)',
                    fontSize: '12px',
                    marginLeft: '8px',
                    fontFamily: 'Inter, sans-serif',
                }}>
                    {companyAgents.length} employee{companyAgents.length !== 1 ? 's' : ''}
                </span>
            </div>

            <Canvas
                camera={{
                    position: [0, layout.floorDepth * 0.6, layout.floorDepth * 1.2],
                    fov: 45,
                }}
                shadows
                style={{ background: COLORS.SKY }}
                gl={{ antialias: true, alpha: false }}
            >
                {/* Lighting */}
                <ambientLight intensity={0.5} />
                <directionalLight
                    position={[5, 8, 10]}
                    intensity={0.6}
                    castShadow
                    shadow-mapSize-width={1024}
                    shadow-mapSize-height={1024}
                />
                <pointLight position={[0, FLOOR.ROOM_HEIGHT - 0.3, 0]} intensity={0.4} color="#ffffff" />

                {/* Building shell */}
                <FloorShell width={layout.floorWidth} depth={layout.floorDepth} />

                {/* Agent desks */}
                {layout.desks.map((desk) => (
                    <FloorAgent
                        key={desk.agent.id}
                        agent={desk.agent}
                        activity={snapshot.activities[desk.agent.id] || null}
                        vmState={snapshot.vm_states[desk.agent.id] || null}
                        deskPosition={desk.position}
                        deskScale={desk.scale}
                    />
                ))}

                {/* Break room */}
                <BreakRoom
                    position={layout.breakRoomPosition}
                    width={layout.breakRoomWidth}
                    depth={layout.breakRoomDepth}
                    idleAgents={idleAgents}
                />

                {/* Room labels */}
                <Html
                    position={[layout.breakRoomPosition[0], FLOOR.ROOM_HEIGHT - 0.3, layout.breakRoomPosition[2]]}
                    center
                    distanceFactor={8}
                    style={{ pointerEvents: 'none' }}
                >
                    <div style={{
                        color: COLORS.LABEL_MUTED,
                        fontSize: '10px',
                        fontFamily: 'Inter, sans-serif',
                        fontWeight: 500,
                    }}>
                        Break Room
                    </div>
                </Html>

                {layout.desks.length > 0 && layout.desks[0].agent.role === 'CEO' && (
                    <Html
                        position={[layout.desks[0].position[0], FLOOR.ROOM_HEIGHT - 0.3, layout.desks[0].position[2]]}
                        center
                        distanceFactor={8}
                        style={{ pointerEvents: 'none' }}
                    >
                        <div style={{
                            color: COLORS.LABEL_MUTED,
                            fontSize: '10px',
                            fontFamily: 'Inter, sans-serif',
                            fontWeight: 500,
                        }}>
                            CEO Office
                        </div>
                    </Html>
                )}

                {/* Camera controls — pan and zoom only, no rotation for cutaway */}
                <OrbitControls
                    enablePan
                    enableZoom
                    enableRotate
                    minDistance={3}
                    maxDistance={25}
                    maxPolarAngle={Math.PI / 2.1}
                />
            </Canvas>
        </div>
    );
}
