'use client';
import { useState, useEffect, useCallback } from 'react';

const WS_URL = process.env.NEXT_PUBLIC_WS_URL || 'ws://localhost:8080/v1/events';

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
