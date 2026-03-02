import '../styles/globals.css';
import Link from 'next/link';
import { Activity, Building2, Users2, MessageSquare, CheckSquare, Briefcase, Wallet } from 'lucide-react';

export const metadata = {
    title: 'MultiClaw Dashboard',
    description: 'Manage your Agent Holding Company',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
    return (
        <html lang="en">
            <body style={{ display: 'flex', minHeight: '100vh', margin: 0 }}>
                <aside style={{ width: '250px', backgroundColor: 'var(--panel)', borderRight: '1px solid var(--border)', padding: '20px' }}>
                    <h2 style={{ color: 'var(--primary)', marginBottom: '30px' }}>MultiClaw</h2>
                    <nav style={{ display: 'flex', flexDirection: 'column', gap: '15px' }}>
                        <Link href="/" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><Activity size={18} /> Dashboard</Link>
                        <Link href="/org" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><Users2 size={18} /> Org Tree</Link>
                        <Link href="/companies" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><Building2 size={18} /> Companies</Link>
                        <Link href="/chats" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><MessageSquare size={18} /> Chats</Link>
                        <Link href="/approvals" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><CheckSquare size={18} /> Approvals</Link>
                        <Link href="/services" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><Briefcase size={18} /> Services UI</Link>
                        <Link href="/ledger" style={{ display: 'flex', alignItems: 'center', gap: '10px' }}><Wallet size={18} /> Ledger</Link>
                    </nav>
                </aside>
                <main style={{ flex: 1, padding: '40px', overflowY: 'auto' }}>
                    {children}
                </main>
            </body>
        </html>
    );
}
