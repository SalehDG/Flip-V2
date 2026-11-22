import { useCallback, useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ReactNode } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { ExtractionStatusSummary, ExtractionTransaction } from '../../../services/tauri/types';

interface Props { onOpenTransactions: () => void; }
const empty: ExtractionStatusSummary = { detected: 0, parsed: 0, ocrProcessing: 0, completed: 0, needsReview: 0, duplicateReview: 0, failed: 0 };
type Filter = 'all' | 'completed' | 'review';

const money = (amount: number | null) => amount == null ? '—' : `Rp${new Intl.NumberFormat('id-ID').format(amount)}`;
const dateTime = (value: string | null) => {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('id-ID', { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }).format(date);
};
const normalizeStatus = (status: string) => status.toLowerCase().replace(/[\s-]+/g, '_');
const statusLabel = (status: string) => ({
  completed: 'Selesai', needs_review: 'Perlu Ditinjau', duplicate_review: 'Duplikat', failed: 'Gagal', detected: 'Terdeteksi', parsed: 'Diparsing', ocr_processing: 'OCR Berjalan',
}[normalizeStatus(status)] ?? status.replace(/_/g, ' '));
const statusClass = (status: string) => normalizeStatus(status).replace(/_/g, '-');
function Icon({ name }: { name: 'mail' | 'stack' | 'check' | 'warning' | 'calendar' | 'search' | 'edit' | 'more' | 'copy' | 'bell' | 'home' }) {
  const paths: Record<string, ReactNode> = {
    mail: <><rect x="3" y="5" width="18" height="14" rx="2"/><path d="m4 7 8 6 8-6"/></>,
    stack: <><path d="m12 3 9 5-9 5-9-5 9-5Z"/><path d="m3 12 9 5 9-5M3 16l9 5 9-5"/></>,
    check: <path d="m5 12 4 4L19 6"/>, warning: <><path d="M12 3 22 20H2L12 3Z"/><path d="M12 9v4m0 3h.01"/></>,
    calendar: <><rect x="3" y="4" width="18" height="17" rx="2"/><path d="M16 2v4M8 2v4M3 9h18"/></>,
    search: <><circle cx="10.8" cy="10.8" r="6.8"/><path d="m16 16 5 5"/></>, edit: <><path d="m4 16.5 11-11a2.1 2.1 0 0 1 3 3l-11 11L3 20l1-3.5Z"/><path d="m13.5 7.5 3 3"/></>,
    more: <><circle cx="12" cy="5" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="12" cy="19" r="1"/></>,
    copy: <><rect x="8" y="8" width="12" height="13" rx="2"/><path d="M16 5V3H5a2 2 0 0 0-2 2v12h2"/></>,
    bell: <><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9"/><path d="M10 21h4"/></>,
    home: <><path d="m3 10 9-7 9 7"/><path d="M5 9v12h14V9M9 21v-7h6v7"/></>,
  };
  return <svg className="dash-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

export default function Dashboard({ onOpenTransactions }: Props) {
  const [stats, setStats] = useState(empty);
  const [rows, setRows] = useState<ExtractionTransaction[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<number[]>([]);
  const [dateRange, setDateRange] = useState('7');
  const [monitoringReady, setMonitoringReady] = useState(false);
  const [lastMonitoringCycle, setLastMonitoringCycle] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      const [summary, transactions] = await Promise.all([tauriBridge.getExtractionSummary(), tauriBridge.getTransactions({ limit: 5, offset: 0 })]);
      setStats(summary); setRows(transactions);
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
      listen('flip://monitoring-cycle-complete', () => { if (!cancelled) setLastMonitoringCycle(new Date().toISOString()); }),
      listen('flip://monitoring-error', () => { if (!cancelled) setMonitoringReady(true); }),
    ]).then(([started, cycle, error]) => {
      if (cancelled) { started(); cycle(); error(); return; }
      unlistenStarted = started; unlistenCycle = cycle; unlistenError = error;
    });
    // The backend starts the monitor during app setup; this UI indicator reflects
    // the configured app-lifetime service, not whether an account is connected.
    setMonitoringReady(true);
    return () => {
      cancelled = true;
      unlistenStarted?.(); unlistenCycle?.(); unlistenError?.();
    };
  }, []);
  async function processQueue() {
    setBusy(true); setError(''); setNotice('Menjalankan proses ekstraksi antrean…');
    try { const result = await tauriBridge.runExtraction(); setNotice(`Pemrosesan selesai: ${result.processed} diproses, ${result.completed} selesai, ${result.needsReview} perlu ditinjau.`); await refresh(); }
    catch (e) { setError(String(e)); setNotice(''); } finally { setBusy(false); }
  }
  const total = stats.detected + stats.parsed + stats.ocrProcessing + stats.completed + stats.needsReview + stats.duplicateReview + stats.failed;
  const filteredRows = useMemo(() => rows.filter(row => {
    const matchesFilter = filter === 'all' || (filter === 'completed' && normalizeStatus(row.status) === 'completed') || (filter === 'review' && normalizeStatus(row.status) === 'needs_review');
    const date = row.transactionDate ? new Date(row.transactionDate) : null;
    const matchesDate = dateRange === 'all' || !date || Number.isNaN(date.getTime()) || (Date.now() - date.getTime()) <= Number(dateRange) * 86400000;
    const term = search.toLowerCase().trim();
    return matchesFilter && matchesDate && (!term || [row.recipientName, row.transferNote, row.accountEmail, row.flipTransactionId].some(value => value?.toLowerCase().includes(term)));
  }), [rows, filter, search, dateRange]);
  const selectableRows = filteredRows.filter(row => ['completed', 'needs_review'].includes(normalizeStatus(row.status)));
  const allSelected = selectableRows.length > 0 && selectableRows.every(row => selected.includes(row.id));
  const toggleAll = () => setSelected(allSelected ? selected.filter(id => !selectableRows.some(row => row.id === id)) : Array.from(new Set([...selected, ...selectableRows.map(row => row.id)])));
  async function copySelected() {
    const chosen = rows.filter(row => selected.includes(row.id) && ['completed', 'needs_review'].includes(normalizeStatus(row.status)));
    if (!chosen.length) { setNotice('Pilih transaksi yang ingin disalin terlebih dahulu.'); return; }
    const reviewCount = chosen.filter(row => normalizeStatus(row.status) === 'needs_review').length;
    if (reviewCount && !window.confirm(`Ada ${reviewCount} transaksi yang perlu ditinjau. Tetap salin?`)) return;
    const tsv = chosen.map(row => [row.transactionDate ? new Date(row.transactionDate).toLocaleDateString('id-ID') : '', row.transferNote ?? '', row.recipientName ?? '', '', row.amount ?? '', '', ''].join('\t')).join('\n');
    try { await navigator.clipboard.writeText(tsv); try { await tauriBridge.markTransactionsCopied(chosen.map(row => row.id)); setNotice(`${chosen.length} transaksi berhasil disalin. Copy tracking tersimpan.`); } catch (trackingError) { setNotice(`${chosen.length} transaksi disalin, tetapi tracking gagal disimpan: ${String(trackingError)}`); } setSelected([]); }
    catch (e) { setError(`Gagal menyalin ke clipboard: ${String(e)}`); }
  }
  return <section className="dashboard-page">
    <div className="dash-welcome-row"><div><h1>Selamat datang di Flip V2 <span aria-hidden="true">👋</span></h1><p>Pengambil transaksi Flip dari Gmail, lengkap dengan OCR Catatan Transfer.</p></div>
      <div className="dash-monitor-pill"><span className="dash-green-dot"/><div><strong>{monitoringReady ? 'Monitoring Otomatis Aktif' : 'Menyiapkan Monitoring'}</strong><small>{lastMonitoringCycle ? `Siklus terakhir: ${dateTime(lastMonitoringCycle)}` : 'Sync otomatis setiap 60 detik'}</small></div></div>
      <button className="dash-bell" aria-label="Notifikasi"><Icon name="bell"/><span className="dash-bell-dot"/></button>
    </div>
    {notice && <div className="notice" role="status">{notice}</div>}{error && <div className="error-banner" role="alert">{error}</div>}
    <div className="dash-stats-grid">
      <article className="dash-stat-card blue"><div className="dash-stat-icon"><Icon name="mail"/></div><div><span>Email Diproses</span><strong>{stats.parsed.toLocaleString('id-ID')}</strong><small className="dash-trend">↗ Data berhasil diparsing</small></div></article>
      <article className="dash-stat-card purple"><div className="dash-stat-icon"><Icon name="stack"/></div><div><span>Transaksi</span><strong>{total.toLocaleString('id-ID')}</strong><small className="dash-trend">Jumlah seluruh status</small></div></article>
      <article className="dash-stat-card green"><div className="dash-stat-icon"><Icon name="check"/></div><div><span>Selesai</span><strong>{stats.completed.toLocaleString('id-ID')}</strong><small>{total ? `${((stats.completed / total) * 100).toFixed(1).replace('.', ',')}% dari total` : '0% dari total'}</small></div></article>
      <article className="dash-stat-card orange"><div className="dash-stat-icon"><Icon name="warning"/></div><div><span>Perlu Ditinjau</span><strong>{stats.needsReview.toLocaleString('id-ID')}</strong><small>{total ? `${((stats.needsReview / total) * 100).toFixed(1).replace('.', ',')}% dari total` : '0% dari total'}</small></div></article>
    </div>
    <section className="dash-table-panel">
      <div className="dash-table-title"><h2>Transaksi Terbaru</h2><button className="dash-refresh" onClick={() => void refresh()}>↻ Segarkan</button></div>
      <div className="dash-table-controls"><div className="dash-tabs"><button className={filter === 'all' ? 'active' : ''} onClick={() => setFilter('all')}>Semua ({total.toLocaleString('id-ID')})</button><button className={filter === 'completed' ? 'active' : ''} onClick={() => setFilter('completed')}>Selesai ({stats.completed.toLocaleString('id-ID')})</button><button className={filter === 'review' ? 'active' : ''} onClick={() => setFilter('review')}>Perlu Ditinjau ({stats.needsReview.toLocaleString('id-ID')})</button></div>
        <div className="dash-tools"><label className="dash-date-select"><Icon name="calendar"/><select value={dateRange} onChange={e => setDateRange(e.target.value)} aria-label="Rentang tanggal"><option value="7">7 Hari Terakhir</option><option value="30">30 Hari Terakhir</option><option value="all">Semua Waktu</option></select></label><label className="dash-search"><Icon name="search"/><input placeholder="Cari transaksi…" value={search} onChange={e => setSearch(e.target.value)}/></label></div></div>
      <div className="dash-table-scroll"><table className="dash-transactions-table"><thead><tr><th><input type="checkbox" checked={allSelected} onChange={toggleAll} aria-label="Pilih semua transaksi"/></th><th>Waktu Email</th><th>Nominal</th><th>Penerima</th><th>Catatan Transfer</th><th>Status</th><th></th></tr></thead><tbody>
        {filteredRows.length ? filteredRows.map(row => <tr key={row.id} className={normalizeStatus(row.status) === 'needs_review' ? 'needs-review-row' : ''}><td><input type="checkbox" checked={selected.includes(row.id)} disabled={!['completed','needs_review'].includes(normalizeStatus(row.status))} onChange={() => setSelected(prev => prev.includes(row.id) ? prev.filter(id => id !== row.id) : [...prev, row.id])}/></td><td>{dateTime(row.transactionDate)}</td><td className="dash-amount">{money(row.amount)}</td><td>{row.recipientName || '—'}</td><td className={normalizeStatus(row.status) === 'needs_review' && !row.transferNote ? 'dash-note-review' : ''}>{row.transferNote || (normalizeStatus(row.status) === 'needs_review' ? '(Catatan belum jelas)' : '—')}</td><td><span className={`dash-status ${statusClass(row.status)}`}>{normalizeStatus(row.status) === 'completed' ? <Icon name="check"/> : normalizeStatus(row.status) === 'needs_review' ? <Icon name="warning"/> : null}{statusLabel(row.status)}</span></td><td className="dash-row-actions"><button title="Buka transaksi" onClick={onOpenTransactions}><Icon name="edit"/></button><button title="Pilihan lainnya" onClick={onOpenTransactions}><Icon name="more"/></button></td></tr>) : <tr><td colSpan={7} className="dash-empty">Tidak ada transaksi yang cocok.</td></tr>}
      </tbody></table></div>
      <div className="dash-table-footer"><span>Menampilkan {filteredRows.length} transaksi terbaru</span><div className="dash-pagination"><button disabled>‹</button><button className="current">1</button><span>Data terbaru</span><button onClick={onOpenTransactions}>Lihat semua →</button></div></div>
    </section>
    <section className="dash-bottom-actions"><span>{selected.length} transaksi dipilih</span><div><button className="dash-select-all" onClick={toggleAll}>{allSelected ? 'Batalkan Pilihan' : 'Pilih Semua'}</button><button className="dash-mark-completed" disabled>✓ Tandai Selesai</button><button className="dash-mark-review" disabled>△ Tandai Perlu Ditinjau</button><button className="dash-copy" onClick={() => void copySelected()}><Icon name="copy"/> Salin ke Clipboard <span className="dash-copy-chevron">⌄</span></button></div></section>
    <button className="dash-run-extraction" onClick={() => void processQueue()} disabled={busy}>{busy ? 'Sedang memproses…' : '▶  Jalankan Ekstraksi'}</button>
  </section>;
}
