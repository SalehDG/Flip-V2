import { useCallback, useEffect, useState } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { RetentionPreview, RetentionRunSummary } from '../../../services/tauri/types';
import './Retention.css';

const presets = [3, 7, 14, 30];

export default function Retention() {
  const [days, setDays] = useState(7);
  const [customDays, setCustomDays] = useState('');
  const [preview, setPreview] = useState<RetentionPreview | null>(null);
  const [lastRun, setLastRun] = useState<RetentionRunSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');

  const refresh = useCallback(async () => {
    try {
      const [settings, result] = await Promise.all([tauriBridge.getRetentionSettings(), tauriBridge.previewRetentionCleanup()]);
      setDays(settings.retentionDays);
      setPreview(result);
      setError('');
    } catch (e) { setError(String(e)); }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const saveDays = async (value: number) => {
    if (!Number.isInteger(value) || value < 1 || value > 365) { setError('Retensi harus 1–365 hari.'); return; }
    setBusy(true); setError('');
    try { const settings = await tauriBridge.setRetentionSettings(value); setDays(settings.retentionDays); setCustomDays(''); setMessage(`Masa retensi disimpan: ${value} hari.`); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const cleanup = async () => {
    const count = preview?.eligibleCount ?? 0;
    if (!window.confirm(`Hapus ${count} transaksi yang sudah melewati masa retensi beserta PNG yang tidak lagi digunakan? Tindakan ini permanen.`)) return;
    setBusy(true); setError(''); setMessage('');
    try { const result = await tauriBridge.runRetentionCleanup(); setLastRun(result); setMessage(`Cleanup selesai: ${result.deletedTransactions} transaksi dihapus, ${result.deletedPngFiles} PNG dibersihkan.`); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  return <section className="retention-page">
    <header className="retention-header"><div><span className="retention-eyebrow">DATA MANAGEMENT</span><h1>Data Retention</h1><p>Atur masa penyimpanan transaksi yang sudah disalin.</p></div><button className="retention-refresh" onClick={() => void refresh()} disabled={busy}>↻ Refresh</button></header>
    <div className="retention-settings-card">
      <div className="retention-card-title"><div className="retention-icon">◷</div><div><h2>Masa Retensi</h2><p>Perhitungan dimulai dari copy pertama. Copy ulang tidak mengubah tanggal akhir.</p></div></div>
      <div className="retention-presets">{presets.map(p => <button key={p} className={days === p ? 'selected' : ''} onClick={() => void saveDays(p)} disabled={busy}>{p}<small>hari</small></button>)}</div>
      <div className="retention-custom"><label htmlFor="retention-custom-days">Custom (1–365 hari)</label><div><input id="retention-custom-days" type="number" min="1" max="365" value={customDays} onChange={e => setCustomDays(e.target.value)} placeholder="Masukkan jumlah hari"/><button onClick={() => void saveDays(Number(customDays))} disabled={busy || !customDays}>Terapkan</button></div></div>
      <div className="retention-current">Masa retensi aktif: <strong>{days} hari</strong></div>
    </div>
    <div className="retention-preview-card"><div className="retention-card-title"><div className="retention-icon warning">⌫</div><div><h2>Preview Penghapusan</h2><p>Data yang akan dihapus pada cleanup berikutnya.</p></div></div>
      <div className="retention-preview-stats"><article><small>Transaksi kedaluwarsa</small><strong>{preview?.eligibleCount ?? '—'}</strong></article><article><small>PNG terkait</small><strong>{preview?.pngFileCount ?? '—'}</strong></article><article><small>Kedaluwarsa tertua</small><strong className="date-value">{preview?.oldestExpiredAt ? new Date(preview.oldestExpiredAt.replace(' ', 'T') + 'Z').toLocaleString() : 'Tidak ada'}</strong></article></div>
      <button className="retention-delete" onClick={() => void cleanup()} disabled={busy || !preview || preview.eligibleCount === 0}>{busy ? 'Memproses…' : 'Hapus data kedaluwarsa sekarang'}</button>
      <p className="retention-warning">Penghapusan permanen. Transaksi yang belum pernah disalin tidak dihapus. PNG hanya dihapus jika tidak digunakan transaksi lain.</p>
    </div>
    {message && <div className="retention-notice success">{message}</div>}{error && <div className="retention-notice error">{error}</div>}
    {lastRun && <div className="retention-last-run">Hasil terakhir: {lastRun.deletedTransactions} transaksi, {lastRun.deletedPngFiles} PNG terhapus, {lastRun.failedPngDeletions} kegagalan hapus file.</div>}
    <div className="retention-info"><strong>Cleanup otomatis</strong><p>Flip V2 menjalankan pemeriksaan saat aplikasi dimulai dan kemudian setiap 24 jam. Cleanup otomatis mengikuti masa retensi yang aktif.</p></div>
  </section>;
}
