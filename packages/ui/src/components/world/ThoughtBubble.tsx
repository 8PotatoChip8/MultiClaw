import { useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import * as THREE from 'three';

interface ThoughtBubbleProps {
    position: [number, number, number];
    text: string;
}

export default function ThoughtBubble({ position, text }: ThoughtBubbleProps) {
    const groupRef = useRef<THREE.Group>(null!);
    const timeRef = useRef(Math.random() * Math.PI * 2);

    useFrame((_, delta) => {
        if (!groupRef.current) return;
        timeRef.current += delta * 1.5;
        groupRef.current.position.y = position[1] + Math.sin(timeRef.current) * 0.03;
    });

    const truncated = text && text.length > 30 ? text.slice(0, 30) + '...' : text;

    return (
        <group ref={groupRef} position={position}>
            <Html center distanceFactor={6} style={{ pointerEvents: 'none' }}>
                <div style={{
                    background: 'rgba(22, 30, 52, 0.92)',
                    border: '1px solid rgba(59, 130, 246, 0.25)',
                    borderRadius: '8px',
                    padding: '3px 8px',
                    whiteSpace: 'nowrap',
                    fontFamily: 'Inter, sans-serif',
                    fontSize: '9px',
                    color: '#e8ecf4',
                    maxWidth: '140px',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    backdropFilter: 'blur(6px)',
                }}>
                    💭 {truncated || 'Working...'}
                </div>
            </Html>
        </group>
    );
}
