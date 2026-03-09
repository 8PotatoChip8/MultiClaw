import { BUILDING, COLORS } from './worldConstants';
import { BuildingData, WorldSnapshot } from './worldTypes';
import { Company } from '../../lib/types';

function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
}

export function computeBuildingDimensions(
    agentCount: number,
    netBalance: number,
    totalCapital: number
): { width: number; height: number; depth: number } {
    const agentScale = clamp(0.5 + agentCount * 0.35, BUILDING.MIN_WIDTH, BUILDING.MAX_WIDTH);
    const financialValue = Math.abs(netBalance) + totalCapital;
    const heightScale = clamp(
        BUILDING.MIN_HEIGHT + Math.log10(financialValue + 1) * 1.2,
        BUILDING.MIN_HEIGHT,
        BUILDING.MAX_HEIGHT
    );
    return {
        width: agentScale,
        height: heightScale,
        depth: agentScale,
    };
}

export function computeCityLayout(
    companies: Company[],
    snapshot: WorldSnapshot
): BuildingData[] {
    const buildings: BuildingData[] = companies.map((company) => {
        const agents = snapshot.agents.filter(a => a.company_id === company.id);
        const agentCount = agents.length;

        // Sum net balance across all currencies
        const companyBalances = snapshot.balances[company.id] || {};
        let netBalance = 0;
        let totalCapital = 0;
        for (const curr of Object.values(companyBalances)) {
            netBalance += curr.net || 0;
            totalCapital += curr.capital || 0;
        }

        const dims = computeBuildingDimensions(agentCount, netBalance, totalCapital);
        const color = company.type === 'INTERNAL' ? COLORS.INTERNAL : COLORS.EXTERNAL;

        return {
            company,
            agentCount,
            netBalance,
            totalCapital,
            position: [0, 0, 0] as [number, number, number],
            height: dims.height,
            width: dims.width,
            depth: dims.depth,
            color,
        };
    });

    // Arrange in a grid layout centered at origin
    const count = buildings.length;
    if (count === 0) return buildings;

    const cols = Math.ceil(Math.sqrt(count));
    const totalWidth = (cols - 1) * BUILDING.SPACING;

    buildings.forEach((b, i) => {
        const col = i % cols;
        const row = Math.floor(i / cols);
        b.position = [
            col * BUILDING.SPACING - totalWidth / 2,
            b.height / 2,
            row * BUILDING.SPACING - totalWidth / 2,
        ];
    });

    return buildings;
}
