'use client';
import { useEffect, useState, useCallback } from 'react';
import { api } from '../../lib/api';
import { Server, RefreshCw, ChevronDown, ChevronUp, Clock, HardDrive, X, Copy, Check } from 'lucide-react';

interface DockerContainer {
    ID: string;
    Names: string;
    Image: string;
    Status: string;
    State: string;
    RunningFor: string;
    Ports: string;
    Size: string;
}

export default function InfrastructurePage() {
    const [containers, setContainers] = useState<DockerContainer[]>([]);
    const [loading, setLoading] = useState(true);
    const [expandedLogs, setExpandedLogs] = useState<Record<string, string | null>>({});
    const [logsLoading, setLogsLoading] = useState<Record<string, boolean>>({});
    const [autoRefresh, setAutoRefresh] = useState(true);
    const [copiedId, setCopiedId] = useState<string | null>(null);

    const copyLogs = (containerId: string) => {
        const logs = expandedLogs[containerId];
        if (!logs) return;

        // Try modern clipboard API first (works on HTTPS / localhost)
        if (navigator.clipboard && window.isSecureContext) {
            navigator.clipboard.writeText(logs).then(() => {
                setCopiedId(containerId);
                setTimeout(() => setCopiedId(null), 2000);
            });
        } else {
            // Fallback for HTTP: hidden textarea + execCommand
            const textarea = document.createElement('textarea');
            textarea.value = logs;
            textarea.style.position = 'fixed';
            textarea.style.opacity = '0';
            document.body.appendChild(textarea);
            textarea.select();
            try {
                document.execCommand('copy');
                setCopiedId(containerId);
                setTimeout(() => setCopiedId(null), 2000);
            } catch (e) {
                console.error('Copy failed:', e);
            }
            document.body.removeChild(textarea);
        }
    };

    const fetchContainers = useCallback(async () => {
        try {
            const data = await api.getContainers();
            setContainers(Array.isArray(data) ? data : []);
        } catch (e) {
            console.error('Failed to fetch containers:', e);
        }
        setLoading(false);
    }, []);

    useEffect(() => {
        fetchContainers();
        if (!autoRefresh) return;
        const interval = setInterval(fetchContainers, 10000);
        return () => clearInterval(interval);
    }, [fetchContainers, autoRefresh]);

    const toggleLogs = async (containerId: string, containerName: string) => {
        if (expandedLogs[containerId] !== undefined) {
            setExpandedLogs(prev => { const n = { ...prev }; delete n[containerId]; return n; });
            return;
        }
        setLogsLoading(prev => ({ ...prev, [containerId]: true }));
        try {
            const nameClean = containerName.replace(/^\//, '');
            const data = await api.getContainerLogs(nameClean, 200);
            setExpandedLogs(prev => ({ ...prev, [containerId]: data?.logs || 'No logs available' }));
        } catch {
            setExpandedLogs(prev => ({ ...prev, [containerId]: 'Failed to fetch logs' }));
        }
        setLogsLoading(prev => ({ ...prev, [containerId]: false }));
    };

    const stateColor = (state: string) => {
        switch (state?.toLowerCase()) {
            case 'running': return 'var(--success)';
            case 'exited': return '#ef4444';
            case 'restarting': return '#f59e0b';
            default: return 'var(--text-muted)';
        }
    };

    return (
        <div className="animate-in">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '28px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>Infrastructure</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Monitor Docker containers and services</p>
                </div>
                <div style={{ display: 'flex', gap: '10px', alignItems: 'center' }}>
                    <label style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '13px', color: 'var(--text-muted)', cursor: 'pointer' }}>
                        <input type="checkbox" checked={autoRefresh} onChange={() => setAutoRefresh(!autoRefresh)}
                            style={{ accentColor: 'var(--primary)' }} />
                        Auto-refresh
                    </label>
                    <button className="button" onClick={fetchContainers}
                        style={{ display: 'flex', alignItems: 'center', gap: '6px', background: 'rgba(255,255,255,0.06)', border: '1px solid var(--border)' }}>
                        <RefreshCw size={14} /> Refresh
                    </button>
                </div>
            </div>

            {/* Summary */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: '16px', marginBottom: '24px' }}>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700 }}>{containers.length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Total Containers</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700, color: 'var(--success)' }}>
                        {containers.filter(c => c.State === 'running').length}
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Running</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700, color: '#ef4444' }}>
                        {containers.filter(c => c.State === 'exited').length}
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Stopped</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700, color: '#f59e0b' }}>
                        {containers.filter(c => c.State === 'restarting').length}
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Restarting</div>
                </div>
            </div>

            {/* Container cards */}
            {loading ? (
                <div className="panel" style={{ textAlign: 'center', padding: '60px' }}>
                    <RefreshCw size={24} style={{ color: 'var(--text-muted)', animation: 'spin 1s linear infinite' }} />
                    <p style={{ color: 'var(--text-muted)', marginTop: '12px' }}>Loading containers...</p>
                </div>
            ) : containers.length === 0 ? (
                <div className="panel" style={{ textAlign: 'center', padding: '60px' }}>
                    <Server size={36} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                    <p style={{ color: 'var(--text-muted)' }}>No containers found. Is Docker running?</p>
                </div>
            ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                    {containers.map(c => (
                        <div key={c.ID} className="panel" style={{ padding: '0', overflow: 'hidden' }}>
                            {/* Card header */}
                            <div style={{ padding: '16px 20px', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '14px' }}>
                                    {/* Status indicator */}
                                    <div style={{
                                        width: '10px', height: '10px', borderRadius: '50%',
                                        background: stateColor(c.State),
                                        boxShadow: `0 0 8px ${stateColor(c.State)}`,
                                    }} />
                                    <div>
                                        <div style={{ fontWeight: 600, fontSize: '15px' }}>
                                            {c.Names?.replace(/^\//, '')}
                                        </div>
                                        <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '2px', display: 'flex', gap: '12px' }}>
                                            <span style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                                <HardDrive size={11} /> {c.Image}
                                            </span>
                                            <span style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                                                <Clock size={11} /> {c.RunningFor || c.Status}
                                            </span>
                                        </div>
                                    </div>
                                </div>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
                                    <span className={`badge ${c.State === 'running' ? 'active' : c.State === 'exited' ? 'quarantined' : 'internal'}`}
                                        style={{ fontSize: '11px' }}>
                                        {c.State?.toUpperCase()}
                                    </span>
                                    <button
                                        onClick={() => toggleLogs(c.ID, c.Names)}
                                        style={{
                                            display: 'flex', alignItems: 'center', gap: '6px',
                                            background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)',
                                            color: 'var(--text)', borderRadius: '6px', padding: '6px 12px',
                                            fontSize: '12px', cursor: 'pointer', fontWeight: 500,
                                            transition: 'all 0.2s',
                                        }}
                                    >
                                        {expandedLogs[c.ID] !== undefined ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                                        {expandedLogs[c.ID] !== undefined ? 'Hide Logs' : 'View Logs'}
                                    </button>
                                </div>
                            </div>

                            {/* Expanded logs panel */}
                            {expandedLogs[c.ID] !== undefined && (
                                <div style={{
                                    borderTop: '1px solid var(--border)',
                                    background: '#0a0e1a',
                                    padding: '16px 20px',
                                    maxHeight: '400px',
                                    overflowY: 'auto',
                                    position: 'relative',
                                }}>
                                    <div style={{
                                        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                                        marginBottom: '12px',
                                    }}>
                                        <span style={{ fontSize: '12px', color: 'var(--accent)', fontWeight: 600 }}>
                                            Container Logs (last 200 lines)
                                        </span>
                                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                                            <button onClick={() => copyLogs(c.ID)}
                                                style={{
                                                    display: 'flex', alignItems: 'center', gap: '5px',
                                                    background: copiedId === c.ID ? 'rgba(34,197,94,0.15)' : 'rgba(255,255,255,0.06)',
                                                    border: `1px solid ${copiedId === c.ID ? 'rgba(34,197,94,0.4)' : 'var(--border)'}`,
                                                    color: copiedId === c.ID ? '#22c55e' : 'var(--text-muted)',
                                                    borderRadius: '5px', padding: '4px 10px',
                                                    fontSize: '11px', cursor: 'pointer', fontWeight: 500,
                                                    transition: 'all 0.2s',
                                                }}>
                                                {copiedId === c.ID ? <Check size={12} /> : <Copy size={12} />}
                                                {copiedId === c.ID ? 'Copied!' : 'Copy Logs'}
                                            </button>
                                            <button onClick={() => toggleLogs(c.ID, c.Names)}
                                                style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
                                                <X size={14} />
                                            </button>
                                        </div>
                                    </div>
                                    {logsLoading[c.ID] ? (
                                        <p style={{ color: 'var(--text-muted)', fontSize: '12px' }}>Loading logs...</p>
                                    ) : (
                                        <pre style={{
                                            fontFamily: "'SF Mono', 'Fira Code', monospace",
                                            fontSize: '11px',
                                            lineHeight: '1.6',
                                            color: '#a0aec0',
                                            whiteSpace: 'pre-wrap',
                                            wordBreak: 'break-all',
                                            margin: 0,
                                        }}>
                                            {expandedLogs[c.ID]}
                                        </pre>
                                    )}
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
