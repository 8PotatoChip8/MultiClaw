import { useEffect, useState } from 'react';

const WS_URL = process.env.NEXT_PUBLIC_WS_URL || 'ws://localhost:8080/v1/events';

export function useMultiClawEvents() {
    const [lastEvent, setLastEvent] = useState<any>(null);

    useEffect(() => {
        const ws = new WebSocket(WS_URL);

        ws.onmessage = (event) => {
            try {
                const data = JSON.parse(event.data);
                setLastEvent(data);
            } catch (e) {
                console.error("WS Parse error", e);
            }
        };

        return () => {
            ws.close();
        };
    }, []);

    return lastEvent;
}
