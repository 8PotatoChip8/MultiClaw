import '../styles/globals.css';
import Link from 'next/link';
import { Activity, Building2, Users2, MessageSquare, CheckSquare, Briefcase, Wallet, Shield } from 'lucide-react';

export const metadata = {
    title: 'MultiClaw Dashboard',
    description: 'Manage your Agent Holding Company',
};

const navItems = [
    { href: '/', icon: Activity, label: 'Dashboard' },
    { href: '/org', icon: Users2, label: 'Org Tree' },
    { href: '/companies', icon: Building2, label: 'Companies' },
    { href: '/chats', icon: MessageSquare, label: 'Chats' },
    { href: '/approvals', icon: CheckSquare, label: 'Approvals' },
    { href: '/services', icon: Briefcase, label: 'Services' },
    { href: '/ledger', icon: Wallet, label: 'Ledger' },
];

export default function RootLayout({ children }: { children: React.ReactNode }) {
    return (
        <html lang="en">
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
                    <div style={{ marginTop: 'auto', padding: '12px', borderTop: '1px solid var(--border)', fontSize: '11px', color: 'var(--text-muted)' }}>
                        MultiClaw v0.1.0
                    </div>
                </aside>
                <main style={{ flex: 1, padding: '32px 40px', overflowY: 'auto', maxHeight: '100vh' }}>
                    {children}
                </main>
            </body>
        </html>
    );
}
