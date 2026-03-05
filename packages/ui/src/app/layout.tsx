'use client';
import '../styles/globals.css';
import Link from 'next/link';
import { useEffect, useState } from 'react';
import { Activity, Building2, Users2, MessageSquare, CheckSquare, Briefcase, Wallet, Shield, Radio, ArrowUpCircle, Server, Settings } from 'lucide-react';

const navItems = [
    { href: '/', icon: Activity, label: 'Dashboard' },
    { href: '/org', icon: Users2, label: 'Org Tree' },
    { href: '/companies', icon: Building2, label: 'Companies' },
    { href: '/chats', icon: MessageSquare, label: 'My Chats' },
    { href: '/messaging', icon: Radio, label: 'Agent Comms' },
    { href: '/infrastructure', icon: Server, label: 'Infrastructure' },
    { href: '/approvals', icon: CheckSquare, label: 'Approvals' },
    { href: '/services', icon: Briefcase, label: 'Services' },
    { href: '/ledger', icon: Wallet, label: 'Ledger' },
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
        } catch { }
    };

    useEffect(() => {
        checkUpdate();
        const interval = setInterval(checkUpdate, 5 * 60 * 1000);
        // Poll localStorage for update info written by settings page (same-tab reactivity)
        const pollLocal = setInterval(() => {
            const stored = localStorage.getItem('_update_info');
            if (stored) {
                try {
                    const parsed = JSON.parse(stored);
                    setUpdateInfo(parsed);
                } catch {}
            }
        }, 2000);
        return () => { clearInterval(interval); clearInterval(pollLocal); };
    }, []);

    const handleUpdate = async () => {
        if (updating) return;
        setUpdating(true);
        setStatus('Pulling latest code...');
        try {
            const apiUrl = typeof window !== 'undefined' ? `${window.location.protocol}//${window.location.hostname}:8080/v1` : '';
            const token = typeof window !== 'undefined' ? localStorage.getItem('admin_token') : '';
            await fetch(`${apiUrl}/system/update`, {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' }
            });
            setStatus('Update started — containers rebuilding. Page will reload shortly...');
            setTimeout(() => window.location.reload(), 30000);
        } catch {
            setStatus('Update failed');
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

export default function RootLayout({ children }: { children: React.ReactNode }) {
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
                            MultiClaw v0.1.1
                        </div>
                    </div>
                </aside>
                <main style={{ flex: 1, padding: '32px 40px', overflowY: 'auto', maxHeight: '100vh' }}>
                    {children}
                </main>
            </body>
        </html>
    );
}
