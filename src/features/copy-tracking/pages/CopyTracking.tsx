import { useCallback, useEffect, useMemo, useState } from 'react';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { ExtractionTransaction } from '../../../services/tauri/types';
import './CopyTracking.css';

function formatDateTime(value: string | null) {
  if (!value) return '—';
  const normalized = value.includes('T') ? value : value.replace(' ', 'T') + 'Z';
  const date = new Date(normalized);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString('id-ID', { dateStyle: 'medium', timeStyle: 'short' });
}
function formatAmount(value: number | null) {
  return value == null ? '—' : new Intl.NumberFormat('id-ID', { style: 'currency', currency: 'IDR', maximumFractionDigits: 0 }).format(value);
}
export default function CopyTracking() {
  const [rows, setRows] = useState<ExtractionTransaction[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<'all' | 'copied' | 'not-copied'>('all');
  const refresh = useCallback(async () => {
    setLoading(true); setError('');
    try { setRows(await tauriBridge.getTransactions({ limit: 500, offset: 0 })); }
    catch (e) { setError(String(e)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  const eligible = useMemo(() => rows.filter(r => ['Completed', 'Needs Review'].includes(r.status)), [rows]);
  const copied = eligible.filter(r => Boolean(r.firstCopiedAt || r.lastCopiedAt));
  const filtered = useMemo(() => eligible.filter(row => {
    const isCopied = Boolean(row.firstCopiedAt || row.lastCopiedAt);
    const filterMatch = filter === 'all' || (filter === 'copied' && isCopied) || (filter === 'not-copied' && !isCopied);
    const query = search.trim().toLowerCase();
    const searchMatch = !query || [row.recipientName, row.transferNote, row.accountEmail, row.flipTransactionId, row.amount == null ? '' : String(row.amount)].some(v => String(v ?? '').toLowerCase().includes(query));
    return filterMatch && searchMatch;
  }), [eligible, filter, search]);
  return <section className="copy-tracking-page">
    <div className="page-heading"><div><h2>Copy Tracking</h2><p>Audit kapan transaksi berhasil disalin ke clipboard.</p></div><button className="subtle" onClick={() => void refresh()}>↻ Refresh</button></div>
    {error && <div className="error-banner">Gagal memuat tracking: {error}</div>}
    <div className="copy-tracking-stats">
      <article><span>Transaksi eligible</span><strong>{eligible.length}</strong><small>Completed + Needs Review</small></article>
      <article className="tracked"><span>Sudah disalin</span><strong>{copied.length}</strong><small>Tracking berhasil tersimpan</small></article>
      <article className="untracked"><span>Belum disalin</span><strong>{eligible.length - copied.length}</strong><small>Belum memiliki timestamp copy</small></article>
      <article><span>Copy berulang</span><strong>{eligible.filter(r => r.firstCopiedAt && r.lastCopiedAt && r.firstCopiedAt !== r.lastCopiedAt).length}</strong><small>First copy ≠ last copy</small></article>
    </div>
    <div className="copy-tracking-toolbar"><div className="copy-tracking-tabs"><button className={filter === 'all' ? 'active' : ''} onClick={() => setFilter('all')}>Semua ({eligible.length})</button><button className={filter === 'copied' ? 'active' : ''} onClick={() => setFilter('copied')}>Sudah disalin ({copied.length})</button><button className={filter === 'not-copied' ? 'active' : ''} onClick={() => setFilter('not-copied')}>Belum disalin ({eligible.length - copied.length})</button></div><input value={search} onChange={e => setSearch(e.target.value)} placeholder="Cari nama, catatan, email, ID, nominal…" /></div>
    <div className="copy-tracking-table-wrap"><table className="copy-tracking-table"><thead><tr><th>Status</th><th>Penerima / Catatan</th><th>Nominal</th><th>Copy Pertama</th><th>Copy Terakhir</th></tr></thead><tbody>
      {loading ? <tr><td colSpan={5} className="empty">Memuat data tracking…</td></tr> : filtered.length ? filtered.map(row => { const isCopied = Boolean(row.firstCopiedAt || row.lastCopiedAt); return <tr key={row.id}><td><span className={`copy-tracking-badge ${isCopied ? 'copied' : 'pending'}`}>{isCopied ? 'Tercatat disalin' : 'Belum disalin'}</span><small>{row.status === 'Needs Review' ? 'Perlu ditinjau' : 'Completed'}</small></td><td className="copy-tracking-recipient"><strong>{row.recipientName || 'Nama tidak tersedia'}</strong><span>{row.transferNote || 'Catatan transfer kosong'}</span><small>{row.accountEmail}</small></td><td className="copy-tracking-amount">{formatAmount(row.amount)}</td><td>{formatDateTime(row.firstCopiedAt)}</td><td>{formatDateTime(row.lastCopiedAt)}</td></tr>; }) : <tr><td colSpan={5} className="empty">Tidak ada transaksi pada filter ini.</td></tr>}
    </tbody></table></div><p className="copy-tracking-footnote">Waktu copy pertama tidak berubah saat transaksi disalin ulang. Waktu copy terakhir akan diperbarui setiap copy berhasil.</p>
  </section>;
}
