'use client';
import { useEffect, useState, useMemo } from 'react';
import { api } from '../../../lib/api';
import { Agent } from '../../../lib/types';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { Shield, Play, Square, RefreshCw, AlertTriangle, Plus, Brain, Trash2, X, Terminal, Loader2, Monitor, Box } from 'lucide-react';
import { useAgentPresence } from '../../../lib/ws';
import AgentStatus from '../../../components/AgentStatus';

interface Memory {
    id: string;
    agent_id: string;
    category: string;
    key: string;
    content: string;
    importance: number;
    created_at: string;
    updated_at: string;
}

interface OpenClawFile {
    name: string;
    path: string;
    type: string;
    size: number;
    content: string | null;
}

type TabType = 'details' | 'memory' | 'terminal';

interface CmdEntry {
    command: string;
    exitCode: number;
    stdout: string;
    stderr: string;
    timestamp: Date;
}

interface VmInfoData {
    status: string;
    ip_address?: string;
    memory_usage_bytes?: number;
    memory_total_bytes?: number;
}

export default function AgentDetailPage() {
    const params = useParams();
    const id = params?.id as string;
    const [agent, setAgent] = useState<Agent | null>(null);
    const [showHire, setShowHire] = useState<'manager' | 'worker' | null>(null);
    const [hireName, setHireName] = useState('');
    const [hireSpecialty, setHireSpecialty] = useState('');
    const [tab, setTab] = useState<TabType>('details');
    const [memories, setMemories] = useState<Memory[]>([]);
    const [ocFiles, setOcFiles] = useState<OpenClawFile[]>([]);
    const [expandedFile, setExpandedFile] = useState<string | null>(null);
    const [showAddMemory, setShowAddMemory] = useState(false);
    const [newMem, setNewMem] = useState({ category: 'NOTE', key: '', content: '', importance: 5 });
    // Terminal state
    const [vmTarget, setVmTarget] = useState<'desktop' | 'sandbox'>('desktop');
    const [cmdHistory, setCmdHistory] = useState<Record<string, CmdEntry[]>>({ desktop: [], sandbox: [] });
    const [currentCmd, setCurrentCmd] = useState('');
    const [workingDir, setWorkingDir] = useState<Record<string, string>>({ desktop: '/home/agent', sandbox: '/home/agent' });
    const [isRunning, setIsRunning] = useState(false);
    const [vmInfoData, setVmInfoData] = useState<Record<string, VmInfoData | null>>({ desktop: null, sandbox: null });
    const [sandboxProvisioning, setSandboxProvisioning] = useState(false);
    const terminalEndRef = { current: null as HTMLDivElement | null };

    const agentList = useMemo(() => agent ? [agent] : [], [agent]);
    const presenceMap = useAgentPresence(agentList);

    const load = () => { api.getAgent(id).then(d => { if (d && !d.error) setAgent(d); }); };
    useEffect(() => { if (id) load(); }, [id]);

    const loadMemories = () => {
        api.getAgentMemories(id).then(d => setMemories(Array.isArray(d) ? d : []));
        api.getOpenClawFiles(id).then(d => setOcFiles(Array.isArray(d) ? d : []));
    };
    useEffect(() => { if (id && tab === 'memory') loadMemories(); }, [id, tab]);

    // Load VM info when terminal tab is active
    const loadVmInfo = () => {
        if (agent?.vm_id) api.vmInfo(id, 'desktop').then(d => { if (d && !d.error) setVmInfoData(prev => ({ ...prev, desktop: d })); });
        if (agent?.sandbox_vm_id) api.vmInfo(id, 'sandbox').then(d => { if (d && !d.error) setVmInfoData(prev => ({ ...prev, sandbox: d })); });
    };
    useEffect(() => {
        if (id && tab === 'terminal' && (agent?.vm_id || agent?.sandbox_vm_id)) {
            loadVmInfo();
            const interval = setInterval(loadVmInfo, 30000);
            return () => clearInterval(interval);
        }
    }, [id, tab, agent?.vm_id, agent?.sandbox_vm_id]);

    const executeCommand = async () => {
        if (!currentCmd.trim() || isRunning) return;
        setIsRunning(true);
        const cwd = workingDir[vmTarget];
        const result = await api.vmExec(id, {
            command: currentCmd,
            working_dir: cwd,
            timeout_secs: 60,
        }, vmTarget);
        setCmdHistory(prev => ({
            ...prev,
            [vmTarget]: [...(prev[vmTarget] || []), {
                command: currentCmd,
                exitCode: result.exit_code ?? -1,
                stdout: result.stdout || '',
                stderr: result.stderr || result.error || '',
                timestamp: new Date(),
            }],
        }));
        // Track cd commands to update working dir
        const trimmed = currentCmd.trim();
        if (trimmed.startsWith('cd ') && (result.exit_code === 0)) {
            const target = trimmed.slice(3).trim().replace(/^['"]|['"]$/g, '');
            if (target.startsWith('/')) setWorkingDir(prev => ({ ...prev, [vmTarget]: target }));
            else if (target === '~') setWorkingDir(prev => ({ ...prev, [vmTarget]: '/home/agent' }));
            else setWorkingDir(prev => ({ ...prev, [vmTarget]: `${cwd.replace(/\/$/, '')}/${target}` }));
        }
        setCurrentCmd('');
        setIsRunning(false);
        setTimeout(() => terminalEndRef.current?.scrollIntoView({ behavior: 'smooth' }), 50);
    };

    const handleProvisionSandbox = async () => {
        setSandboxProvisioning(true);
        await api.vmSandboxProvision(id);
        // Poll for completion
        const poll = setInterval(async () => {
            const a = await api.getAgent(id);
            if (a && a.sandbox_vm_id) {
                clearInterval(poll);
                setAgent(a);
                setSandboxProvisioning(false);
            }
        }, 3000);
        // Stop polling after 2 minutes
        setTimeout(() => { clearInterval(poll); setSandboxProvisioning(false); }, 120000);
    };

    const handleHire = async () => {
        if (!hireName) return;
        if (showHire === 'manager') await api.hireManager(id, { name: hireName, specialty: hireSpecialty || undefined });
        else await api.hireWorker(id, { name: hireName, specialty: hireSpecialty || undefined });
        setShowHire(null); setHireName(''); setHireSpecialty('');
        load();
    };

    const handlePanic = async () => { if (confirm('PANIC: Quarantine this agent?')) { await api.panic(id); load(); } };

    const handleAddMemory = async () => {
        if (!newMem.key || !newMem.content) return;
        await api.createAgentMemory(id, newMem);
        setShowAddMemory(false);
        setNewMem({ category: 'NOTE', key: '', content: '', importance: 5 });
        loadMemories();
    };

    const handleDeleteMemory = async (memId: string) => {
        await api.deleteAgentMemory(id, memId);
        loadMemories();
    };

    if (!agent) return <div className="animate-in"><p style={{ color: 'var(--text-muted)' }}>Loading agent...</p></div>;

    const catColors: Record<string, string> = { IDENTITY: 'var(--primary)', TASK: 'var(--accent)', CONTEXT: 'var(--success)', NOTE: 'var(--text-muted)' };
    const grouped = memories.reduce((acc, m) => { (acc[m.category] = acc[m.category] || []).push(m); return acc; }, {} as Record<string, Memory[]>);

    return (
        <div className="animate-in" style={{ maxWidth: '900px' }}>
            <div style={{ marginBottom: '24px' }}>
                <Link href="/org" style={{ fontSize: '13px', color: 'var(--text-muted)' }}>← Org Tree</Link>
            </div>

            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '24px' }}>
                <div>
                    <h1 style={{ fontSize: '28px', fontWeight: 700, marginBottom: '4px' }}>{agent.name}</h1>
                    <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                        <span className={`badge ${agent.role === 'CEO' ? 'external' : agent.role === 'MANAGER' ? 'internal' : 'active'}`}>{agent.role}</span>
                        <span className={`badge ${agent.status === 'ACTIVE' ? 'active' : 'quarantined'}`}>{agent.status}</span>
                        <AgentStatus presence={presenceMap[agent.id]?.presenceStatus ?? 'Active'} showLabel={true} size={9} />
                        {agent.handle && (
                            <span style={{ fontSize: '13px', color: 'var(--accent)', fontFamily: 'monospace' }}>{agent.handle}</span>
                        )}
                    </div>
                </div>
                {agent.status !== 'QUARANTINED' && (
                    <button className="button danger small" onClick={handlePanic} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                        <AlertTriangle size={14} /> Panic
                    </button>
                )}
            </div>

            {/* Tabs */}
            <div style={{ display: 'flex', gap: '0', marginBottom: '24px', borderBottom: '1px solid var(--border)' }}>
                {((['details', 'memory', ...((agent.vm_id || agent.sandbox_vm_id) ? ['terminal'] : [])] as TabType[])).map(t => (
                    <button key={t} onClick={() => setTab(t)} style={{
                        padding: '10px 20px', border: 'none', cursor: 'pointer',
                        background: tab === t ? 'var(--primary-glow)' : 'transparent',
                        color: tab === t ? 'var(--primary)' : 'var(--text-muted)',
                        borderBottom: tab === t ? '2px solid var(--primary)' : '2px solid transparent',
                        fontSize: '13px', fontWeight: 600, textTransform: 'uppercase',
                        letterSpacing: '0.05em', transition: 'all 0.2s',
                        display: 'flex', alignItems: 'center', gap: '6px',
                    }}>
                        {t === 'memory' && <Brain size={14} />}
                        {t === 'terminal' && <Terminal size={14} />}
                        {t}
                    </button>
                ))}
            </div>

            {tab === 'details' ? (
                <>
                    <div className="panel" style={{ marginBottom: '16px' }}>
                        <h3 style={{ marginBottom: '16px' }}>Details</h3>
                        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
                            {[
                                ['Model', agent.effective_model], ['Specialty', agent.specialty || '—'],
                                ['Desktop VM', agent.vm_id || 'None'], ['Sandbox VM', agent.sandbox_vm_id || 'None'],
                                ['Created', new Date(agent.created_at).toLocaleDateString()],
                            ].map(([label, value]) => (
                                <div key={label as string}>
                                    <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '4px' }}>{label}</div>
                                    <div style={{ fontSize: '14px', fontWeight: 500 }}>{value}</div>
                                </div>
                            ))}
                        </div>
                    </div>

                    {agent.vm_id && (
                        <div className="panel" style={{ marginBottom: '16px' }}>
                            <h3 style={{ marginBottom: '16px', display: 'flex', alignItems: 'center', gap: '8px' }}>
                                <Monitor size={16} /> Desktop VM
                            </h3>
                            <div style={{ display: 'flex', gap: '8px' }}>
                                <button className="button small secondary" onClick={() => api.vmStart(id, 'desktop')}><Play size={14} /> Start</button>
                                <button className="button small secondary" onClick={() => api.vmStop(id, 'desktop')}><Square size={14} /> Stop</button>
                            </div>
                        </div>
                    )}

                    <div className="panel" style={{ marginBottom: '16px' }}>
                        <h3 style={{ marginBottom: '16px', display: 'flex', alignItems: 'center', gap: '8px' }}>
                            <Box size={16} /> Sandbox VM
                        </h3>
                        {agent.sandbox_vm_id ? (
                            <div style={{ display: 'flex', gap: '8px' }}>
                                <button className="button small secondary" onClick={() => api.vmStart(id, 'sandbox')}><Play size={14} /> Start</button>
                                <button className="button small secondary" onClick={() => api.vmStop(id, 'sandbox')}><Square size={14} /> Stop</button>
                                <button className="button small secondary" onClick={() => { if (confirm('Wipe sandbox VM? This will destroy all data in the sandbox.')) api.vmRebuild(id, 'sandbox'); }}><RefreshCw size={14} /> Wipe</button>
                            </div>
                        ) : (
                            <div>
                                <p style={{ fontSize: '13px', color: 'var(--text-muted)', marginBottom: '12px' }}>
                                    No sandbox VM provisioned. A sandbox is a temporary environment that can be wiped to a clean state.
                                </p>
                                <button className="button small" onClick={handleProvisionSandbox} disabled={sandboxProvisioning}>
                                    {sandboxProvisioning ? <><Loader2 size={14} style={{ animation: 'spin 1s linear infinite' }} /> Provisioning...</> : <><Plus size={14} /> Provision Sandbox</>}
                                </button>
                            </div>
                        )}
                    </div>

                    {(agent.role === 'CEO' || agent.role === 'MANAGER') && (
                        <div className="panel">
                            <h3 style={{ marginBottom: '12px' }}>Hire Staff</h3>
                            <div style={{ display: 'flex', gap: '8px' }}>
                                {agent.role === 'CEO' && (
                                    <button className="button small" onClick={() => setShowHire('manager')}>
                                        <Plus size={14} /> Hire Manager
                                    </button>
                                )}
                                {agent.role === 'MANAGER' && (
                                    <button className="button small" onClick={() => setShowHire('worker')}>
                                        <Plus size={14} /> Hire Worker
                                    </button>
                                )}
                            </div>
                        </div>
                    )}
                </>
            ) : tab === 'terminal' ? (
                /* Terminal Tab */
                <div>
                    {/* VM Target Toggle */}
                    <div style={{ display: 'flex', gap: '0', marginBottom: '12px', background: 'rgba(0,0,0,0.2)', borderRadius: '8px', padding: '3px', width: 'fit-content' }}>
                        {(['desktop', 'sandbox'] as const).map(t => {
                            const hasVm = t === 'desktop' ? agent.vm_id : agent.sandbox_vm_id;
                            return (
                                <button key={t} onClick={() => setVmTarget(t)} style={{
                                    padding: '6px 16px', border: 'none', cursor: 'pointer',
                                    background: vmTarget === t ? 'var(--primary)' : 'transparent',
                                    color: vmTarget === t ? '#fff' : 'var(--text-muted)',
                                    borderRadius: '6px', fontSize: '12px', fontWeight: 600,
                                    display: 'flex', alignItems: 'center', gap: '6px',
                                    opacity: hasVm ? 1 : 0.5,
                                    transition: 'all 0.2s',
                                }}>
                                    {t === 'desktop' ? <Monitor size={12} /> : <Box size={12} />}
                                    {t === 'desktop' ? 'Desktop' : 'Sandbox'}
                                    {!hasVm && <span style={{ fontSize: '10px' }}>(none)</span>}
                                </button>
                            );
                        })}
                    </div>

                    {/* Sandbox not provisioned */}
                    {vmTarget === 'sandbox' && !agent.sandbox_vm_id ? (
                        <div className="panel" style={{ textAlign: 'center', padding: '40px' }}>
                            <Box size={36} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                            <p style={{ color: 'var(--text-muted)', marginBottom: '12px' }}>No sandbox VM provisioned</p>
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px', marginBottom: '20px' }}>
                                A sandbox is a temporary environment that can be wiped to a clean state — useful for testing and development.
                            </p>
                            <button className="button small" onClick={handleProvisionSandbox} disabled={sandboxProvisioning}>
                                {sandboxProvisioning ? <><Loader2 size={14} style={{ animation: 'spin 1s linear infinite' }} /> Provisioning...</> : <><Plus size={14} /> Provision Sandbox</>}
                            </button>
                        </div>
                    ) : vmTarget === 'desktop' && !agent.vm_id ? (
                        <div className="panel" style={{ textAlign: 'center', padding: '40px' }}>
                            <Monitor size={36} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                            <p style={{ color: 'var(--text-muted)' }}>No desktop VM assigned to this agent</p>
                        </div>
                    ) : (
                        <>
                            {/* VM Status Bar */}
                            <div className="panel" style={{ marginBottom: '12px', padding: '10px 16px', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
                                    <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                                        <div style={{
                                            width: '8px', height: '8px', borderRadius: '50%',
                                            background: vmInfoData[vmTarget]?.status === 'Running' ? '#22c55e' : '#ef4444',
                                        }} />
                                        <span style={{ fontSize: '12px', fontWeight: 600 }}>{vmInfoData[vmTarget]?.status || 'Unknown'}</span>
                                    </div>
                                    {vmInfoData[vmTarget]?.ip_address && (
                                        <span style={{ fontSize: '12px', color: 'var(--text-muted)', fontFamily: 'monospace' }}>
                                            {vmInfoData[vmTarget]!.ip_address}
                                        </span>
                                    )}
                                    {vmInfoData[vmTarget]?.memory_usage_bytes != null && vmInfoData[vmTarget]?.memory_total_bytes != null && (
                                        <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
                                            RAM: {(vmInfoData[vmTarget]!.memory_usage_bytes! / 1024 / 1024).toFixed(0)}MB / {(vmInfoData[vmTarget]!.memory_total_bytes! / 1024 / 1024).toFixed(0)}MB
                                        </span>
                                    )}
                                    <span style={{ fontSize: '10px', padding: '2px 8px', borderRadius: '10px',
                                        background: vmTarget === 'desktop' ? 'rgba(59,130,246,0.15)' : 'rgba(168,85,247,0.15)',
                                        color: vmTarget === 'desktop' ? '#3b82f6' : '#a855f7',
                                        fontWeight: 600, textTransform: 'uppercase',
                                    }}>{vmTarget}</span>
                                </div>
                                <div style={{ display: 'flex', gap: '6px' }}>
                                    <button className="button small secondary" onClick={() => api.vmStart(id, vmTarget)} style={{ fontSize: '11px', padding: '4px 8px' }}>
                                        <Play size={12} />
                                    </button>
                                    <button className="button small secondary" onClick={() => api.vmStop(id, vmTarget)} style={{ fontSize: '11px', padding: '4px 8px' }}>
                                        <Square size={12} />
                                    </button>
                                    {vmTarget === 'sandbox' && (
                                        <button className="button small secondary" onClick={() => { if (confirm('Wipe sandbox? All data will be lost.')) api.vmRebuild(id, 'sandbox'); }} style={{ fontSize: '11px', padding: '4px 8px' }}>
                                            <RefreshCw size={12} />
                                        </button>
                                    )}
                                </div>
                            </div>

                            {/* Terminal Output */}
                            <div style={{
                                background: '#0a0a0a', borderRadius: '10px', border: '1px solid var(--border)',
                                fontFamily: 'monospace', fontSize: '13px', lineHeight: '1.6',
                                minHeight: '400px', maxHeight: '600px', overflowY: 'auto',
                                display: 'flex', flexDirection: 'column',
                                borderTop: vmTarget === 'sandbox' ? '2px solid #a855f7' : '2px solid #3b82f6',
                            }}>
                                <div style={{ flex: 1, padding: '12px 16px' }}>
                                    {(cmdHistory[vmTarget] || []).length === 0 && (
                                        <div style={{ color: '#555', padding: '20px 0' }}>
                                            Connected to {agent.name}&apos;s {vmTarget}. Type a command below.
                                        </div>
                                    )}
                                    {(cmdHistory[vmTarget] || []).map((entry, i) => (
                                        <div key={i} style={{ marginBottom: '12px' }}>
                                            <div style={{ color: '#22c55e', display: 'flex', gap: '8px' }}>
                                                <span style={{ color: vmTarget === 'sandbox' ? '#a855f7' : '#3b82f6' }}>agent@{vmTarget}</span>
                                                <span style={{ color: '#6b7280' }}>:</span>
                                                <span style={{ color: '#8b5cf6' }}>{workingDir[vmTarget]}</span>
                                                <span style={{ color: '#6b7280' }}>$</span>
                                                <span style={{ color: '#e5e7eb' }}>{entry.command}</span>
                                            </div>
                                            {entry.stdout && (
                                                <pre style={{ margin: '2px 0 0 0', color: '#d1d5db', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                                                    {entry.stdout}
                                                </pre>
                                            )}
                                            {entry.stderr && (
                                                <pre style={{ margin: '2px 0 0 0', color: '#ef4444', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                                                    {entry.stderr}
                                                </pre>
                                            )}
                                            {entry.exitCode !== 0 && (
                                                <div style={{ color: '#ef4444', fontSize: '11px', marginTop: '2px' }}>
                                                    exit code: {entry.exitCode}
                                                </div>
                                            )}
                                        </div>
                                    ))}
                                    <div ref={el => { terminalEndRef.current = el; }} />
                                </div>

                                {/* Command Input */}
                                <div style={{
                                    borderTop: '1px solid #1f2937', padding: '8px 16px',
                                    display: 'flex', alignItems: 'center', gap: '8px',
                                }}>
                                    <span style={{ color: vmTarget === 'sandbox' ? '#a855f7' : '#3b82f6', fontSize: '12px', whiteSpace: 'nowrap' }}>
                                        {workingDir[vmTarget]} $
                                    </span>
                                    <input
                                        value={currentCmd}
                                        onChange={e => setCurrentCmd(e.target.value)}
                                        onKeyDown={e => { if (e.key === 'Enter') executeCommand(); }}
                                        placeholder="Type a command..."
                                        disabled={isRunning}
                                        style={{
                                            flex: 1, background: 'transparent', border: 'none', outline: 'none',
                                            color: '#e5e7eb', fontFamily: 'monospace', fontSize: '13px',
                                        }}
                                    />
                                    {isRunning ? (
                                        <Loader2 size={14} style={{ color: '#3b82f6', animation: 'spin 1s linear infinite' }} />
                                    ) : (
                                        <button onClick={executeCommand} disabled={!currentCmd.trim()}
                                            style={{
                                                background: 'none', border: 'none', color: '#3b82f6',
                                                cursor: currentCmd.trim() ? 'pointer' : 'default', padding: '2px',
                                                opacity: currentCmd.trim() ? 1 : 0.3,
                                            }}>
                                            <Play size={14} />
                                        </button>
                                    )}
                                </div>
                            </div>
                        </>
                    )}
                </div>
            ) : (
                /* Memory Tab */
                <div>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                        <h3 style={{ fontSize: '16px', fontWeight: 600 }}>
                            <Brain size={18} style={{ marginRight: '8px', color: 'var(--accent)' }} />
                            Agent Memory ({memories.length} items)
                        </h3>
                        <button className="button small" onClick={() => setShowAddMemory(true)}
                            style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                            <Plus size={14} /> Add Memory
                        </button>
                    </div>

                    {memories.length === 0 ? (
                        <div className="panel" style={{ textAlign: 'center', padding: '40px' }}>
                            <Brain size={36} style={{ color: 'var(--text-muted)', marginBottom: '12px' }} />
                            <p style={{ color: 'var(--text-muted)', marginBottom: '12px' }}>No memories yet</p>
                            <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>Memories are created automatically as the agent works, or you can add them manually.</p>
                        </div>
                    ) : (
                        Object.entries(grouped).map(([cat, mems]) => (
                            <div key={cat} className="panel" style={{ marginBottom: '12px' }}>
                                <h4 style={{
                                    fontSize: '12px', fontWeight: 700, textTransform: 'uppercase',
                                    letterSpacing: '0.05em', marginBottom: '12px',
                                    color: catColors[cat] || 'var(--text-muted)',
                                    display: 'flex', alignItems: 'center', gap: '8px',
                                }}>
                                    <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: catColors[cat] || 'var(--text-muted)' }} />
                                    {cat} ({mems.length})
                                </h4>
                                {mems.map(m => (
                                    <div key={m.id} style={{
                                        padding: '10px 12px', marginBottom: '6px',
                                        background: 'rgba(0,0,0,0.2)', borderRadius: '8px',
                                        display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start',
                                    }}>
                                        <div style={{ flex: 1 }}>
                                            <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
                                                <span style={{ fontSize: '13px', fontWeight: 600 }}>{m.key}</span>
                                                <span style={{
                                                    fontSize: '10px', padding: '1px 6px', borderRadius: '10px',
                                                    background: 'rgba(255,255,255,0.06)', color: 'var(--text-muted)',
                                                }}>
                                                    importance: {m.importance}
                                                </span>
                                            </div>
                                            <div style={{ fontSize: '13px', color: 'var(--text-muted)', lineHeight: '1.5' }}>{m.content}</div>
                                            <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '4px', opacity: 0.6 }}>
                                                Updated: {new Date(m.updated_at).toLocaleString()}
                                            </div>
                                        </div>
                                        <button onClick={() => handleDeleteMemory(m.id)}
                                            style={{ background: 'none', border: 'none', color: '#ef4444', cursor: 'pointer', padding: '4px', opacity: 0.6 }}
                                            title="Delete memory">
                                            <Trash2 size={14} />
                                        </button>
                                    </div>
                                ))}
                            </div>
                        ))
                    )}

                    {/* OpenClaw Runtime Files */}
                    {ocFiles.length > 0 && (
                        <div style={{ marginTop: '20px' }}>
                            <h3 style={{ fontSize: '14px', fontWeight: 600, marginBottom: '12px', display: 'flex', alignItems: 'center', gap: '8px' }}>
                                <Shield size={16} style={{ color: 'var(--primary)' }} />
                                OpenClaw Runtime Files ({ocFiles.length})
                            </h3>
                            {ocFiles.map(f => (
                                <div key={f.path} className="panel" style={{ marginBottom: '8px', padding: '0' }}>
                                    <div
                                        onClick={() => setExpandedFile(expandedFile === f.path ? null : f.path)}
                                        style={{
                                            padding: '10px 14px', cursor: 'pointer',
                                            display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                                        }}
                                    >
                                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                                            <span style={{
                                                fontSize: '10px', padding: '2px 8px', borderRadius: '10px',
                                                background: f.type === 'brain' ? '#a855f7' : f.type === 'session' ? 'var(--accent)' : f.type === 'state' ? 'var(--success)' : 'var(--primary)',
                                                color: '#fff', fontWeight: 600, textTransform: 'uppercase',
                                            }}>{f.type}</span>
                                            <span style={{ fontSize: '13px', fontWeight: 500, fontFamily: 'monospace' }}>{f.name}</span>
                                        </div>
                                        <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                                            {f.size < 1024 ? `${f.size} B` : `${(f.size / 1024).toFixed(1)} KB`}
                                        </span>
                                    </div>
                                    {expandedFile === f.path && f.content && (
                                        <div style={{
                                            padding: '12px 14px', borderTop: '1px solid var(--border)',
                                            background: 'rgba(0,0,0,0.3)', maxHeight: '400px', overflowY: 'auto',
                                        }}>
                                            <pre style={{
                                                fontSize: '11px', lineHeight: '1.5', fontFamily: 'monospace',
                                                whiteSpace: 'pre-wrap', wordBreak: 'break-word', margin: 0,
                                                color: 'var(--text-muted)',
                                            }}>{f.content}</pre>
                                        </div>
                                    )}
                                </div>
                            ))}
                        </div>
                    )}
                </div>
            )}

            {/* Hire Modal */}
            {showHire && (
                <div className="modal-overlay" onClick={() => setShowHire(null)}>
                    <div className="modal" onClick={e => e.stopPropagation()}>
                        <h2 style={{ fontSize: '20px', fontWeight: 700, marginBottom: '20px' }}>Hire {showHire === 'manager' ? 'Manager' : 'Worker'}</h2>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Name</label>
                                <input value={hireName} onChange={e => setHireName(e.target.value)} placeholder="Agent name" />
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Specialty</label>
                                <input value={hireSpecialty} onChange={e => setHireSpecialty(e.target.value)} placeholder="e.g. Sales, Engineering" />
                            </div>
                            <button className="button" onClick={handleHire} disabled={!hireName}>Hire</button>
                        </div>
                    </div>
                </div>
            )}

            {/* Add Memory Modal */}
            {showAddMemory && (
                <div className="modal-overlay" onClick={() => setShowAddMemory(false)}>
                    <div className="modal" onClick={e => e.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
                            <h2 style={{ fontSize: '20px', fontWeight: 700 }}>Add Memory</h2>
                            <button onClick={() => setShowAddMemory(false)} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
                                <X size={20} />
                            </button>
                        </div>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Category</label>
                                <select value={newMem.category} onChange={e => setNewMem({ ...newMem, category: e.target.value })}>
                                    <option value="IDENTITY">Identity — Who they are</option>
                                    <option value="TASK">Task — What they are doing</option>
                                    <option value="CONTEXT">Context — Where they left off</option>
                                    <option value="NOTE">Note — General knowledge</option>
                                </select>
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Key</label>
                                <input value={newMem.key} onChange={e => setNewMem({ ...newMem, key: e.target.value })} placeholder="Short label, e.g. 'current_project'" />
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Content</label>
                                <textarea value={newMem.content} onChange={e => setNewMem({ ...newMem, content: e.target.value })} rows={3}
                                    placeholder="What should the agent remember?" />
                            </div>
                            <div>
                                <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>Importance (1-10)</label>
                                <input type="number" min={1} max={10} value={newMem.importance}
                                    onChange={e => setNewMem({ ...newMem, importance: parseInt(e.target.value) || 5 })} />
                            </div>
                            <button className="button" onClick={handleAddMemory} disabled={!newMem.key || !newMem.content}
                                style={{ marginTop: '8px' }}>
                                Save Memory
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
