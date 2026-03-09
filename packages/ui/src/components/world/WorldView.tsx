'use client';
import { useState, useMemo } from 'react';
import { useWorldData } from './useWorldData';
import { computeCityLayout } from './worldUtils';
import WorldLoadingState from './WorldLoadingState';
import CityScene from './CityScene';
import FloorView from './FloorView';
import { ViewMode } from './worldTypes';

export default function WorldView() {
    const { worldData, loading } = useWorldData();
    const [viewMode, setViewMode] = useState<ViewMode>('city');
    const [selectedCompanyId, setSelectedCompanyId] = useState<string | null>(null);

    const buildings = useMemo(() => {
        if (!worldData) return [];
        // Exclude MAIN agent's "company" — only show real companies
        return computeCityLayout(worldData.companies, worldData);
    }, [worldData]);

    const selectedCompany = useMemo(() => {
        if (!selectedCompanyId || !worldData) return null;
        return worldData.companies.find(c => c.id === selectedCompanyId) || null;
    }, [selectedCompanyId, worldData]);

    if (loading || !worldData) {
        return <WorldLoadingState />;
    }

    if (viewMode === 'floor' && selectedCompany) {
        return (
            <FloorView
                company={selectedCompany}
                snapshot={worldData}
                onBack={() => {
                    setViewMode('city');
                    setSelectedCompanyId(null);
                }}
            />
        );
    }

    return (
        <div style={{ position: 'relative', width: '100%', height: 'calc(100vh - 64px)' }}>
            {buildings.length === 0 && (
                <div style={{
                    position: 'absolute',
                    top: '50%',
                    left: '50%',
                    transform: 'translate(-50%, -50%)',
                    zIndex: 10,
                    textAlign: 'center',
                    color: 'var(--text-muted)',
                    fontSize: '16px',
                }}>
                    <p>No companies yet. Create one to see it appear here.</p>
                </div>
            )}
            <CityScene
                buildings={buildings}
                onBuildingClick={(companyId) => {
                    setSelectedCompanyId(companyId);
                    setViewMode('floor');
                }}
            />
        </div>
    );
}
