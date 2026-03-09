import { useEffect, useState, useCallback } from 'react';
import { api } from '../../lib/api';
import { useMultiClawEvents } from '../../lib/ws';
import { WorldSnapshot } from './worldTypes';

export function useWorldData() {
    const [worldData, setWorldData] = useState<WorldSnapshot | null>(null);
    const [loading, setLoading] = useState(true);
    const event = useMultiClawEvents();

    const fetchData = useCallback(() => {
        api.getWorldSnapshot()
            .then((data: WorldSnapshot) => {
                setWorldData(data);
                setLoading(false);
            })
            .catch(() => setLoading(false));
    }, []);

    // Initial fetch
    useEffect(() => {
        fetchData();
    }, [fetchData]);

    // Periodic refresh as fallback (every 30s)
    useEffect(() => {
        const interval = setInterval(fetchData, 30000);
        return () => clearInterval(interval);
    }, [fetchData]);

    // Real-time event handling
    useEffect(() => {
        if (!event || !worldData) return;
        switch (event.type) {
            case 'agent_activity_changed':
                setWorldData(prev => {
                    if (!prev) return prev;
                    return {
                        ...prev,
                        activities: {
                            ...prev.activities,
                            [event.agent_id]: {
                                agent_id: event.agent_id,
                                status: event.status,
                                task: event.task || null,
                                since: new Date().toISOString(),
                            }
                        }
                    };
                });
                break;
            case 'company_created':
            case 'ceo_hired':
            case 'vm_provisioned':
            case 'sandbox_provisioned':
            case 'agent_quarantined':
                // Structural changes — full refresh
                fetchData();
                break;
        }
    }, [event]); // eslint-disable-line react-hooks/exhaustive-deps

    return { worldData, loading };
}
