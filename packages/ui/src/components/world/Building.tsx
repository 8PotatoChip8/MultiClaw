import { useRef, useState } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { BuildingData } from './worldTypes';
import { COLORS } from './worldConstants';
import BuildingLabel from './BuildingLabel';

interface BuildingProps {
    data: BuildingData;
    onClick: (companyId: string) => void;
}

export default function Building({ data, onClick }: BuildingProps) {
    const meshRef = useRef<THREE.Mesh>(null!);
    const [hovered, setHovered] = useState(false);
    const targetEmissive = useRef(0);

    useFrame(() => {
        if (!meshRef.current) return;
        const mat = meshRef.current.material as THREE.MeshStandardMaterial;
        targetEmissive.current = hovered ? 0.3 : 0;
        mat.emissiveIntensity += (targetEmissive.current - mat.emissiveIntensity) * 0.1;
    });

    const darkColor = data.company.type === 'INTERNAL' ? COLORS.INTERNAL_DARK : COLORS.EXTERNAL_DARK;

    // Create window pattern effect using slightly different material properties
    return (
        <group position={data.position}>
            {/* Main building body */}
            <mesh
                ref={meshRef}
                castShadow
                receiveShadow
                onPointerOver={(e) => { e.stopPropagation(); setHovered(true); document.body.style.cursor = 'pointer'; }}
                onPointerOut={() => { setHovered(false); document.body.style.cursor = 'default'; }}
                onClick={(e) => { e.stopPropagation(); onClick(data.company.id); }}
            >
                <boxGeometry args={[data.width, data.height, data.depth]} />
                <meshStandardMaterial
                    color={data.color}
                    emissive={data.color}
                    emissiveIntensity={0}
                    roughness={0.7}
                    metalness={0.1}
                />
            </mesh>

            {/* Roof accent */}
            <mesh position={[0, data.height / 2 + 0.06, 0]} castShadow>
                <boxGeometry args={[data.width + 0.1, 0.12, data.depth + 0.1]} />
                <meshStandardMaterial color={darkColor} roughness={0.5} metalness={0.2} />
            </mesh>

            {/* Window rows - front face */}
            {Array.from({ length: Math.floor(data.height / 0.6) }).map((_, row) => (
                Array.from({ length: Math.max(2, Math.floor(data.width / 0.4)) }).map((_, col) => (
                    <mesh
                        key={`fw-${row}-${col}`}
                        position={[
                            (col - (Math.max(2, Math.floor(data.width / 0.4)) - 1) / 2) * 0.35,
                            -data.height / 2 + 0.5 + row * 0.6,
                            data.depth / 2 + 0.001,
                        ]}
                    >
                        <planeGeometry args={[0.2, 0.3]} />
                        <meshStandardMaterial
                            color="#1a2a44"
                            emissive="#2a4a7a"
                            emissiveIntensity={0.4}
                            transparent
                            opacity={0.9}
                        />
                    </mesh>
                ))
            ))}

            {/* Label */}
            <BuildingLabel
                name={data.company.name}
                agentCount={data.agentCount}
                type={data.company.type}
                height={data.height}
            />
        </group>
    );
}
