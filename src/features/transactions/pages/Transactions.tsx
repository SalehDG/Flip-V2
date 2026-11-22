import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { transactionService } from '../services/transactionService';
import type { ExtractionProgressEvent, ExtractionTransaction } from '../../../services/tauri/types';

const filters = ['Semua', 'Completed', 'Needs Review', 'Duplicate Review', 'Failed', 'Detected', 'Parsed', 'OCR Processing'];
function formatAmount(value: number | null) {
  return value == null ? '—' : new Intl.NumberFormat('id-ID', { style: 'currency', currency: 'IDR', maximumFractionDigits: 0 }).format(value);
}
function dateLabel(value: string | null) {
  if (!value) return '—';
  const date = new Date(`${value}T00:00:00`);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString('id-ID');
}

export default function Transactions() {
  const [rows, setRows] = useState<ExtractionTransaction[]>([]);
  const [filter, setFilter] = useState('Semua');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [savingId, setSavingId] = useState<number | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [noteDraft, setNoteDraft] = useState('');

  const load = useCallback(async () => {
    setLoading(true); setError('');
    try { setRows(await transactionService.list({ limit: 250, offset: 0, status: filter === 'Semua' ? null : filter })); }
    catch (e) { setError(String(e)); }
    finally { setLoading(false); }
  }, [filter]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<ExtractionProgressEvent>('flip://extraction-progress', () => { void load(); }).then(fn => { unlisten = fn; });
    return () => unlisten?.();
  }, [load]);

  async function retry(id: number) {
    setSavingId(id); setError('');
    try { await transactionService.retry(id); await load(); }
    catch (e) { setError(String(e)); }
    finally { setSavingId(null); }
  }
  function beginEdit(row: ExtractionTransaction) { setEditingId(row.id); setNoteDraft(row.transferNote ?? ''); }
  async function saveNote(id: number) {
    setSavingId(id); setError('');
    try { await transactionService.correctNote(id, noteDraft.trim()); setEditingId(null); await load(); }
    catch (e) { setError(String(e)); }
    finally { setSavingId(null); }
  }

  return (
    <section className="transactions-page">
      <div className="page-heading">
        <div><h2>Transaksi</h2><p>Data hasil ekstraksi email dan OCR.</p></div>
        <button className="subtle" onClick={() => void load()} disabled={loading}>{loading ? 'Memuat…' : '↻ Refresh'}</button>
      </div>
      {error && <div className="error-banner" role="alert">{error}</div>}
      <div className="filter-bar"><label htmlFor="status-filter">Status</label><select id="status-filter" value={filter} onChange={e => setFilter(e.target.value)}>{filters.map(f => <option key={f} value={f}>{f}</option>)}</select><span className="muted">{rows.length} data ditampilkan</span></div>
      <div className="transaction-table-wrap">
        <table className="transaction-table">
          <thead><tr><th>Tanggal</th><th>Nama Tujuan</th><th>Nominal</th><th>Catatan Transfer (OCR)</th><th>Status</th><th>Aksi</th></tr></thead>
          <tbody>
            {rows.map(row => (
              <tr key={row.id}>
                <td>{dateLabel(row.transactionDate)}</td>
                <td><strong>{row.recipientName || '—'}</strong><small>{row.accountEmail}</small></td>
                <td className="amount">{formatAmount(row.amount)}</td>
                <td className="note-cell">
                  {editingId === row.id ? (
                    <div className="note-edit"><textarea value={noteDraft} onChange={e => setNoteDraft(e.target.value)} rows={2}/><div><button className="primary small" onClick={() => void saveNote(row.id)} disabled={savingId === row.id}>Simpan</button><button className="subtle small" onClick={() => setEditingId(null)}>Batal</button></div></div>
                  ) : (
                    <><span>{row.transferNote || '—'}</span>{row.reviewReason && <small className="review-reason">{row.reviewReason}</small>}{row.lastError && <small className="error-text">{row.lastError}</small>}</>
                  )}
                </td>
                <td><span className={`status-chip ${row.status.toLowerCase().replaceAll(' ', '-')}`}>{row.status}</span></td>
                <td className="actions-cell"><button className="subtle small" onClick={() => beginEdit(row)} disabled={savingId === row.id}>Edit Catatan</button>{['Failed', 'Needs Review', 'Duplicate Review'].includes(row.status) && <button className="subtle small" onClick={() => void retry(row.id)} disabled={savingId === row.id}>{savingId === row.id ? 'Retry…' : 'Retry'}</button>}</td>
              </tr>
            ))}
            {rows.length === 0 && <tr><td colSpan={6} className="empty-cell">{loading ? 'Memuat transaksi…' : 'Belum ada data transaksi pada status ini.'}</td></tr>}
          </tbody>
        </table>
      </div>
      <p className="muted table-footnote">Hasil OCR ditampilkan apa adanya. Koreksi Catatan Transfer tersimpan ke database lokal.</p>
    </section>
  );
}
