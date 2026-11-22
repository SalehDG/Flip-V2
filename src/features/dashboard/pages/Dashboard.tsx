import { useCallback, useEffect, useState } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { ExtractionStatusSummary, ExtractionSummary } from '../../../services/tauri/types';

interface Props { onOpenTransactions: () => void; }
const empty: ExtractionStatusSummary = { detected: 0, parsed: 0, ocrProcessing: 0, completed: 0, needsReview: 0, duplicateReview: 0, failed: 0 };
export default function Dashboard({ onOpenTransactions }: Props) {
  const [stats, setStats] = useState(empty);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const refresh = useCallback(async () => { try { setStats(await tauriBridge.getExtractionSummary()); } catch (e) { setError(String(e)); } }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  async function processQueue() {
    setBusy(true); setError(''); setNotice('Menjalankan proses ekstraksi antrean...');
    try {
      const result: ExtractionSummary = await tauriBridge.runExtraction();
      setNotice(`Pemrosesan selesai: ${result.processed} diproses, ${result.completed} selesai, ${result.needsReview} perlu ditinjau.`);
      await refresh();
    } catch (e) { setError(String(e)); setNotice(''); }
    finally { setBusy(false); }
  }
  const cards = [
    ['Selesai', stats.completed, 'completed'], ['Perlu Ditinjau', stats.needsReview, 'review'],
    ['Duplikat', stats.duplicateReview, 'duplicate'], ['Gagal', stats.failed, 'failed'],
  ] as const;
  return <section className="dashboard-page">
    <div className="page-heading"><div><h2>Dashboard</h2><p>Ringkasan pemrosesan transaksi Flip dari data lokal.</p></div><button className="primary" onClick={() => void processQueue()} disabled={busy}>{busy ? 'Memproses…' : '▶  Jalankan Ekstraksi'}</button></div>
    {notice && <div className="notice" role="status">{notice}</div>}{error && <div className="error-banner" role="alert">{error}</div>}
    <div className="stat-grid">{cards.map(([label, value, kind]) => <article key={kind} className={`stat-card ${kind}`}><span>{label}</span><strong>{value}</strong></article>)}</div>
    <div className="dashboard-grid"><article className="panel"><div className="panel-head"><h3>Antrean Pemrosesan</h3><button className="subtle" onClick={() => void refresh()}>Refresh</button></div><div className="queue-row"><span>Detected</span><strong>{stats.detected}</strong></div><div className="queue-row"><span>Parsed</span><strong>{stats.parsed}</strong></div><div className="queue-row"><span>OCR Processing</span><strong>{stats.ocrProcessing}</strong></div><p className="muted">Ekstraksi otomatis berjalan saat aplikasi startup. Kamu juga bisa menjalankannya manual.</p></article>
    <article className="panel"><h3>Perlu Tindakan</h3><p>Periksa transaksi yang membutuhkan validasi atau mengalami kegagalan OCR.</p><div className="action-summary"><div><strong>{stats.needsReview + stats.duplicateReview + stats.failed}</strong><span>transaksi membutuhkan tindakan</span></div><button onClick={onOpenTransactions}>Buka transaksi →</button></div></article></div>
  </section>;
}
