import { useCallback, useEffect, useState } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { MonitoringSettings, StorageInfo, RetentionSettings } from '../../../services/tauri/types';
import GmailAccountTest from '../../gmail/components/GmailAccountTest';
import Logs from '../../logs/pages/Logs';
import './Settings.css';

type SettingsTab = 'general' | 'accounts' | 'logs';
const retentionPresets = [1, 2, 3];
const intervalOptions = [30, 60, 120, 300, 600, 1800, 3600];
const formatBytes = (bytes: number) => {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / Math.pow(1024, index)).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
};


export default function Settings() {
  const [tab, setTab] = useState<SettingsTab>('general');
  const [monitoring, setMonitoring] = useState<MonitoringSettings | null>(null);
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [retention, setRetention] = useState<RetentionSettings | null>(null);
  const [customDays, setCustomDays] = useState('');
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const refresh = useCallback(async () => {
    setLoading(true); setError('');
    const [monitoringResult, storageResult, retentionResult] = await Promise.allSettled([
      tauriBridge.getMonitoringSettings(), tauriBridge.getStorageInfo(), tauriBridge.getRetentionSettings(),
    ]) as [PromiseSettledResult<MonitoringSettings>, PromiseSettledResult<StorageInfo>, PromiseSettledResult<RetentionSettings>];
    if (monitoringResult.status === 'fulfilled') setMonitoring(monitoringResult.value);
    if (storageResult.status === 'fulfilled') setStorage(storageResult.value);
    if (retentionResult.status === 'fulfilled') { setRetention(retentionResult.value); setCustomDays(String(retentionResult.value.retentionDays)); }
    const errors = [monitoringResult, storageResult, retentionResult].filter((r): r is PromiseRejectedResult => r.status === 'rejected').map(r => String(r.reason));
    if (errors.length) setError(errors.join('\n'));
    setLoading(false);
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const saveMonitoring = async (enabled: boolean, intervalSeconds: number) => {
    setBusy(true); setError(''); setMessage('');
    try { setMonitoring(await tauriBridge.setMonitoringSettings(enabled, intervalSeconds)); setMessage('Monitoring berhasil diperbarui.'); }
    catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const saveRetention = async (days: number) => {
    if (!Number.isInteger(days) || days < 1 || days > 365) { setError('Masukkan masa retensi 1–365 hari.'); return; }
    setBusy(true); setError(''); setMessage('');
    try { setRetention(await tauriBridge.setRetentionSettings(days)); setCustomDays(String(days)); setMessage(`Retensi berhasil disimpan: ${days} hari sejak copy terakhir.`); }
    catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const openStorage = async () => { setError(''); try { await tauriBridge.openStorageDirectory(); } catch (e) { setError(String(e)); } };
  const tabs: { id: SettingsTab; title: string; desc: string; icon: string }[] = [
    { id: 'general', title: 'Umum', desc: 'Retensi dan penyimpanan', icon: '⚙' },
    { id: 'accounts', title: 'Akun', desc: 'Akun Gmail terhubung', icon: '✉' },
    { id: 'logs', title: 'Log', desc: 'Riwayat aktivitas aplikasi', icon: '◷' },
  ];

  return <section className="settings-page">
    <header className="settings-header"><div><span className="settings-eyebrow">PREFERENCES</span><h1>Pengaturan</h1><p>Atur preferensi Flip V2, akun, dan aktivitas aplikasi.</p></div><button className="settings-refresh" onClick={() => void refresh()} disabled={busy || loading}>↻ Refresh</button></header>
    <div className="settings-layout">
      <nav className="settings-tabs" aria-label="Sub halaman pengaturan">{tabs.map(item => <button key={item.id} className={tab === item.id ? 'active' : ''} onClick={() => setTab(item.id)}><span className="settings-tab-icon">{item.icon}</span><span className="settings-tab-copy"><strong>{item.title}</strong><small>{item.desc}</small></span><span className="settings-tab-arrow">›</span></button>)}</nav>
      <div className="settings-content">
        {tab === 'general' && <>
          <section className="settings-card"><div className="settings-card-head"><div className="settings-icon retention">◷</div><div><h2>Data Retention</h2><p>Atur berapa lama transaksi yang sudah di-copy disimpan sebelum dibersihkan otomatis.</p></div></div>
            {loading && !retention ? <div className="settings-state">Memuat masa retensi…</div> : retention && <>
              <div className="retention-setting-label"><strong>Hapus data setelah</strong><small>Perhitungan dimulai dari copy terakhir. Transaksi yang belum pernah di-copy tidak akan dihapus otomatis.</small></div>
              <div className="retention-preset-grid">{retentionPresets.map(days => <button key={days} className={`retention-preset ${retention.retentionDays === days ? 'selected' : ''}`} disabled={busy} onClick={() => void saveRetention(days)}><strong>{days}</strong><span>Hari</span>{days === 3 && <small>Rekomendasi</small>}</button>)}<div className="retention-custom"><label htmlFor="retention-custom-days">Custom</label><div><input id="retention-custom-days" type="number" min="1" max="365" value={customDays} onChange={e => setCustomDays(e.target.value)} disabled={busy}/><span>hari</span><button disabled={busy || !customDays || Number(customDays) === retention.retentionDays} onClick={() => void saveRetention(Number(customDays))}>Terapkan</button></div></div></div>
              <div className="settings-inline-info"><span>i</span> Copy ulang akan memperbarui waktu retensi. Data dibersihkan otomatis setelah masa ini berlalu.</div>
            </>}
          </section>
          <section className="settings-card"><div className="settings-card-head"><div className="settings-icon storage">▤</div><div><h2>Lokasi Penyimpanan</h2><p>Database dan file invoice tersimpan secara lokal di perangkat.</p></div></div>
            {loading && !storage ? <div className="settings-state">Memuat informasi storage…</div> : storage && <><div className="settings-storage-grid"><article><small>Database</small><strong>{formatBytes(storage.databaseBytes)}</strong></article><article><small>File invoice</small><strong>{formatBytes(storage.pngBytes)}</strong><span>{storage.pngFiles} file</span></article><article><small>Total</small><strong>{formatBytes(storage.totalBytes)}</strong></article></div><div className="settings-path"><small>Folder data aplikasi</small><code>{storage.dataDirectory}</code><button className="settings-open-folder" onClick={() => void openStorage()}>↗ Buka folder penyimpanan</button></div></>}</section>
          <section className="settings-card"><div className="settings-card-head"><div className="settings-icon monitoring">◉</div><div><h2>Automatic Monitoring</h2><p>Kontrol pemeriksaan email Gmail yang berjalan di background.</p></div></div>{loading && !monitoring ? <div className="settings-state">Memuat pengaturan…</div> : monitoring && <><div className="settings-toggle-row"><div><strong>Monitoring otomatis</strong><small>{monitoring.enabled ? 'Flip V2 memeriksa akun Gmail terhubung.' : 'Monitoring sedang dijeda.'}</small></div><button className={`settings-switch ${monitoring.enabled ? 'on' : ''}`} role="switch" aria-checked={monitoring.enabled} disabled={busy} onClick={() => void saveMonitoring(!monitoring.enabled, monitoring.intervalSeconds)}><span/></button></div><div className="settings-control-row"><div><strong>Interval pemeriksaan</strong><small>Jarak waktu antar siklus monitoring.</small></div><select value={monitoring.intervalSeconds} disabled={busy} onChange={e => void saveMonitoring(monitoring.enabled, Number(e.target.value))}>{intervalOptions.map(seconds => <option value={seconds} key={seconds}>{seconds >= 60 ? `${seconds / 60} menit${seconds % 60 ? ` ${seconds % 60} detik` : ''}` : `${seconds} detik`}</option>)}</select></div></>}</section>
        </>}
        {tab === 'accounts' && <div className="settings-subpage"><GmailAccountTest /></div>}
        {tab === 'logs' && <div className="settings-subpage"><Logs /></div>}
      </div>
    </div>
    {message && <div className="settings-feedback success">✓ {message}</div>}{error && <div className="settings-feedback error">{error}</div>}
  </section>;
}
