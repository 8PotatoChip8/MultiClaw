'use client';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { Settings as SettingsIcon, ArrowUpCircle, Check, Info, Shield, GitBranch, Zap } from 'lucide-react';

type Channel = 'stable' | 'beta' | 'dev';

const channelInfo: Record<Channel, { label: string; desc: string; icon: any; color: string }> = {
    stable: { label: 'Stable', desc: 'Fully tested production releases. Updated only when a new version is formally published.', icon: Shield, color: 'var(--success)' },
    beta: { label: 'Beta', desc: 'Experimental features from the beta branch. May contain bugs or incomplete features.', icon: GitBranch, color: '#f59e0b' },
    dev: { label: 'Dev', desc: 'Latest commits from main. Bleeding-edge — may break at any time.', icon: Zap, color: '#ef4444' },
};

export default function SettingsPage() {
    const [settings, setSettings] = useState<Record<string, string>>({});
    const [selectedChannel, setSelectedChannel] = useState<Channel>('stable');
    const [saving, setSaving] = useState(false);
    const [saved, setSaved] = useState(false);
    const [updateInfo, setUpdateInfo] = useState<any>(null);
    const [checking, setChecking] = useState(false);

    useEffect(() => {
        api.getSettings().then(data => {
            if (data && typeof data === 'object') {
                setSettings(data);
                if (data.update_channel) setSelectedChannel(data.update_channel as Channel);
            }
        });
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
    };

    const handleCheckUpdate = async () => {
        setChecking(true);
        const info = await api.checkForUpdate();
        setUpdateInfo(info);
        if (info) localStorage.setItem('_update_info', JSON.stringify(info));
        setChecking(false);
    };

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
                    {(Object.keys(channelInfo) as Channel[]).map(ch => {
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

            {/* System Info */}
            <div className="panel" style={{ maxWidth: '700px' }}>
                <h2 style={{ fontSize: '18px', fontWeight: 600, marginBottom: '12px' }}>System Info</h2>
                <div style={{ display: 'grid', gridTemplateColumns: '140px 1fr', gap: '8px', fontSize: '13px' }}>
                    <span style={{ color: 'var(--text-muted)' }}>Version:</span>
                    <span style={{ fontFamily: 'monospace' }}>{settings.version || 'unknown'}</span>
                    <span style={{ color: 'var(--text-muted)' }}>Update channel:</span>
                    <span>{channelInfo[selectedChannel]?.label}</span>
                </div>
            </div>
        </div>
    );
}
