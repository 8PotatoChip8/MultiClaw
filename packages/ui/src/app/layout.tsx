'use client';
import '../styles/globals.css';
import Link from 'next/link';
import { useEffect, useState, useRef, useCallback } from 'react';
import { Activity, Building2, Users2, MessageSquare, CheckSquare, Briefcase, Wallet, Shield, Radio, ArrowUpCircle, Server, Key, Settings, Globe2, Calendar, Eye, Bell, X } from 'lucide-react';
import { useMultiClawEvents } from '../lib/ws';
import { api } from '../lib/api';

const navItems = [
    { href: '/', icon: Activity, label: 'Dashboard' },
    { href: '/world', icon: Globe2, label: 'World' },
    { href: '/org', icon: Users2, label: 'Org Tree' },
    { href: '/companies', icon: Building2, label: 'Companies' },
    { href: '/chats', icon: MessageSquare, label: 'My Chats' },
    { href: '/messaging', icon: Radio, label: 'Agent Comms' },
    { href: '/meetings', icon: Calendar, label: 'Meetings' },
    { href: '/pov', icon: Eye, label: 'Agent POV' },
    { href: '/infrastructure', icon: Server, label: 'Infrastructure' },
    { href: '/approvals', icon: CheckSquare, label: 'Approvals' },
    { href: '/services', icon: Briefcase, label: 'Services' },
    { href: '/ledger', icon: Wallet, label: 'Ledger' },
    { href: '/secrets', icon: Key, label: 'Secrets' },
    { href: '/settings', icon: Settings, label: 'Settings' },
];

function UpdateBanner() {
    const [updateInfo, setUpdateInfo] = useState<{ update_available: boolean; latest_version: string; current_version: string; channel?: string; release_url: string } | null>(null);
    const [updating, setUpdating] = useState(false);
    const [status, setStatus] = useState<string | null>(null);

    const checkUpdate = async () => {
        try {
            const apiUrl = typeof window !== 'undefined' ? `${window.location.protocol}//${window.location.hostname}:8080/v1` : '';
            const token = typeof window !== 'undefined' ? localStorage.getItem('admin_token') : '';
            const res = await fetch(`${apiUrl}/system/update-check`, {
                headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' }
            });
            const data = await res.json();
            setUpdateInfo(data);
            // Write to localStorage so settings page can read it
            localStorage.setItem('_update_info', JSON.stringify(data));
        } catch { }
    };

    useEffect(() => {
        checkUpdate();
        const interval = setInterval(checkUpdate, 5 * 60 * 1000);
        // Re-check immediately when settings page changes the channel
        const onChannelChange = () => checkUpdate();
        window.addEventListener('multiclaw-update-check', onChannelChange);
        return () => { clearInterval(interval); window.removeEventListener('multiclaw-update-check', onChannelChange); };
    }, []);

    const handleUpdate = async () => {
        if (updating) return;
        setUpdating(true);
        setStatus('Starting update...');
        try {
            const apiUrl = typeof window !== 'undefined' ? `${window.location.protocol}//${window.location.hostname}:8080/v1` : '';
            const token = typeof window !== 'undefined' ? localStorage.getItem('admin_token') : '';
            await fetch(`${apiUrl}/system/update`, {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' }
            });
            setStatus('Containers rebuilding — will reload when ready...');
            // Poll /v1/health until the new server is up (instead of blind 30s timeout)
            setTimeout(async () => {
                for (let i = 0; i < 120; i++) {
                    await new Promise(r => setTimeout(r, 2000));
                    try {
                        const resp = await fetch(`${apiUrl}/health`, { signal: AbortSignal.timeout(2000) });
                        if (resp.ok) {
                            setStatus('Update complete! Reloading...');
                            setTimeout(() => window.location.reload(), 1500);
                            return;
                        }
                    } catch { /* server not up yet */ }
                }
                setStatus('Server not responding after 4 minutes — update may have failed.');
                setUpdating(false);
            }, 5000);
        } catch {
            setStatus('Failed to start update');
            setUpdating(false);
        }
    };

    if (!updateInfo?.update_available) return null;

    // For dev/beta channels, versions already include channel prefix (e.g. "dev-18961e2")
    // so don't add "v" prefix. For stable, add "v".
    const isCommitBased = updateInfo.channel === 'dev' || updateInfo.channel === 'beta';
    const currentDisplay = isCommitBased ? updateInfo.current_version : `v${updateInfo.current_version}`;
    const latestDisplay = isCommitBased ? updateInfo.latest_version : `v${updateInfo.latest_version}`;

    return (
        <div style={{
            padding: '10px 12px', margin: '0 8px 8px', borderRadius: '8px',
            background: 'linear-gradient(135deg, rgba(0,200,100,0.15), rgba(0,150,255,0.1))',
            border: '1px solid rgba(0,200,100,0.3)',
            fontSize: '12px',
        }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '6px', marginBottom: '6px', color: 'var(--success)', fontWeight: 600 }}>
                <ArrowUpCircle size={14} />
                Update Available
            </div>
            <div style={{ color: 'var(--text-muted)', marginBottom: '8px' }}>
                {currentDisplay} → {latestDisplay}
            </div>
            {status ? (
                <div style={{ fontSize: '11px', color: 'var(--accent)' }}>{status}</div>
            ) : (
                <button
                    onClick={handleUpdate}
                    disabled={updating}
                    style={{
                        background: 'var(--success)', color: '#fff', border: 'none',
                        padding: '4px 12px', borderRadius: '6px', fontSize: '11px',
                        fontWeight: 600, cursor: 'pointer', width: '100%',
                    }}
                >
                    Update Now
                </button>
            )}
        </div>
    );
}

interface Toast {
    id: string;
    requestId: string;
    type: string;
    requesterName: string;
    description: string;
    createdAt: string;
}

function ApprovalToast() {
    const [toasts, setToasts] = useState<Toast[]>([]);
    const seenIds = useRef<Set<string>>(new Set());
    const agentCache = useRef<Record<string, string>>({});
    const event = useMultiClawEvents();

    const dismiss = useCallback((id: string) => {
        setToasts(prev => prev.filter(t => t.id !== id));
    }, []);

    useEffect(() => {
        if (!event) return;
        if (event.type !== 'new_request' && event.type !== 'approval_required') return;

        const requestId = event.request_id;
        if (!requestId || seenIds.current.has(requestId)) return;
        seenIds.current.add(requestId);

        // Fetch the pending request details
        api.getRequests('PENDING', 'USER').then(async (data) => {
            if (!Array.isArray(data)) return;
            const req = data.find((r: any) => r.id === requestId);
            if (!req) return;

            // Resolve requester name
            let requesterName = 'Agent';
            const agentId = req.created_by_agent_id || req.payload?.requester_id;
            if (agentId) {
                if (agentCache.current[agentId]) {
                    requesterName = agentCache.current[agentId];
                } else {
                    try {
                        const agents = await api.getAgents();
                        if (Array.isArray(agents)) {
                            agents.forEach((a: any) => { agentCache.current[a.id] = a.name; });
                            if (agentCache.current[agentId]) requesterName = agentCache.current[agentId];
                        }
                    } catch {}
                }
            }

            // Build description
            let description: string;
            if (req.type === 'REQUEST_TOOL') {
                description = `Tool: "${req.payload?.tool_name || 'unnamed'}" — ${req.payload?.description || 'No description'}`;
            } else {
                description = req.payload?.description || req.payload?.reason || req.type.replace(/_/g, ' ').toLowerCase();
            }

            const toast: Toast = {
                id: requestId,
                requestId,
                type: req.type,
                requesterName,
                description,
                createdAt: req.created_at,
            };

            setToasts(prev => [...prev, toast]);

            // Auto-dismiss after 30s
            setTimeout(() => {
                setToasts(prev => prev.filter(t => t.id !== requestId));
            }, 30000);
        }).catch(() => {});
    }, [event]);

    if (toasts.length === 0) return null;

    return (
        <div style={{
            position: 'fixed', bottom: '20px', right: '20px',
            zIndex: 2000, display: 'flex', flexDirection: 'column', gap: '10px',
            maxWidth: '400px',
        }}>
            {toasts.map(toast => (
                <div key={toast.id} style={{
                    background: 'var(--panel)',
                    backdropFilter: 'blur(20px)',
                    WebkitBackdropFilter: 'blur(20px)',
                    border: '1px solid rgba(245, 158, 11, 0.4)',
                    borderLeft: '4px solid var(--warning)',
                    borderRadius: 'var(--radius)',
                    padding: '16px',
                    animation: 'slideInRight 0.3s ease',
                    boxShadow: '0 8px 32px rgba(0, 0, 0, 0.4)',
                }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px', color: 'var(--warning)', fontWeight: 600, fontSize: '13px' }}>
                            <Bell size={16} />
                            Approval Needed
                        </div>
                        <button
                            onClick={() => dismiss(toast.id)}
                            style={{
                                background: 'none', border: 'none', color: 'var(--text-muted)',
                                cursor: 'pointer', padding: '2px', lineHeight: 1,
                            }}
                        >
                            <X size={14} />
                        </button>
                    </div>
                    <div style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text)', marginBottom: '4px' }}>
                        {toast.type.replace(/_/g, ' ')}
                    </div>
                    <div style={{
                        fontSize: '12px', color: 'var(--text-muted)', marginBottom: '8px',
                        overflow: 'hidden', textOverflow: 'ellipsis',
                        display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' as any,
                    }}>
                        {toast.description}
                    </div>
                    <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '10px' }}>
                        From: {toast.requesterName}
                    </div>
                    <Link href="/approvals" onClick={() => dismiss(toast.id)}>
                        <button style={{
                            background: 'var(--warning)', color: '#000', border: 'none',
                            padding: '5px 14px', borderRadius: '6px', fontSize: '11px',
                            fontWeight: 600, cursor: 'pointer', width: '100%',
                        }}>
                            Review
                        </button>
                    </Link>
                </div>
            ))}
        </div>
    );
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
    const [versionLabel, setVersionLabel] = useState('v0.1.1');

    useEffect(() => {
        const readVersion = () => {
            try {
                const cached = localStorage.getItem('_update_info');
                if (cached) {
                    const info = JSON.parse(cached);
                    if (info.current_version) setVersionLabel(info.current_version);
                }
            } catch {}
        };
        readVersion();
        // Re-read when UpdateBanner writes new data
        const onStorage = () => readVersion();
        window.addEventListener('storage', onStorage);
        // Also poll occasionally since storage event doesn't fire for same-tab writes
        const interval = setInterval(readVersion, 30000);
        return () => { window.removeEventListener('storage', onStorage); clearInterval(interval); };
    }, []);

    return (
        <html lang="en">
            <head>
                <title>MultiClaw Dashboard</title>
                <meta name="description" content="Manage your Agent Holding Company" />
            </head>
            <body style={{ display: 'flex', minHeight: '100vh' }}>
                <aside style={{
                    width: '240px', minWidth: '240px',
                    background: 'rgba(10, 14, 26, 0.95)',
                    borderRight: '1px solid var(--border)',
                    padding: '24px 16px',
                    display: 'flex', flexDirection: 'column',
                }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '40px', paddingLeft: '8px' }}>
                        <Shield size={24} style={{ color: 'var(--primary)' }} />
                        <h2 style={{
                            background: 'linear-gradient(135deg, var(--primary), var(--accent))',
                            WebkitBackgroundClip: 'text',
                            WebkitTextFillColor: 'transparent',
                            fontSize: '20px', fontWeight: 700, letterSpacing: '-0.02em',
                        }}>MultiClaw</h2>
                    </div>
                    <nav style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                        {navItems.map(item => (
                            <Link key={item.href} href={item.href} style={{
                                display: 'flex', alignItems: 'center', gap: '12px',
                                padding: '10px 12px', borderRadius: '8px',
                                color: 'var(--text-muted)', fontSize: '14px', fontWeight: 500,
                                transition: 'all 0.2s',
                            }}>
                                <item.icon size={18} />
                                {item.label}
                            </Link>
                        ))}
                    </nav>
                    <div style={{ marginTop: 'auto' }}>
                        <UpdateBanner />
                        <div style={{ padding: '12px', borderTop: '1px solid var(--border)', fontSize: '11px', color: 'var(--text-muted)' }}>
                            MultiClaw {versionLabel}
                        </div>
                    </div>
                </aside>
                <main style={{ flex: 1, padding: '32px 40px', overflowY: 'auto', maxHeight: '100vh' }}>
                    {children}
                </main>
                <ApprovalToast />
            </body>
        </html>
    );
}
