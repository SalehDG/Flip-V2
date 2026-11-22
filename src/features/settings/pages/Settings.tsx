import { useCallback, useEffect, useState } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { MonitoringSettings, StorageInfo } from '../../../services/tauri/types';
import './Settings.css';

const intervalOptions = [30, 60, 120, 300, 600, 1800, 3600];
const formatBytes = (bytes: number) => {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / Math.pow(1024, index)).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
};


export default function Settings() {
  const [monitoring, setMonitoring] = useState<MonitoringSettings | null>(null);
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [monitoringError, setMonitoringError] = useState('');
  const [storageError, setStorageError] = useState('');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');

  const refresh = useCallback(async () => {
    setLoading(true);
    setMonitoringError('');
    setStorageError('');
    setError('');

    // Load each panel independently so a failed command never leaves both panels blank.
    const monitoringRequest = tauriBridge.getMonitoringSettings()
      .then((value) => { setMonitoring(value); })
      .catch((e) => { setMonitoringError(String(e)); });
    const storageRequest = tauriBridge.getStorageInfo()
      .then((value) => { setStorage(value); })
      .catch((e) => { setStorageError(String(e)); });

    await Promise.all([monitoringRequest, storageRequest]);
    setLoading(false);
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const saveMonitoring = async (enabled: boolean, intervalSeconds: number) => {
    setBusy(true); setError(''); setMessage('');
    try {
      const saved = await tauriBridge.setMonitoringSettings(enabled, intervalSeconds);
      setMonitoring(saved);
      setMessage('Pengaturan monitoring berhasil disimpan dan diterapkan.');
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  return <section className="settings-page">
    <header className="settings-header"><div><span className="settings-eyebrow">PREFERENCES</span><h1>Settings</h1><p>Konfigurasi perilaku Flip V2 dan pantau penggunaan penyimpanan lokal.</p></div><button className="settings-refresh" onClick={() => void refresh()} disabled={busy}>↻ Refresh</button></header>
    <section className="settings-card">
      <div className="settings-card-head"><div className="settings-icon">◉</div><div><h2>Automatic Monitoring</h2><p>Kontrol pemantauan Gmail otomatis yang berjalan di background.</p></div></div>
      {loading && !monitoring && <div className="settings-state">Memuat pengaturan monitoring…</div>}
      {monitoringError && <div className="settings-state error">Gagal memuat monitoring: {monitoringError}<button onClick={() => void refresh()}>Coba lagi</button></div>}
      {monitoring && <>
        <div className="settings-toggle-row"><div><strong>Monitoring otomatis</strong><small>{monitoring.enabled ? 'Flip V2 memeriksa akun Gmail yang terhubung.' : 'Monitoring dijeda. Sinkronisasi otomatis tidak dijalankan.'}</small></div><button className={`settings-switch ${monitoring.enabled ? 'on' : ''}`} role="switch" aria-checked={monitoring.enabled} disabled={busy} onClick={() => void saveMonitoring(!monitoring.enabled, monitoring.intervalSeconds)}><span /></button></div>
        <div className="settings-control-row"><div><strong>Interval pemeriksaan</strong><small>Jarak waktu antar siklus monitoring. Minimum 30 detik.</small></div><select value={monitoring.intervalSeconds} disabled={busy} onChange={e => void saveMonitoring(monitoring.enabled, Number(e.target.value))}>{intervalOptions.map(seconds => <option value={seconds} key={seconds}>{seconds >= 60 ? `${seconds / 60} menit${seconds % 60 ? ` ${seconds % 60} detik` : ''}` : `${seconds} detik`}</option>)}</select></div>
        <div className="settings-inline-info"><span>●</span> Perubahan langsung diterapkan tanpa restart aplikasi.</div>
      </>}
    </section>
    <section className="settings-card">
      <div className="settings-card-head"><div className="settings-icon storage">▤</div><div><h2>Local Storage</h2><p>Data Flip V2 disimpan secara lokal di perangkat ini.</p></div></div>
      {loading && !storage && <div className="settings-state">Memuat informasi storage…</div>}
      {storageError && <div className="settings-state error">Gagal memuat storage: {storageError}<button onClick={() => void refresh()}>Coba lagi</button></div>}
      {storage && <><div className="settings-storage-grid"><article><small>Database</small><strong>{formatBytes(storage.databaseBytes)}</strong></article><article><small>PNG tersimpan</small><strong>{formatBytes(storage.pngBytes)}</strong><span>{storage.pngFiles} file</span></article><article><small>Total penggunaan</small><strong>{formatBytes(storage.totalBytes)}</strong></article></div><div className="settings-path"><small>Direktori data</small><code>{storage.dataDirectory}</code></div></>}
    </section>
    <section className="settings-card settings-other-card"><div className="settings-card-head"><div className="settings-icon other">⚙</div><div><h2>Pengaturan lainnya</h2><p>Pengaturan berikut dikelola di halaman masing-masing.</p></div></div><div className="settings-other-row"><div><strong>Data Retention</strong><small>Atur periode penyimpanan transaksi dan cleanup otomatis.</small></div><span>7 hari default · 1–365 hari</span></div><div className="settings-other-row"><div><strong>Recovery ekstraksi</strong><small>Retry otomatis dibatasi agar tidak terjadi loop tanpa batas.</small></div><span>3 percobaan maksimum</span></div><div className="settings-other-row"><div><strong>Startup Windows</strong><small>Start otomatis saat login belum tersedia pada tahap ini.</small></div><span className="settings-unavailable">Belum tersedia</span></div></section>
    {message && <div className="settings-feedback success">{message}</div>}{error && <div className="settings-feedback error">{error}</div>}
  </section>;
}
