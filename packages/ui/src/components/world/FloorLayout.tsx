import { Agent } from '../../lib/types';
import { FLOOR } from './worldConstants';

export interface DeskAssignment {
    agent: Agent;
    position: [number, number, number];
    scale: number;
}

export interface FloorLayoutResult {
    desks: DeskAssignment[];
    breakRoomPosition: [number, number, number];
    breakRoomWidth: number;
    breakRoomDepth: number;
    floorWidth: number;
    floorDepth: number;
}

export function computeFloorLayout(agents: Agent[]): FloorLayoutResult {
    const ceo = agents.find(a => a.role === 'CEO');
    const managers = agents.filter(a => a.role === 'MANAGER');
    const workers = agents.filter(a => a.role === 'WORKER');

    const desks: DeskAssignment[] = [];

    // Calculate floor dimensions based on agent count
    const totalAgents = agents.length;
    const cols = Math.max(3, Math.ceil(Math.sqrt(totalAgents + 2))); // +2 for break room space
    const rows = Math.max(2, Math.ceil((totalAgents + 2) / cols));

    const floorWidth = cols * FLOOR.DESK_SPACING_X + 2;
    const floorDepth = rows * FLOOR.DESK_SPACING_Z + 3;

    let currentX = -floorWidth / 2 + FLOOR.DESK_SPACING_X;
    let currentZ = -floorDepth / 2 + FLOOR.DESK_SPACING_Z;

    // CEO gets corner office (top-left, larger desk)
    if (ceo) {
        desks.push({
            agent: ceo,
            position: [-floorWidth / 2 + FLOOR.DESK_SPACING_X, 0, -floorDepth / 2 + FLOOR.DESK_SPACING_Z],
            scale: FLOOR.CEO_DESK_SCALE,
        });
    }

    // Managers get slightly larger desks, placed after CEO
    managers.forEach((manager, i) => {
        const col = (i + 1) % (cols - 1);
        const row = Math.floor((i + 1) / (cols - 1));
        desks.push({
            agent: manager,
            position: [
                -floorWidth / 2 + FLOOR.DESK_SPACING_X + (col + 1) * FLOOR.DESK_SPACING_X,
                0,
                -floorDepth / 2 + FLOOR.DESK_SPACING_Z + row * FLOOR.DESK_SPACING_Z,
            ],
            scale: FLOOR.MANAGER_DESK_SCALE,
        });
    });

    // Workers fill remaining spots
    const usedPositions = desks.length;
    workers.forEach((worker, i) => {
        const idx = usedPositions + i;
        const col = idx % cols;
        const row = Math.floor(idx / cols);
        desks.push({
            agent: worker,
            position: [
                -floorWidth / 2 + FLOOR.DESK_SPACING_X + col * FLOOR.DESK_SPACING_X,
                0,
                -floorDepth / 2 + FLOOR.DESK_SPACING_Z + (row + 1) * FLOOR.DESK_SPACING_Z,
            ],
            scale: 1,
        });
    });

    // Break room in the back-right corner
    const breakRoomWidth = Math.max(2.5, FLOOR.DESK_SPACING_X * 1.5);
    const breakRoomDepth = Math.max(2, FLOOR.DESK_SPACING_Z * 1.2);

    return {
        desks,
        breakRoomPosition: [
            floorWidth / 2 - breakRoomWidth / 2 - 0.5,
            0,
            floorDepth / 2 - breakRoomDepth / 2 - 0.5,
        ],
        breakRoomWidth,
        breakRoomDepth,
        floorWidth,
        floorDepth,
    };
}
