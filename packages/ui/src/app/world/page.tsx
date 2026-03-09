'use client';
import dynamic from 'next/dynamic';

const WorldView = dynamic(() => import('../../components/world/WorldView'), {
    ssr: false,
    loading: () => (
        <div style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            height: '100%',
            minHeight: 'calc(100vh - 64px)',
            color: 'var(--text-muted)',
            fontSize: '16px',
        }}>
            <div style={{ textAlign: 'center' }}>
                <div style={{
                    width: '40px',
                    height: '40px',
                    border: '3px solid var(--border)',
                    borderTopColor: 'var(--primary)',
                    borderRadius: '50%',
                    animation: 'spin 1s linear infinite',
                    margin: '0 auto 16px',
                }} />
                <p>Loading 3D world...</p>
                <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
            </div>
        </div>
    ),
});

export default function WorldPage() {
    return <WorldView />;
}
