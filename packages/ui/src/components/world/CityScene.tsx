import { Canvas } from '@react-three/fiber';
import { OrbitControls } from '@react-three/drei';
import { COLORS } from './worldConstants';
import { BuildingData } from './worldTypes';
import Building from './Building';

interface CitySceneProps {
    buildings: BuildingData[];
    onBuildingClick: (companyId: string) => void;
}

function CityGround() {
    return (
        <group>
            {/* Ground plane */}
            <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, -0.01, 0]} receiveShadow>
                <planeGeometry args={[80, 80]} />
                <meshStandardMaterial color={COLORS.GROUND} roughness={0.9} />
            </mesh>
            {/* Grid lines */}
            <gridHelper args={[80, 40, COLORS.GRID, COLORS.GRID]} position={[0, 0.01, 0]} />
        </group>
    );
}

export default function CityScene({ buildings, onBuildingClick }: CitySceneProps) {
    return (
        <Canvas
            camera={{ position: [18, 22, 18], fov: 50 }}
            shadows
            style={{ background: COLORS.SKY }}
            gl={{ antialias: true, alpha: false }}
        >
            {/* Lighting */}
            <ambientLight intensity={0.4} />
            <directionalLight
                position={[15, 25, 10]}
                intensity={0.8}
                castShadow
                shadow-mapSize-width={1024}
                shadow-mapSize-height={1024}
                shadow-camera-far={60}
                shadow-camera-left={-20}
                shadow-camera-right={20}
                shadow-camera-top={20}
                shadow-camera-bottom={-20}
            />
            <pointLight position={[-10, 15, -10]} intensity={0.3} color="#8b5cf6" />

            {/* Ground */}
            <CityGround />

            {/* Buildings */}
            {buildings.map((b) => (
                <Building key={b.company.id} data={b} onClick={onBuildingClick} />
            ))}

            {/* Camera controls */}
            <OrbitControls
                enablePan
                enableZoom
                enableRotate
                minDistance={8}
                maxDistance={60}
                maxPolarAngle={Math.PI / 2.2}
                target={[0, 0, 0]}
            />
        </Canvas>
    );
}
