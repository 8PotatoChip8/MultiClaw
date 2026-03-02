'use client';

export default function LedgerPage() {
    return (
        <div>
            <h1 style={{ marginBottom: '30px' }}>Holding Company Ledger</h1>
            <p style={{ color: 'var(--text-muted)', marginBottom: '30px' }}>
                Immutable record of virtual transactions between internal companies and external expenditure.
            </p>

            <table style={{ width: '100%', borderCollapse: 'collapse', textAlign: 'left' }}>
                <thead>
                    <tr style={{ borderBottom: '1px solid var(--border)' }}>
                        <th style={{ padding: '10px' }}>Date</th>
                        <th style={{ padding: '10px' }}>From</th>
                        <th style={{ padding: '10px' }}>To</th>
                        <th style={{ padding: '10px' }}>Amount</th>
                        <th style={{ padding: '10px' }}>Memo</th>
                    </tr>
                </thead>
                <tbody>
                    <tr style={{ borderBottom: '1px solid var(--border)' }}>
                        <td style={{ padding: '10px' }}>2024-03-01 10:00</td>
                        <td style={{ padding: '10px' }}>Alpha Software</td>
                        <td style={{ padding: '10px' }}>Omega CyberServices</td>
                        <td style={{ padding: '10px', color: 'var(--danger)' }}>$500.00</td>
                        <td style={{ padding: '10px' }}>Cyber Audit (Virtual)</td>
                    </tr>
                </tbody>
            </table>
        </div>
    );
}
