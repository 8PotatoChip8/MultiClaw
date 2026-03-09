'use client';
import { useState, useEffect, useCallback, useMemo } from 'react';
import { Agent } from './types';

function getWsUrl() {
    if (process.env.NEXT_PUBLIC_WS_URL) return process.env.NEXT_PUBLIC_WS_URL;
    if (typeof window !== 'undefined') {
        const wsProto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        return `${wsProto}//${window.location.hostname}:8080/v1/events`;
    }
    return 'ws://localhost:8080/v1/events';
}

const WS_URL = typeof window !== 'undefined' ? getWsUrl() : 'ws://localhost:8080/v1/events';

export function useMultiClawEvents() {
    const [lastEvent, setLastEvent] = useState<any>(null);

    useEffect(() => {
        let ws: WebSocket | null = null;
        let reconnectTimer: ReturnType<typeof setTimeout>;

        const connect = () => {
            try {
                ws = new WebSocket(WS_URL);
                ws.onopen = () => console.log('[WS] Connected to event stream');
                ws.onmessage = (event) => {
                    try {
                        const data = JSON.parse(event.data);
                        setLastEvent(data);
                    } catch {
                        setLastEvent({ raw: event.data });
                    }
                };
                ws.onclose = () => {
                    console.log('[WS] Disconnected, reconnecting in 3s...');
                    reconnectTimer = setTimeout(connect, 3000);
                };
                ws.onerror = () => ws?.close();
            } catch {
                reconnectTimer = setTimeout(connect, 3000);
            }
        };

        connect();

        return () => {
            clearTimeout(reconnectTimer);
            ws?.close();
        };
    }, []);

    return lastEvent;
}

// ── Agent Presence ──────────────────────────────────────────────

export type PresenceStatus = 'Active' | 'Busy' | 'Offline';

export interface AgentPresence {
    presenceStatus: PresenceStatus;
    task: string | null;
}

export function derivePresence(
    dbStatus: string,
    activity?: { status: string; task?: string | null } | null
): AgentPresence {
    if (dbStatus === 'QUARANTINED') {
        return { presenceStatus: 'Offline', task: null };
    }
    if (activity?.status === 'WORKING') {
        return { presenceStatus: 'Busy', task: activity.task ?? null };
    }
    return { presenceStatus: 'Active', task: null };
}

export function useAgentPresence(initialAgents: Agent[]): Record<string, AgentPresence> {
    const [presenceMap, setPresenceMap] = useState<Record<string, AgentPresence>>({});
    const event = useMultiClawEvents();

    // Seed from initial agent list (includes activity from enriched API)
    useEffect(() => {
        const map: Record<string, AgentPresence> = {};
        for (const a of initialAgents) {
            map[a.id] = derivePresence(a.status, a.activity);
        }
        setPresenceMap(map);
    }, [initialAgents]);

    // Apply real-time WS updates
    useEffect(() => {
        if (!event || event.type !== 'agent_activity_changed') return;
        setPresenceMap(prev => {
            const agentId = event.agent_id;
            // Preserve the DB lifecycle status from the seed
            const current = prev[agentId];
            const isQuarantined = current?.presenceStatus === 'Offline';
            const dbStatus = isQuarantined ? 'QUARANTINED' : 'ACTIVE';
            return {
                ...prev,
                [agentId]: derivePresence(dbStatus, {
                    status: event.status,
                    task: event.task ?? null,
                }),
            };
        });
    }, [event]);

    // Also handle quarantine events
    useEffect(() => {
        if (!event || event.type !== 'agent_quarantined') return;
        setPresenceMap(prev => ({
            ...prev,
            [event.agent_id]: { presenceStatus: 'Offline' as PresenceStatus, task: null },
        }));
    }, [event]);

    return presenceMap;
}
