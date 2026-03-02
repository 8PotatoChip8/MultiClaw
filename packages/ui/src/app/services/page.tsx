'use client';

export default function ServicesPage() {
    return (
        <div>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '30px' }}>
                <h1 style={{ margin: 0 }}>Internal Services Marketplace</h1>
                <button className="button">Publish Service</button>
            </div>

            <div className="panel" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <div>
                    <h3>Cybersecurity Audit</h3>
                    <p style={{ color: 'var(--text-muted)' }}>Provider: Omega CyberServices | Rate: $500/hr (Internal)</p>
                </div>
                <button className="button">Hire Service</button>
            </div>
        </div>
    );
}
