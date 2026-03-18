'use client';
import { useEffect, useState, useCallback } from 'react';
import { api } from '../../lib/api';
import { SharedVm, Company, Agent } from '../../lib/types';
import { useMultiClawEvents } from '../../lib/ws';
import { RefreshCw, HardDrive, Play, Square, RotateCcw, Trash2, Plus, X, Server } from 'lucide-react';

type FilterTab = 'all' | 'dept_test' | 'company_test' | 'company_prod';

export default function SharedVmsPage() {
    const [vms, setVms] = useState<SharedVm[]>([]);
    const [companies, setCompanies] = useState<Company[]>([]);
    const [agents, setAgents] = useState<Agent[]>([]);
    const [loading, setLoading] = useState(true);
    const [filter, setFilter] = useState<FilterTab>('all');
    const [autoRefresh, setAutoRefresh] = useState(true);
    const [showProvision, setShowProvision] = useState(false);
    const [actionLoading, setActionLoading] = useState<Record<string, boolean>>({});
    const [confirmDestroy, setConfirmDestroy] = useState<string | null>(null);

    // Provision form state
    const [provCompany, setProvCompany] = useState('');
    const [provPurpose, setProvPurpose] = useState<string>('dept_test');
    const [provManager, setProvManager] = useState('');
    const [provLabel, setProvLabel] = useState('');
    const [provVcpus, setProvVcpus] = useState(2);
    const [provMemory, setProvMemory] = useState(2048);
    const [provDisk, setProvDisk] = useState(20);
    const [provisioning, setProvisioning] = useState(false);

    const event = useMultiClawEvents();

    const fetchData = useCallback(async () => {
        try {
            const [vmData, companyData, agentData] = await Promise.all([
                api.getSharedVms(),
                api.getCompanies(),
                api.getAgents(),
            ]);
            setVms(Array.isArray(vmData) ? vmData : []);
            setCompanies(Array.isArray(companyData) ? companyData : []);
            setAgents(Array.isArray(agentData) ? agentData : []);
        } catch (e) {
            console.error('Failed to fetch shared VMs:', e);
        }
        setLoading(false);
    }, []);

    useEffect(() => {
        fetchData();
        if (!autoRefresh) return;
        const interval = setInterval(fetchData, 10000);
        return () => clearInterval(interval);
    }, [fetchData, autoRefresh]);

    // Refresh on relevant WebSocket events
    useEffect(() => {
        if (!event) return;
        try {
            const parsed = JSON.parse(event);
            if (parsed.type?.startsWith('shared_vm_')) {
                fetchData();
            }
        } catch {}
    }, [event, fetchData]);

    const filtered = filter === 'all' ? vms : vms.filter(v => v.vm_purpose === filter);

    const companyName = (id: string) => companies.find(c => c.id === id)?.name || id.slice(0, 8);
    const agentName = (id: string | null) => {
        if (!id) return '—';
        return agents.find(a => a.id === id)?.name || id.slice(0, 8);
    };

    const purposeLabel = (p: string) => {
        switch (p) {
            case 'dept_test': return 'Dept Test';
            case 'company_test': return 'Company Test';
            case 'company_prod': return 'Production';
            default: return p;
        }
    };

    const purposeBadgeClass = (p: string) => {
        switch (p) {
            case 'dept_test': return 'internal';
            case 'company_test': return 'pending';
            case 'company_prod': return 'active';
            default: return '';
        }
    };

    const stateColor = (state: string) => {
        switch (state?.toUpperCase()) {
            case 'RUNNING': return 'var(--success)';
            case 'STOPPED': return '#ef4444';
            case 'FAILED': return '#ef4444';
            default: return 'var(--text-muted)';
        }
    };

    const doAction = async (vmId: string, action: () => Promise<any>) => {
        setActionLoading(prev => ({ ...prev, [vmId]: true }));
        try {
            await action();
            await fetchData();
        } catch (e) {
            console.error('Action failed:', e);
        }
        setActionLoading(prev => ({ ...prev, [vmId]: false }));
    };

    const handleProvision = async () => {
        if (!provCompany || !provPurpose) return;
        if (provPurpose === 'dept_test' && !provManager) return;

        setProvisioning(true);

        // Find a CEO or manager to use as requester
        const companyAgents = agents.filter(a => a.company_id === provCompany);
        const requester = provPurpose === 'dept_test'
            ? provManager
            : companyAgents.find(a => a.role === 'CEO')?.id || companyAgents[0]?.id;

        if (!requester) {
            setProvisioning(false);
            return;
        }

        try {
            await api.provisionSharedVm({
                requester_agent_id: requester,
                company_id: provCompany,
                vm_purpose: provPurpose,
                department_manager_id: provPurpose === 'dept_test' ? provManager : undefined,
                label: provLabel || undefined,
                resources: { vcpus: provVcpus, memory_mb: provMemory, disk_gb: provDisk },
            });
            setShowProvision(false);
            setProvLabel('');
            setTimeout(fetchData, 2000);
        } catch (e) {
            console.error('Provision failed:', e);
        }
        setProvisioning(false);
    };

    const managers = agents.filter(a => a.role === 'MANAGER' && a.company_id === provCompany);

    return (
        <div className="animate-in">
            {/* Header */}
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '28px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>Shared VMs</h1>
                    <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>
                        Department test servers, company test servers, and production servers
                    </p>
                </div>
                <div style={{ display: 'flex', gap: '10px', alignItems: 'center' }}>
                    <label style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '13px', color: 'var(--text-muted)', cursor: 'pointer' }}>
                        <input type="checkbox" checked={autoRefresh} onChange={() => setAutoRefresh(!autoRefresh)}
                            style={{ accentColor: 'var(--primary)' }} />
                        Auto-refresh
                    </label>
                    <button className="button" onClick={fetchData}
                        style={{ display: 'flex', alignItems: 'center', gap: '6px', background: 'rgba(255,255,255,0.06)', border: '1px solid var(--border)' }}>
                        <RefreshCw size={14} /> Refresh
                    </button>
                    <button className="button" onClick={() => setShowProvision(true)}
                        style={{ display: 'flex', alignItems: 'center', gap: '6px', background: 'var(--primary)', border: 'none', color: '#fff' }}>
                        <Plus size={14} /> Provision VM
                    </button>
                </div>
            </div>

            {/* Summary */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: '16px', marginBottom: '24px' }}>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700 }}>{vms.length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Total Shared VMs</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700, color: 'var(--success)' }}>
                        {vms.filter(v => v.state?.toUpperCase() === 'RUNNING').length}
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Running</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700, color: '#ef4444' }}>
                        {vms.filter(v => v.state?.toUpperCase() === 'STOPPED').length}
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Stopped</div>
                </div>
                <div className="panel" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: '28px', fontWeight: 700, color: 'var(--accent)' }}>
                        {vms.filter(v => v.vm_purpose === 'company_prod').length}
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Production</div>
                </div>
            </div>

            {/* Filter tabs */}
            <div style={{ display: 'flex', gap: '8px', marginBottom: '20px' }}>
                {(['all', 'dept_test', 'company_test', 'company_prod'] as FilterTab[]).map(tab => (
                    <button key={tab} onClick={() => setFilter(tab)}
                        style={{
                            padding: '6px 16px', borderRadius: '6px', fontSize: '13px', fontWeight: 500,
                            border: filter === tab ? '1px solid var(--primary)' : '1px solid var(--border)',
                            background: filter === tab ? 'rgba(99,102,241,0.15)' : 'rgba(255,255,255,0.04)',
                            color: filter === tab ? 'var(--primary)' : 'var(--text-muted)',
                            cursor: 'pointer', transition: 'all 0.2s',
                        }}>
                        {tab === 'all' ? 'All' : purposeLabel(tab)}
                        <span style={{ marginLeft: '6px', opacity: 0.6 }}>
                            {tab === 'all' ? vms.length : vms.filter(v => v.vm_purpose === tab).length}
                        </span>
                    </button>
                ))}
            </div>

            {/* VM Cards */}
            {loading ? (
                <div className="panel" style={{ textAlign: 'center', padding: '60px' }}>
                    <RefreshCw size={24} style={{ color: 'var(--text-muted)', animation: 'spin 1s linear infinite' }} />
                    <p style={{ color: 'var(--text-muted)', marginTop: '12px' }}>Loading shared VMs...</p>
                </div>
            ) : filtered.length === 0 ? (
                <div className="panel" style={{ textAlign: 'center', padding: '60px' }}>
                    <Server size={36} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                    <p style={{ color: 'var(--text-muted)' }}>
                        {vms.length === 0 ? 'No shared VMs provisioned yet.' : 'No VMs match this filter.'}
                    </p>
                </div>
            ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                    {filtered.map(vm => (
                        <div key={vm.id} className="panel" style={{ padding: '16px 20px' }}>
                            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '14px' }}>
                                    {/* Status indicator */}
                                    <div style={{
                                        width: '10px', height: '10px', borderRadius: '50%',
                                        background: stateColor(vm.state),
                                        boxShadow: `0 0 8px ${stateColor(vm.state)}`,
                                    }} />
                                    <div>
                                        <div style={{ fontWeight: 600, fontSize: '15px', display: 'flex', alignItems: 'center', gap: '10px' }}>
                                            {vm.label || vm.hostname}
                                            <span className={`badge ${purposeBadgeClass(vm.vm_purpose)}`} style={{ fontSize: '10px' }}>
                                                {purposeLabel(vm.vm_purpose)}
                                            </span>
                                            <span className={`badge ${vm.state?.toUpperCase() === 'RUNNING' ? 'active' : 'quarantined'}`} style={{ fontSize: '10px' }}>
                                                {vm.state?.toUpperCase()}
                                            </span>
                                        </div>
                                        <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '4px', display: 'flex', gap: '16px', flexWrap: 'wrap' }}>
                                            <span><strong>Company:</strong> {companyName(vm.company_id)}</span>
                                            {vm.department_manager_id && (
                                                <span><strong>Dept Manager:</strong> {agentName(vm.department_manager_id)}</span>
                                            )}
                                            {vm.ip_address && <span><strong>IP:</strong> {vm.ip_address}</span>}
                                            <span><strong>VM:</strong> {vm.provider_ref}</span>
                                            {vm.resources && (
                                                <span>
                                                    <HardDrive size={11} style={{ verticalAlign: 'middle', marginRight: '3px' }} />
                                                    {vm.resources.vcpus}vCPU / {vm.resources.memory_mb}MB / {vm.resources.disk_gb}GB
                                                </span>
                                            )}
                                        </div>
                                    </div>
                                </div>
                                <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                                    {vm.state?.toUpperCase() === 'STOPPED' && (
                                        <button className="button" disabled={actionLoading[vm.id]}
                                            onClick={() => doAction(vm.id, () => api.sharedVmStart(vm.id))}
                                            style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', padding: '5px 12px', background: 'rgba(34,197,94,0.15)', border: '1px solid rgba(34,197,94,0.3)', color: '#22c55e' }}>
                                            <Play size={12} /> Start
                                        </button>
                                    )}
                                    {vm.state?.toUpperCase() === 'RUNNING' && (
                                        <button className="button" disabled={actionLoading[vm.id]}
                                            onClick={() => doAction(vm.id, () => api.sharedVmStop(vm.id))}
                                            style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', padding: '5px 12px', background: 'rgba(239,68,68,0.15)', border: '1px solid rgba(239,68,68,0.3)', color: '#ef4444' }}>
                                            <Square size={12} /> Stop
                                        </button>
                                    )}
                                    {vm.vm_purpose !== 'company_prod' && (
                                        <button className="button" disabled={actionLoading[vm.id]}
                                            onClick={() => doAction(vm.id, () => api.sharedVmRebuild(vm.id))}
                                            style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', padding: '5px 12px', background: 'rgba(255,255,255,0.06)', border: '1px solid var(--border)' }}>
                                            <RotateCcw size={12} /> Rebuild
                                        </button>
                                    )}
                                    {confirmDestroy === vm.id ? (
                                        <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
                                            <span style={{ fontSize: '11px', color: '#ef4444' }}>Confirm?</span>
                                            <button className="button"
                                                onClick={() => { doAction(vm.id, () => api.destroySharedVm(vm.id)); setConfirmDestroy(null); }}
                                                style={{ fontSize: '11px', padding: '4px 10px', background: '#ef4444', border: 'none', color: '#fff' }}>
                                                Yes
                                            </button>
                                            <button className="button"
                                                onClick={() => setConfirmDestroy(null)}
                                                style={{ fontSize: '11px', padding: '4px 10px', background: 'rgba(255,255,255,0.06)', border: '1px solid var(--border)' }}>
                                                No
                                            </button>
                                        </div>
                                    ) : (
                                        <button className="button"
                                            onClick={() => setConfirmDestroy(vm.id)}
                                            style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', padding: '5px 12px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', color: 'var(--text-muted)' }}>
                                            <Trash2 size={12} />
                                        </button>
                                    )}
                                </div>
                            </div>
                        </div>
                    ))}
                </div>
            )}

            {/* Provision Modal */}
            {showProvision && (
                <div style={{
                    position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.6)', display: 'flex',
                    alignItems: 'center', justifyContent: 'center', zIndex: 1000,
                }} onClick={() => setShowProvision(false)}>
                    <div className="panel" style={{ width: '480px', padding: '28px' }} onClick={e => e.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '20px' }}>
                            <h2 style={{ fontSize: '18px', fontWeight: 600 }}>Provision Shared VM</h2>
                            <button onClick={() => setShowProvision(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
                                <X size={18} />
                            </button>
                        </div>

                        <div style={{ display: 'flex', flexDirection: 'column', gap: '14px' }}>
                            {/* Company */}
                            <div>
                                <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>Company</label>
                                <select value={provCompany} onChange={e => { setProvCompany(e.target.value); setProvManager(''); }}
                                    style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }}>
                                    <option value="">Select company...</option>
                                    {companies.map(c => (
                                        <option key={c.id} value={c.id}>{c.name}</option>
                                    ))}
                                </select>
                            </div>

                            {/* Purpose */}
                            <div>
                                <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>Purpose</label>
                                <select value={provPurpose} onChange={e => setProvPurpose(e.target.value)}
                                    style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }}>
                                    <option value="dept_test">Department Test/Dev Server</option>
                                    <option value="company_test">Company Test Server</option>
                                    <option value="company_prod">Company Production Server</option>
                                </select>
                            </div>

                            {/* Department Manager (only for dept_test) */}
                            {provPurpose === 'dept_test' && (
                                <div>
                                    <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>Department (Manager)</label>
                                    <select value={provManager} onChange={e => setProvManager(e.target.value)}
                                        style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }}>
                                        <option value="">Select manager...</option>
                                        {managers.map(m => (
                                            <option key={m.id} value={m.id}>{m.name} — {m.specialty || 'General'}</option>
                                        ))}
                                    </select>
                                </div>
                            )}

                            {/* Label */}
                            <div>
                                <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>Label (optional)</label>
                                <input value={provLabel} onChange={e => setProvLabel(e.target.value)} placeholder="e.g. Acme Engineering Staging"
                                    style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }} />
                            </div>

                            {/* Resources */}
                            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px' }}>
                                <div>
                                    <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>vCPUs</label>
                                    <input type="number" value={provVcpus} onChange={e => setProvVcpus(parseInt(e.target.value) || 1)} min={1} max={16}
                                        style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }} />
                                </div>
                                <div>
                                    <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>RAM (MB)</label>
                                    <input type="number" value={provMemory} onChange={e => setProvMemory(parseInt(e.target.value) || 1024)} min={512} step={512}
                                        style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }} />
                                </div>
                                <div>
                                    <label style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px', display: 'block' }}>Disk (GB)</label>
                                    <input type="number" value={provDisk} onChange={e => setProvDisk(parseInt(e.target.value) || 10)} min={5}
                                        style={{ width: '100%', padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border)', background: 'var(--panel)', color: 'var(--text)', fontSize: '14px' }} />
                                </div>
                            </div>

                            <button className="button" onClick={handleProvision} disabled={provisioning || !provCompany || (provPurpose === 'dept_test' && !provManager)}
                                style={{
                                    marginTop: '8px', padding: '10px', background: 'var(--primary)', border: 'none', color: '#fff',
                                    fontSize: '14px', fontWeight: 600, borderRadius: '6px', cursor: 'pointer',
                                    opacity: provisioning || !provCompany ? 0.5 : 1,
                                }}>
                                {provisioning ? 'Provisioning...' : 'Provision VM'}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
