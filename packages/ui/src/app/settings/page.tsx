'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Settings as SettingsIcon, ArrowUpCircle, Check, Info, Shield, GitBranch, Zap, Heart, Cpu, X, Star, Plus, Sparkles, Trash2, AlertTriangle } from 'lucide-react';

type Channel = 'stable' | 'beta' | 'dev';

const channelInfo: Record<Channel, { label: string; desc: string; icon: any; color: string }> = {
    stable: { label: 'Stable', desc: 'Fully tested production releases. Updated only when a new version is formally published.', icon: Shield, color: 'var(--success)' },
    beta: { label: 'Beta', desc: 'Experimental features from the beta branch. May contain bugs or incomplete features.', icon: GitBranch, color: '#f59e0b' },
    dev: { label: 'Dev', desc: 'Latest commits from main. Bleeding-edge — may break at any time.', icon: Zap, color: '#ef4444' },
};

export default function SettingsPage() {
    const [settings, setSettings] = useState<Record<string, string>>({});
    const [selectedChannel, setSelectedChannel] = useState<Channel | null>(null);
    const [saving, setSaving] = useState(false);
    const [saved, setSaved] = useState(false);
    const [updateInfo, setUpdateInfo] = useState<any>(null);
    const [checking, setChecking] = useState(false);
    const [heartbeatSecs, setHeartbeatSecs] = useState('600');
    const [heartbeatSaving, setHeartbeatSaving] = useState(false);
    const [heartbeatSaved, setHeartbeatSaved] = useState(false);
    const [models, setModels] = useState<string[]>([]);
    const [defaultModel, setDefaultModel] = useState('glm-5:cloud');
    const [newModelName, setNewModelName] = useState('');
    const [modelsSaving, setModelsSaving] = useState(false);
    const [modelsSaved, setModelsSaved] = useState(false);
    const [pullStatus, setPullStatus] = useState<Record<string, { status: string; error?: string }>>({});
    const [rewriteModel, setRewriteModel] = useState('glm-5:cloud');
    const [rewriteSaving, setRewriteSaving] = useState(false);
    const [rewriteSaved, setRewriteSaved] = useState(false);

    // Reset / Wipe state
    const [holdingConfig, setHoldingConfig] = useState<{
        initialized: boolean;
        holding_name?: string;
        main_agent_name?: string;
        default_model?: string;
    }>({ initialized: false });
    const [resetHoldingName, setResetHoldingName] = useState('');
    const [resetAgentName, setResetAgentName] = useState('');
    const [resetModel, setResetModel] = useState('');
    const [showResetModal, setShowResetModal] = useState(false);
    const [resetConfirmText, setResetConfirmText] = useState('');
    const [resetting, setResetting] = useState(false);
    const [resetError, setResetError] = useState('');

    useEffect(() => {
        api.getSettings().then(data => {
            if (data && typeof data === 'object') {
                setSettings(data);
                setSelectedChannel((data.update_channel as Channel) || 'stable');
                if (data.heartbeat_interval_secs) setHeartbeatSecs(data.heartbeat_interval_secs);
                if (data.rewrite_model) setRewriteModel(data.rewrite_model);
            }
        }).catch(() => setSelectedChannel('stable'));
        // Auto-fetch update status so it's visible immediately
        api.checkForUpdate().then(info => {
            setUpdateInfo(info);
            if (info) localStorage.setItem('_update_info', JSON.stringify(info));
        }).catch(() => {});
        api.getModels().then(data => {
            if (data?.models) setModels(data.models);
            if (data?.default) setDefaultModel(data.default);
        });
        api.getHoldingConfig().then(data => {
            if (data && typeof data === 'object') {
                setHoldingConfig(data);
                if (data.holding_name) setResetHoldingName(data.holding_name);
                if (data.main_agent_name) setResetAgentName(data.main_agent_name);
                if (data.default_model) setResetModel(data.default_model);
            }
        });
    }, []);

    // Poll model pull status
    useEffect(() => {
        const fetchStatus = () => {
            api.getModelPullStatus().then(data => {
                if (data && typeof data === 'object') setPullStatus(data);
            });
        };
        fetchStatus();
        const interval = setInterval(fetchStatus, 3000);
        return () => clearInterval(interval);
    }, []);

    const handleSaveChannel = async (channel: Channel) => {
        setSelectedChannel(channel);
        setSaving(true);
        await api.updateSettings({ update_channel: channel });
        setSaving(false);
        setSaved(true);
        setTimeout(() => setSaved(false), 2000);
        // Re-check for updates with new channel
        setChecking(true);
        const info = await api.checkForUpdate();
        setUpdateInfo(info);
        if (info) localStorage.setItem('_update_info', JSON.stringify(info));
        setChecking(false);
        // Notify the sidebar UpdateBanner to re-check immediately
        window.dispatchEvent(new Event('multiclaw-update-check'));
    };

    const handleCheckUpdate = async () => {
        setChecking(true);
        const info = await api.checkForUpdate();
        setUpdateInfo(info);
        if (info) localStorage.setItem('_update_info', JSON.stringify(info));
        setChecking(false);
        window.dispatchEvent(new Event('multiclaw-update-check'));
    };

    const handleSaveHeartbeat = async () => {
        const val = parseInt(heartbeatSecs, 10);
        if (isNaN(val) || val < 0) return;
        setHeartbeatSaving(true);
        await api.updateSettings({ heartbeat_interval_secs: String(val) });
        setHeartbeatSaving(false);
        setHeartbeatSaved(true);
        setTimeout(() => setHeartbeatSaved(false), 2000);
    };

    const heartbeatMinutes = (() => {
        const secs = parseInt(heartbeatSecs, 10);
        if (isNaN(secs) || secs === 0) return null;
        return Math.round(secs / 60);
    })();

    return (
        <div className="animate-in">
            <div style={{ marginBottom: '32px' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '4px' }}>
                    <SettingsIcon size={24} style={{ color: 'var(--primary)' }} />
                    <h1 style={{ fontSize: '28px', fontWeight: 700 }}>Settings</h1>
                </div>
                <p style={{ color: 'var(--text-muted)', fontSize: '14px' }}>Configure your MultiClaw installation</p>
            </div>

            {/* Update Channel */}
            <div className="panel" style={{ maxWidth: '700px', marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '20px' }}>
                    <div>
                        <h2 style={{ fontSize: '18px', fontWeight: 600, marginBottom: '4px' }}>Update Channel</h2>
                        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Choose how aggressively you want to receive updates</p>
                    </div>
                    {saved && (
                        <span style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', color: 'var(--success)', fontWeight: 600 }}>
                            <Check size={14} /> Saved
                        </span>
                    )}
                </div>

                <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                    {!selectedChannel ? (
                        <div style={{ padding: '16px 20px', color: 'var(--text-muted)', fontSize: '13px' }}>Loading channel settings...</div>
                    ) : (Object.keys(channelInfo) as Channel[]).map(ch => {
                        const info = channelInfo[ch];
                        const isSelected = selectedChannel === ch;
                        const Icon = info.icon;
                        return (
                            <div key={ch} onClick={() => handleSaveChannel(ch)} style={{
                                padding: '16px 20px',
                                borderRadius: '10px',
                                border: isSelected ? `2px solid ${info.color}` : '2px solid var(--border)',
                                background: isSelected ? `${info.color}10` : 'transparent',
                                cursor: 'pointer',
                                transition: 'all 0.2s',
                                display: 'flex', gap: '14px', alignItems: 'flex-start',
                            }}>
                                <div style={{
                                    width: '36px', height: '36px', borderRadius: '8px',
                                    background: `${info.color}20`, display: 'flex',
                                    alignItems: 'center', justifyContent: 'center', flexShrink: 0,
                                }}>
                                    <Icon size={18} style={{ color: info.color }} />
                                </div>
                                <div style={{ flex: 1 }}>
                                    <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '2px' }}>
                                        <span style={{ fontWeight: 600, fontSize: '14px' }}>{info.label}</span>
                                        {isSelected && (
                                            <span style={{
                                                fontSize: '10px', padding: '1px 8px', borderRadius: '10px',
                                                background: info.color, color: '#fff', fontWeight: 600,
                                            }}>ACTIVE</span>
                                        )}
                                    </div>
                                    <p style={{ fontSize: '12px', color: 'var(--text-muted)', lineHeight: '1.5' }}>{info.desc}</p>
                                </div>
                                <div style={{
                                    width: '18px', height: '18px', borderRadius: '50%',
                                    border: `2px solid ${isSelected ? info.color : 'var(--border)'}`,
                                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                                    flexShrink: 0, marginTop: '2px',
                                }}>
                                    {isSelected && <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: info.color }} />}
                                </div>
                            </div>
                        );
                    })}
                </div>
            </div>

            {/* Update Status */}
            <div className="panel" style={{ maxWidth: '700px', marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '12px' }}>
                    <h2 style={{ fontSize: '18px', fontWeight: 600 }}>Update Status</h2>
                    <button className="button small" onClick={handleCheckUpdate} disabled={checking}
                        style={{ fontSize: '12px', display: 'flex', alignItems: 'center', gap: '4px' }}>
                        <ArrowUpCircle size={14} />
                        {checking ? 'Checking...' : 'Check Now'}
                    </button>
                </div>

                {updateInfo ? (
                    <div style={{ fontSize: '13px' }}>
                        {(() => {
                            const isCommitBased = updateInfo.channel === 'dev' || updateInfo.channel === 'beta';
                            const currentDisplay = isCommitBased ? updateInfo.current_version : `v${updateInfo.current_version}`;
                            const latestDisplay = isCommitBased ? updateInfo.latest_version : `v${updateInfo.latest_version}`;
                            return (
                                <div style={{ display: 'grid', gridTemplateColumns: '140px 1fr', gap: '8px', marginBottom: '12px' }}>
                                    <span style={{ color: 'var(--text-muted)' }}>Current version:</span>
                                    <span style={{ fontFamily: 'monospace' }}>{currentDisplay}</span>
                                    <span style={{ color: 'var(--text-muted)' }}>Channel:</span>
                                    <span style={{ fontWeight: 600, color: channelInfo[updateInfo.channel as Channel]?.color || 'var(--text)' }}>
                                        {channelInfo[updateInfo.channel as Channel]?.label || updateInfo.channel}
                                    </span>
                                    <span style={{ color: 'var(--text-muted)' }}>Latest available:</span>
                                    <span style={{ fontFamily: 'monospace' }}>{latestDisplay}</span>
                                    {updateInfo.semver && isCommitBased && (<>
                                        <span style={{ color: 'var(--text-muted)' }}>Release version:</span>
                                        <span style={{ fontFamily: 'monospace' }}>v{updateInfo.semver}</span>
                                    </>)}
                                    {updateInfo.commit_message && (<>
                                        <span style={{ color: 'var(--text-muted)' }}>Latest commit:</span>
                                        <span style={{ fontFamily: 'monospace', fontSize: '12px' }}>{updateInfo.commit_message}</span>
                                    </>)}
                                </div>
                            );
                        })()}
                        {updateInfo.update_available ? (
                            <div style={{
                                padding: '10px 14px', borderRadius: '8px',
                                background: 'linear-gradient(135deg, rgba(0,200,100,0.1), rgba(0,150,255,0.05))',
                                border: '1px solid rgba(0,200,100,0.3)',
                                display: 'flex', alignItems: 'center', gap: '8px',
                            }}>
                                <ArrowUpCircle size={16} style={{ color: 'var(--success)' }} />
                                <span style={{ fontWeight: 600, color: 'var(--success)' }}>Update available!</span>
                                <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
                                    Use the update banner in the sidebar to install
                                </span>
                            </div>
                        ) : (
                            <div style={{
                                padding: '10px 14px', borderRadius: '8px',
                                background: 'rgba(255,255,255,0.03)',
                                display: 'flex', alignItems: 'center', gap: '8px',
                            }}>
                                <Check size={16} style={{ color: 'var(--success)' }} />
                                <span style={{ color: 'var(--text-muted)' }}>You're up to date</span>
                            </div>
                        )}
                    </div>
                ) : (
                    <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>
                        <Info size={14} style={{ verticalAlign: 'middle', marginRight: '6px' }} />
                        Click "Check Now" to see your update status
                    </p>
                )}
            </div>

            {/* MainAgent Heartbeat */}
            <div className="panel" style={{ maxWidth: '700px', marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '12px' }}>
                    <div>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
                            <Heart size={18} style={{ color: 'var(--primary)' }} />
                            <h2 style={{ fontSize: '18px', fontWeight: 600 }}>MainAgent Heartbeat</h2>
                        </div>
                        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>
                            How often KonnerBot checks on pending approvals, company status, and issues
                        </p>
                    </div>
                    {heartbeatSaved && (
                        <span style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', color: 'var(--success)', fontWeight: 600 }}>
                            <Check size={14} /> Saved
                        </span>
                    )}
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '12px' }}>
                    <input
                        type="number"
                        min="0"
                        step="60"
                        value={heartbeatSecs}
                        onChange={e => setHeartbeatSecs(e.target.value)}
                        style={{
                            width: '120px', padding: '10px 12px', borderRadius: '8px',
                            background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                            color: 'var(--text)', fontSize: '14px', fontFamily: 'monospace',
                            textAlign: 'center',
                        }}
                    />
                    <span style={{ color: 'var(--text-muted)', fontSize: '13px' }}>
                        seconds
                        {heartbeatMinutes !== null && ` (${heartbeatMinutes} min)`}
                    </span>
                    <button className="button small" onClick={handleSaveHeartbeat} disabled={heartbeatSaving}
                        style={{ fontSize: '12px', marginLeft: 'auto' }}>
                        {heartbeatSaving ? 'Saving...' : 'Save'}
                    </button>
                </div>

                <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
                    {[
                        { label: '5 min', value: '300' },
                        { label: '10 min', value: '600' },
                        { label: '15 min', value: '900' },
                        { label: '30 min', value: '1800' },
                        { label: 'Disabled', value: '0' },
                    ].map(preset => (
                        <button key={preset.value} onClick={() => { setHeartbeatSecs(preset.value); }}
                            style={{
                                padding: '4px 12px', borderRadius: '6px', fontSize: '12px',
                                fontWeight: 500, cursor: 'pointer',
                                border: heartbeatSecs === preset.value ? '1px solid var(--primary)' : '1px solid var(--border)',
                                background: heartbeatSecs === preset.value ? 'rgba(59,130,246,0.15)' : 'transparent',
                                color: heartbeatSecs === preset.value ? 'var(--primary)' : 'var(--text-muted)',
                                transition: 'all 0.2s',
                            }}>
                            {preset.label}
                        </button>
                    ))}
                </div>

                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '12px' }}>
                    Set to 0 to disable. When nothing needs attention, each heartbeat costs very little (a short prompt + a silent OK response).
                    Takes effect on the next cycle — no restart required.
                </p>
            </div>

            {/* Message Rewrite Model */}
            <div className="panel" style={{ maxWidth: '700px', marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '12px' }}>
                    <div>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
                            <Sparkles size={18} style={{ color: 'var(--primary)' }} />
                            <h2 style={{ fontSize: '18px', fontWeight: 600 }}>Message Rewrite Model</h2>
                        </div>
                        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>
                            AI model used to rewrite and improve your messages before sending
                        </p>
                    </div>
                    {rewriteSaved && (
                        <span style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', color: 'var(--success)', fontWeight: 600 }}>
                            <Check size={14} /> Saved
                        </span>
                    )}
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
                    <select
                        value={rewriteModel}
                        onChange={e => setRewriteModel(e.target.value)}
                        style={{
                            flex: 1, maxWidth: '300px', padding: '10px 12px', borderRadius: '8px',
                            background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                            color: 'var(--text)', fontSize: '13px', fontFamily: 'monospace',
                        }}
                    >
                        {models.map(m => (
                            <option key={m} value={m}>{m}</option>
                        ))}
                    </select>
                    <button className="button small" onClick={async () => {
                        setRewriteSaving(true);
                        await api.updateSettings({ rewrite_model: rewriteModel });
                        setRewriteSaving(false);
                        setRewriteSaved(true);
                        setTimeout(() => setRewriteSaved(false), 2000);
                    }} disabled={rewriteSaving} style={{ fontSize: '12px' }}>
                        {rewriteSaving ? 'Saving...' : 'Save'}
                    </button>
                </div>

                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '12px' }}>
                    Used by the rewrite button in the chat input. Click the sparkle icon next to the message input to rewrite a draft message.
                </p>
            </div>

            {/* Available Models */}
            <div className="panel" style={{ maxWidth: '700px', marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '12px' }}>
                    <div>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
                            <Cpu size={18} style={{ color: 'var(--primary)' }} />
                            <h2 style={{ fontSize: '18px', fontWeight: 600 }}>Available Models</h2>
                        </div>
                        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>
                            Models available for agent selection. CEOs choose manager models; managers choose worker models.
                        </p>
                    </div>
                    {modelsSaved && (
                        <span style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px', color: 'var(--success)', fontWeight: 600 }}>
                            <Check size={14} /> Saved
                        </span>
                    )}
                </div>

                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', marginBottom: '16px' }}>
                    {models.map(m => {
                        const ps = pullStatus[m];
                        const isPulling = ps?.status === 'pulling';
                        const isFailed = ps?.status === 'failed';
                        const isReady = ps?.status === 'ready';
                        return (
                        <div key={m} style={{
                            display: 'flex', alignItems: 'center', gap: '6px',
                            padding: '6px 12px', borderRadius: '8px',
                            background: m === defaultModel ? 'rgba(59,130,246,0.15)' : 'rgba(255,255,255,0.05)',
                            border: m === defaultModel ? '1px solid var(--primary)' : '1px solid var(--border)',
                            fontSize: '13px', fontFamily: 'monospace',
                        }}>
                            <button onClick={() => {
                                setDefaultModel(m);
                            }} title="Set as default" style={{
                                background: 'none', border: 'none', cursor: 'pointer', padding: 0,
                                color: m === defaultModel ? '#f59e0b' : 'var(--text-muted)',
                                display: 'flex', alignItems: 'center',
                            }}>
                                <Star size={14} fill={m === defaultModel ? '#f59e0b' : 'none'} />
                            </button>
                            <span>{m}</span>
                            {isPulling && (
                                <span title="Pulling model..." style={{
                                    fontSize: '10px', padding: '1px 6px', borderRadius: '10px',
                                    background: 'rgba(59,130,246,0.2)', color: 'var(--primary)',
                                    fontWeight: 600, fontFamily: 'sans-serif',
                                    animation: 'pulse 1.5s infinite',
                                }}>PULLING</span>
                            )}
                            {isFailed && (
                                <span title={ps?.error || 'Pull failed — click to retry'} onClick={() => api.pullModel(m)}
                                    style={{
                                        fontSize: '10px', padding: '1px 6px', borderRadius: '10px',
                                        background: 'rgba(239,68,68,0.2)', color: '#ef4444',
                                        fontWeight: 600, fontFamily: 'sans-serif', cursor: 'pointer',
                                    }}>RETRY</span>
                            )}
                            {isReady && (
                                <Check size={12} style={{ color: 'var(--success)' }} />
                            )}
                            <button onClick={() => {
                                const updated = models.filter(x => x !== m);
                                setModels(updated);
                                if (defaultModel === m && updated.length > 0) setDefaultModel(updated[0]);
                            }} style={{
                                background: 'none', border: 'none', cursor: 'pointer', padding: 0,
                                color: 'var(--text-muted)', display: 'flex', alignItems: 'center',
                            }}>
                                <X size={14} />
                            </button>
                        </div>
                        );
                    })}
                </div>

                <div style={{ display: 'flex', gap: '8px', marginBottom: '12px' }}>
                    <input
                        value={newModelName}
                        onChange={e => setNewModelName(e.target.value)}
                        onKeyDown={e => {
                            if (e.key === 'Enter' && newModelName.trim() && !models.includes(newModelName.trim())) {
                                setModels([...models, newModelName.trim()]);
                                setNewModelName('');
                            }
                        }}
                        placeholder="Add model name (e.g. llama-4:cloud)"
                        style={{
                            flex: 1, padding: '8px 12px', borderRadius: '8px',
                            background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                            color: 'var(--text)', fontSize: '13px', fontFamily: 'monospace',
                        }}
                    />
                    <button className="button small" onClick={() => {
                        if (newModelName.trim() && !models.includes(newModelName.trim())) {
                            setModels([...models, newModelName.trim()]);
                            setNewModelName('');
                        }
                    }} style={{ fontSize: '12px', display: 'flex', alignItems: 'center', gap: '4px' }}>
                        <Plus size={14} /> Add
                    </button>
                </div>

                <button className="button small" onClick={async () => {
                    setModelsSaving(true);
                    await api.updateSettings({
                        available_models: JSON.stringify(models),
                        default_model: defaultModel,
                    });
                    setModelsSaving(false);
                    setModelsSaved(true);
                    setTimeout(() => setModelsSaved(false), 2000);
                }} disabled={modelsSaving} style={{ fontSize: '12px' }}>
                    {modelsSaving ? 'Saving...' : 'Save Models'}
                </button>

                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '12px' }}>
                    Click the star to set the default model. New agents inherit their parent&apos;s model unless explicitly changed.
                </p>
            </div>

            {/* System Info */}
            <div className="panel" style={{ maxWidth: '700px', marginBottom: '24px' }}>
                <h2 style={{ fontSize: '18px', fontWeight: 600, marginBottom: '12px' }}>System Info</h2>
                <div style={{ display: 'grid', gridTemplateColumns: '140px 1fr', gap: '8px', fontSize: '13px' }}>
                    <span style={{ color: 'var(--text-muted)' }}>Version:</span>
                    <span style={{ fontFamily: 'monospace' }}>{settings.version || 'unknown'}</span>
                    <span style={{ color: 'var(--text-muted)' }}>Update channel:</span>
                    <span>{selectedChannel ? channelInfo[selectedChannel]?.label : '...'}</span>
                </div>
            </div>

            {/* Reset / Wipe Holding */}
            <div className="panel" style={{
                maxWidth: '700px',
                border: '1px solid rgba(239, 68, 68, 0.3)',
                background: 'linear-gradient(135deg, rgba(239,68,68,0.03), rgba(239,68,68,0.01))',
            }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '4px' }}>
                    <div style={{
                        width: '36px', height: '36px', borderRadius: '8px',
                        background: 'rgba(239,68,68,0.15)', display: 'flex',
                        alignItems: 'center', justifyContent: 'center', flexShrink: 0,
                    }}>
                        <Trash2 size={18} style={{ color: '#ef4444' }} />
                    </div>
                    <div>
                        <h2 style={{ fontSize: '18px', fontWeight: 600, color: '#ef4444' }}>Reset Holding Company</h2>
                        <p style={{ fontSize: '13px', color: 'var(--text-muted)', marginTop: '2px' }}>
                            Completely wipe all data and start fresh
                        </p>
                    </div>
                </div>

                <div style={{
                    padding: '12px 16px', borderRadius: '8px', margin: '16px 0',
                    background: 'rgba(239,68,68,0.08)', border: '1px solid rgba(239,68,68,0.15)',
                    fontSize: '13px', color: 'var(--text-muted)', lineHeight: '1.6',
                }}>
                    <div style={{ display: 'flex', gap: '8px', alignItems: 'flex-start' }}>
                        <AlertTriangle size={16} style={{ color: '#ef4444', flexShrink: 0, marginTop: '2px' }} />
                        <div>
                            <strong style={{ color: '#ef4444' }}>This action is irreversible.</strong> Resetting will permanently delete:
                            <ul style={{ margin: '6px 0 0 16px', padding: 0 }}>
                                <li>All companies, agents, managers, and workers</li>
                                <li>All conversations, threads, and message history</li>
                                <li>All memories, secrets, files, and meeting records</li>
                                <li>All financial records, ledger entries, and orders</li>
                                <li>All running agent containers (OpenClaw instances)</li>
                            </ul>
                            <p style={{ marginTop: '8px', marginBottom: 0 }}>
                                A new holding will be created with the settings below, and a fresh MAIN agent will boot up.
                                You can also use this to change your holding settings without needing to reinstall.
                            </p>
                        </div>
                    </div>
                </div>

                {/* Editable Settings */}
                <div style={{ marginBottom: '16px' }}>
                    <h3 style={{ fontSize: '14px', fontWeight: 600, marginBottom: '12px', color: 'var(--text)' }}>
                        Holding Settings
                    </h3>
                    <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '12px' }}>
                        Edit these values to change your configuration. These are the same settings from the initial install.
                    </p>

                    <div style={{ display: 'grid', gridTemplateColumns: '160px 1fr', gap: '10px', alignItems: 'center' }}>
                        <label style={{ fontSize: '13px', color: 'var(--text-muted)', fontWeight: 500 }}>Holding Name</label>
                        <input
                            value={resetHoldingName}
                            onChange={e => setResetHoldingName(e.target.value)}
                            placeholder="Main Holding"
                            style={{
                                padding: '8px 12px', borderRadius: '8px',
                                background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                color: 'var(--text)', fontSize: '13px',
                            }}
                        />

                        <label style={{ fontSize: '13px', color: 'var(--text-muted)', fontWeight: 500 }}>Main Agent Name</label>
                        <input
                            value={resetAgentName}
                            onChange={e => setResetAgentName(e.target.value)}
                            placeholder="KonnerBot"
                            style={{
                                padding: '8px 12px', borderRadius: '8px',
                                background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                color: 'var(--text)', fontSize: '13px',
                            }}
                        />

                        <label style={{ fontSize: '13px', color: 'var(--text-muted)', fontWeight: 500 }}>Default Model</label>
                        <input
                            value={resetModel}
                            onChange={e => setResetModel(e.target.value)}
                            placeholder="minimax-m2.7:cloud"
                            style={{
                                padding: '8px 12px', borderRadius: '8px',
                                background: 'rgba(255,255,255,0.05)', border: '1px solid var(--border)',
                                color: 'var(--text)', fontSize: '13px', fontFamily: 'monospace',
                            }}
                        />
                    </div>
                </div>

                <button
                    className="button danger"
                    onClick={() => { setShowResetModal(true); setResetConfirmText(''); setResetError(''); }}
                    style={{
                        fontSize: '13px', padding: '10px 20px',
                        display: 'flex', alignItems: 'center', gap: '8px',
                        background: 'linear-gradient(135deg, #dc2626, #b91c1c)',
                        border: '1px solid rgba(239,68,68,0.4)',
                        color: '#fff', fontWeight: 600,
                        borderRadius: '8px', cursor: 'pointer',
                    }}
                >
                    <Trash2 size={16} />
                    Wipe Everything & Reinitialize
                </button>

                <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '8px' }}>
                    You will be asked to confirm before anything is deleted.
                </p>
            </div>

            {/* Reset Confirmation Modal */}
            {showResetModal && (
                <div className="modal-overlay" onClick={() => setShowResetModal(false)} style={{
                    position: 'fixed', top: 0, left: 0, right: 0, bottom: 0,
                    background: 'rgba(0,0,0,0.7)', backdropFilter: 'blur(4px)',
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    zIndex: 1000,
                }}>
                    <div className="modal" onClick={e => e.stopPropagation()} style={{
                        background: 'var(--bg)', border: '1px solid rgba(239,68,68,0.3)',
                        borderRadius: '12px', padding: '28px', maxWidth: '480px', width: '90%',
                        boxShadow: '0 20px 60px rgba(0,0,0,0.5)',
                    }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '16px' }}>
                            <div style={{
                                width: '40px', height: '40px', borderRadius: '10px',
                                background: 'rgba(239,68,68,0.15)', display: 'flex',
                                alignItems: 'center', justifyContent: 'center',
                            }}>
                                <AlertTriangle size={22} style={{ color: '#ef4444' }} />
                            </div>
                            <div>
                                <h2 style={{ fontSize: '18px', fontWeight: 700, color: '#ef4444' }}>Confirm Reset</h2>
                                <p style={{ fontSize: '12px', color: 'var(--text-muted)' }}>This cannot be undone</p>
                            </div>
                        </div>

                        <div style={{
                            padding: '12px 14px', borderRadius: '8px', marginBottom: '16px',
                            background: 'rgba(239,68,68,0.06)', border: '1px solid rgba(239,68,68,0.12)',
                            fontSize: '13px', color: 'var(--text-muted)', lineHeight: '1.5',
                        }}>
                            You are about to <strong style={{ color: '#ef4444' }}>permanently delete all data</strong> in
                            your MultiClaw installation — every company, agent, conversation, memory, secret, and
                            financial record. A new holding will be created with these settings:
                        </div>

                        <div style={{
                            padding: '10px 14px', borderRadius: '8px', marginBottom: '16px',
                            background: 'rgba(255,255,255,0.03)', border: '1px solid var(--border)',
                            fontSize: '13px', fontFamily: 'monospace',
                        }}>
                            <div style={{ display: 'grid', gridTemplateColumns: '130px 1fr', gap: '4px' }}>
                                <span style={{ color: 'var(--text-muted)' }}>Holding:</span>
                                <span>{resetHoldingName || 'Main Holding'}</span>
                                <span style={{ color: 'var(--text-muted)' }}>Main Agent:</span>
                                <span>{resetAgentName || 'MainAgent'}</span>
                                <span style={{ color: 'var(--text-muted)' }}>Model:</span>
                                <span>{resetModel || 'minimax-m2.7:cloud'}</span>
                            </div>
                        </div>

                        <div style={{ marginBottom: '16px' }}>
                            <label style={{ fontSize: '13px', color: 'var(--text-muted)', display: 'block', marginBottom: '6px' }}>
                                Type <strong style={{ color: 'var(--text)', fontFamily: 'monospace' }}>CONFIRM</strong> to proceed:
                            </label>
                            <input
                                value={resetConfirmText}
                                onChange={e => setResetConfirmText(e.target.value)}
                                placeholder="Type CONFIRM"
                                autoFocus
                                style={{
                                    width: '100%', padding: '10px 12px', borderRadius: '8px',
                                    background: 'rgba(255,255,255,0.05)',
                                    border: resetConfirmText === 'CONFIRM'
                                        ? '1px solid rgba(239,68,68,0.5)'
                                        : '1px solid var(--border)',
                                    color: 'var(--text)', fontSize: '14px', fontFamily: 'monospace',
                                    textAlign: 'center', letterSpacing: '2px',
                                    boxSizing: 'border-box',
                                }}
                            />
                        </div>

                        {resetError && (
                            <div style={{
                                padding: '8px 12px', borderRadius: '6px', marginBottom: '12px',
                                background: 'rgba(239,68,68,0.1)', border: '1px solid rgba(239,68,68,0.2)',
                                fontSize: '12px', color: '#ef4444',
                            }}>
                                {resetError}
                            </div>
                        )}

                        <div style={{ display: 'flex', gap: '10px', justifyContent: 'flex-end' }}>
                            <button
                                onClick={() => setShowResetModal(false)}
                                style={{
                                    padding: '10px 20px', borderRadius: '8px', fontSize: '13px',
                                    background: 'transparent', border: '1px solid var(--border)',
                                    color: 'var(--text)', cursor: 'pointer', fontWeight: 500,
                                }}
                            >
                                Cancel
                            </button>
                            <button
                                disabled={resetConfirmText !== 'CONFIRM' || resetting}
                                onClick={async () => {
                                    setResetting(true);
                                    setResetError('');
                                    try {
                                        const result = await api.systemReset({
                                            holding_name: resetHoldingName || undefined,
                                            main_agent_name: resetAgentName || undefined,
                                            default_model: resetModel || undefined,
                                        });
                                        if (result?.status === 'reset_complete') {
                                            setShowResetModal(false);
                                            // Refresh the page to reload all state
                                            window.location.reload();
                                        } else if (result?.error) {
                                            setResetError(result.error);
                                        } else {
                                            setResetError('Unexpected response from server.');
                                        }
                                    } catch (err: any) {
                                        setResetError(err.message || 'Reset failed — check the server logs.');
                                    }
                                    setResetting(false);
                                }}
                                style={{
                                    padding: '10px 24px', borderRadius: '8px', fontSize: '13px',
                                    background: resetConfirmText === 'CONFIRM' && !resetting
                                        ? 'linear-gradient(135deg, #dc2626, #b91c1c)'
                                        : 'rgba(239,68,68,0.2)',
                                    border: '1px solid rgba(239,68,68,0.4)',
                                    color: resetConfirmText === 'CONFIRM' && !resetting ? '#fff' : 'rgba(255,255,255,0.3)',
                                    cursor: resetConfirmText === 'CONFIRM' && !resetting ? 'pointer' : 'not-allowed',
                                    fontWeight: 700, display: 'flex', alignItems: 'center', gap: '6px',
                                }}
                            >
                                {resetting ? (
                                    <>Resetting...</>
                                ) : (
                                    <><Trash2 size={14} /> Wipe & Reset</>
                                )}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
