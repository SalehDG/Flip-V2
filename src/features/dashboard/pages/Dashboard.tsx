import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ReactNode } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { ExtractionStatusSummary, ExtractionTransaction } from '../../../services/tauri/types';
import './Dashboard.css';

interface Props { onOpenTransactions: () => void; }
const empty: ExtractionStatusSummary = { detected: 0, parsed: 0, ocrProcessing: 0, completed: 0, needsReview: 0, duplicateReview: 0, failed: 0 };
const money = (amount: number | null) => amount == null ? '—' : `Rp${new Intl.NumberFormat('id-ID').format(amount)}`;
const dateOnly = (value: string | null) => {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('id-ID', { day: '2-digit', month: '2-digit', year: 'numeric' }).format(date);
};
const dateTime = (value: string | null) => {
  if (!value) return 'Belum ada sinkronisasi';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('id-ID', { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }).format(date);
};
const normalizeStatus = (status: string) => status.toLowerCase().replace(/[\s-]+/g, '_');
const statusLabel = (status: string) => ({
  completed: 'Selesai', needs_review: 'Perlu Ditinjau', duplicate_review: 'Duplikat', failed: 'Gagal', detected: 'Terdeteksi', parsed: 'Diparsing', ocr_processing: 'OCR Berjalan',
}[normalizeStatus(status)] ?? status.replace(/_/g, ' '));
const statusClass = (status: string) => normalizeStatus(status).replace(/_/g, '-');
function Icon({ name }: { name: 'mail' | 'stack' | 'check' | 'warning' | 'sync' | 'clock' | 'copy' }) {
  const paths: Record<string, ReactNode> = {
    mail: <><rect x="3" y="5" width="18" height="14" rx="2"/><path d="m4 7 8 6 8-6"/></>,
    stack: <><path d="m12 3 9 5-9 5-9-5 9-5Z"/><path d="m3 12 9 5 9-5M3 16l9 5 9-5"/></>,
    check: <path d="m5 12 4 4L19 6"/>, warning: <><path d="M12 3 22 20H2L12 3Z"/><path d="M12 9v4m0 3h.01"/></>,
    sync: <><path d="M20 7v5h-5"/><path d="M4 17v-5h5"/><path d="M5.6 9a7 7 0 0 1 11.7-2L20 12M4 12l2.7 5a7 7 0 0 0 11.7-2"/></>,
    clock: <><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></>,
    copy: <><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3"/></>,
  };
  return <svg className="dash-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

export default function Dashboard({ onOpenTransactions }: Props) {
  const [stats, setStats] = useState(empty);
  const [rows, setRows] = useState<ExtractionTransaction[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const [monitoringReady, setMonitoringReady] = useState(false);
  const [lastMonitoringCycle, setLastMonitoringCycle] = useState<string | null>(null);
  const [nextSyncIn, setNextSyncIn] = useState(60);

  const refresh = useCallback(async () => {
    try {
      const [summary, transactions] = await Promise.all([
        tauriBridge.getExtractionSummary(),
        tauriBridge.getTransactions({ limit: 5000, offset: 0 }),
      ]);
      setStats(summary);
      setRows(transactions);
      setError('');
    } catch (e) { setError(String(e)); }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    let cancelled = false;
    let unlistenStarted: (() => void) | undefined;
    let unlistenCycle: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    void Promise.all([
      listen('flip://monitoring-started', () => { if (!cancelled) setMonitoringReady(true); }),
      listen('flip://monitoring-cycle-complete', () => {
        if (!cancelled) { setLastMonitoringCycle(new Date().toISOString()); setNextSyncIn(60); void refresh(); }
      }),
      listen('flip://monitoring-error', () => { if (!cancelled) setMonitoringReady(true); }),
    ]).then(([started, cycle, errorListener]) => {
      if (cancelled) { started(); cycle(); errorListener(); return; }
      unlistenStarted = started; unlistenCycle = cycle; unlistenError = errorListener;
    });
    setMonitoringReady(true);
    return () => { cancelled = true; unlistenStarted?.(); unlistenCycle?.(); unlistenError?.(); };
  }, [refresh]);
  useEffect(() => {
    const timer = window.setInterval(() => setNextSyncIn(current => current > 0 ? current - 1 : 60), 1000);
    return () => window.clearInterval(timer);
  }, []);

  async function manualSync() {
    setBusy(true); setError(''); setNotice('Sinkronisasi sedang berjalan…'); setNextSyncIn(60);
    try {
      const result = await tauriBridge.runExtraction();
      setNotice(`Sinkronisasi selesai: ${result.processed} diproses, ${result.completed} selesai, ${result.needsReview} perlu ditinjau.`);
      setNextSyncIn(60); setLastMonitoringCycle(new Date().toISOString()); await refresh();
    } catch (e) { setError(String(e)); setNotice(''); }
    finally { setBusy(false); }
  }

  const total = stats.detected + stats.parsed + stats.ocrProcessing + stats.completed + stats.needsReview + stats.duplicateReview + stats.failed;
  const copiedCount = rows.filter(row => row.lastCopiedAt != null).length;
  const percentCopied = total ? Math.min(100, Math.round((copiedCount / total) * 100)) : 0;
  const percentDone = total ? Math.round((stats.completed / total) * 100) : 0;
  const percentReview = total ? Math.round((stats.needsReview / total) * 100) : 0;
  const lastSyncLabel = lastMonitoringCycle ? dateTime(lastMonitoringCycle) : 'Monitoring sedang berjalan';

  return <section className="dashboard-page">
    <div className="dash-welcome-row">
      <div className="dash-welcome-copy">
        <h1>Selamat Datang <span aria-hidden="true">👋</span></h1>
        <p>Pengambilan transaksi Flip berjalan otomatis.</p>
      </div>
      <div className="dash-monitor-pill" aria-label="Status monitoring otomatis">
        <span className="dash-green-dot" />
        <div className="dash-monitor-copy">
          <strong>{monitoringReady ? 'Monitoring Otomatis Aktif' : 'Menyiapkan Monitoring'}</strong>
          <small>Sinkronisasi setiap 60 detik</small>
          <span className="dash-monitor-last">{lastSyncLabel}</span>
        </div>
        <div className="dash-monitor-countdown">
          <small>Sync berikutnya</small>
          <strong>{Math.floor(nextSyncIn / 60).toString().padStart(2, '0')}:{(nextSyncIn % 60).toString().padStart(2, '0')}</strong>
        </div>
      </div>
      <button className="dash-manual-sync" onClick={() => void manualSync()} disabled={busy}><Icon name="sync"/>{busy ? 'Memproses…' : 'Sync Manual'}</button>
    </div>
    {notice && <div className="notice" role="status">{notice}</div>}{error && <div className="error-banner" role="alert">{error}</div>}

    <div className="dash-stats-grid">
      <article className="dash-stat-card total"><div className="dash-stat-icon"><Icon name="stack"/></div><div className="dash-stat-content"><span>Total Transaksi</span><strong>{total.toLocaleString('id-ID')}</strong><small>Seluruh status</small></div></article>
      <article className="dash-stat-card copied"><div className="dash-stat-icon"><Icon name="copy"/></div><div className="dash-stat-content"><span>Di-copy</span><div className="dash-stat-value-row"><strong>{copiedCount.toLocaleString('id-ID')}</strong><em>{percentCopied}%</em></div><small>Transaksi telah di-copy</small></div></article>
      <article className="dash-stat-card green"><div className="dash-stat-icon"><Icon name="check"/></div><div className="dash-stat-content"><span>Selesai</span><div className="dash-stat-value-row"><strong>{stats.completed.toLocaleString('id-ID')}</strong><em>{percentDone}%</em></div><small>Dari total transaksi</small></div></article>
      <article className="dash-stat-card orange"><div className="dash-stat-icon"><Icon name="warning"/></div><div className="dash-stat-content"><span>Perlu Ditinjau</span><div className="dash-stat-value-row"><strong>{stats.needsReview.toLocaleString('id-ID')}</strong><em>{percentReview}%</em></div><small>Dari total transaksi</small></div></article>
    </div>

    <section className="dash-table-panel">
      <div className="dash-table-title"><h2><Icon name="stack"/>Transaksi Terbaru</h2><button className="dash-refresh" onClick={() => void refresh()}><Icon name="sync"/>Segarkan</button></div>
      <div className="dash-table-scroll"><table className="dash-transactions-table"><thead><tr><th>Waktu Email</th><th>Nominal</th><th>Penerima</th><th>Catatan Transfer</th><th>Status</th></tr></thead><tbody>
        {rows.length ? rows.slice(0, 5).map(row => <tr key={row.id} className={normalizeStatus(row.status) === 'needs_review' ? 'needs-review-row' : ''}><td>{dateOnly(row.transactionDate)}</td><td className="dash-amount">{money(row.amount)}</td><td>{row.recipientName || '—'}</td><td className={normalizeStatus(row.status) === 'needs_review' && !row.transferNote ? 'dash-note-review' : ''}>{row.transferNote || (normalizeStatus(row.status) === 'needs_review' ? '(Catatan belum jelas)' : '—')}</td><td><span className={`dash-status ${statusClass(row.status)}`}>{normalizeStatus(row.status) === 'completed' ? <Icon name="check"/> : normalizeStatus(row.status) === 'needs_review' ? <Icon name="warning"/> : null}{statusLabel(row.status)}</span></td></tr>) : <tr><td colSpan={5} className="dash-empty">Belum ada transaksi.</td></tr>}
      </tbody></table></div>
      <div className="dash-table-footer"><span>Menampilkan maksimal 5 transaksi terbaru</span><button className="dash-view-all" onClick={onOpenTransactions}>Lihat semua transaksi <span aria-hidden="true">→</span></button></div>
    </section>
  </section>;
}
