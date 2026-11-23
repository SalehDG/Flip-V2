import { useCallback, useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { transactionService } from '../services/transactionService';
import { tauriBridge } from '../../../services/tauri/bridge';
import type { ExtractionProgressEvent, ExtractionTransaction } from '../../../services/tauri/types';
import './Transactions.css';

const filters = ['Semua', 'Completed', 'Needs Review', 'Duplicate Review', 'Failed', 'Skipped', 'Detected', 'Parsed', 'OCR Processing'];
const labels: Record<string, string> = { Completed: 'Selesai', 'Needs Review': 'Perlu Ditinjau', 'Duplicate Review': 'Duplikat', Failed: 'Gagal', Skipped: 'Dilewati', Detected: 'Terdeteksi', Parsed: 'Diparsing', 'OCR Processing': 'Proses OCR' };
function formatAmount(value: number | null) { return value == null ? '—' : new Intl.NumberFormat('id-ID', { style: 'currency', currency: 'IDR', maximumFractionDigits: 0 }).format(value); }
function formatClipboardAmount(value: number | null) { return value == null ? '' : new Intl.NumberFormat('id-ID', { maximumFractionDigits: 0 }).format(value); }
function dateLabel(value: string | null) { if (!value) return '—'; const date = normalizeTransactionDate(value); return date ? date.toLocaleDateString('id-ID', { day: '2-digit', month: 'short', year: 'numeric' }) : value; }
function dateTimeLabel(value: string | null) { if (!value) return '—'; const date = new Date(value); return Number.isNaN(date.getTime()) ? value : date.toLocaleString('id-ID', { dateStyle: 'medium', timeStyle: 'short' }); }
function inputDate(value: string | null) { if (!value) return ''; if (value.includes('/')) { const [d,m,y] = value.split('/'); return `${y}-${m}-${d}`; } return value.slice(0,10); }
function exportDate(value: string | null) { if (!value) return ''; const date = normalizeTransactionDate(value); return date ? `${String(date.getDate()).padStart(2, '0')}/${String(date.getMonth() + 1).padStart(2, '0')}/${date.getFullYear()}` : value; }
function statusClass(status: string) { return status.toLowerCase().replaceAll(' ', '-'); }
function formatEmailBody(body: string) { const decoder = document.createElement('textarea'); decoder.innerHTML = body; const decoded = decoder.value.replace(/\u200b/g, '').replace(/\u00a0/g, ' '); return decoded.replace(/\r\n?/g, '\n').split('\n').map(line => line.replace(/[\t ]+/g, ' ').trim()).filter((line, index, lines) => line !== '' || (index > 0 && lines[index - 1] !== '')).join('\n').replace(/\n{3,}/g, '\n\n').trim(); }
function normalizeTransactionDate(value: string | null): Date | null {
  if (!value) return null;
  const raw = value.trim().slice(0, 10);
  // Support ISO (YYYY-MM-DD) and localized (DD/MM/YYYY) dates from existing records.
  const iso = raw.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  const localized = raw.match(/^(\d{2})[\/.](\d{2})[\/.](\d{4})$/);
  const parts = iso ? [Number(iso[1]), Number(iso[2]), Number(iso[3])] : localized ? [Number(localized[3]), Number(localized[2]), Number(localized[1])] : null;
  if (!parts) return null;
  const [year, month, day] = parts;
  const date = new Date(year, month - 1, day);
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day ? date : null;
}
function dateMatches(value: string | null, preset: string, customStart: string, customEnd: string) {
  if (preset === 'all') return true;
  const transactionDay = normalizeTransactionDate(value);
  if (!transactionDay) return false;
  transactionDay.setHours(0, 0, 0, 0);
  const today = new Date(); today.setHours(0, 0, 0, 0);
  if (preset === 'today') return transactionDay.getTime() === today.getTime();
  if (preset === 'yesterday') { const yesterday = new Date(today); yesterday.setDate(yesterday.getDate() - 1); return transactionDay.getTime() === yesterday.getTime(); }
  if (preset === 'week') { const start = new Date(today); start.setDate(start.getDate() - ((start.getDay() + 6) % 7)); return transactionDay >= start && transactionDay <= today; }
  if (preset === 'custom') {
    const start = customStart ? normalizeTransactionDate(customStart) : null;
    const end = customEnd ? normalizeTransactionDate(customEnd) : null;
    if (start && transactionDay < start) return false;
    if (end && transactionDay > end) return false;
    return true;
  }
  return true;
}

export default function Transactions() {
  const [rows, setRows] = useState<ExtractionTransaction[]>([]);
  const [filter, setFilter] = useState('Semua'); const [dateFilter, setDateFilter] = useState('all'); const [customStart, setCustomStart] = useState(''); const [customEnd, setCustomEnd] = useState(''); const [accountFilter, setAccountFilter] = useState('all'); const [accountMenuOpen, setAccountMenuOpen] = useState(false); const [subPage, setSubPage] = useState<'all' | 'latest'>('all'); const [search, setSearch] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null); const [selectedIds, setSelectedIds] = useState<number[]>([]); const [detailOpen, setDetailOpen] = useState(true);
  const [page, setPage] = useState(1); const pageSize = 8;
  const [loading, setLoading] = useState(false); const [error, setError] = useState(''); const [savingId, setSavingId] = useState<number | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null); const [noteDraft, setNoteDraft] = useState('');
  const [reviewRow, setReviewRow] = useState<ExtractionTransaction | null>(null);
  const [reviewDraft, setReviewDraft] = useState({ date: '', recipient: '', transactionId: '', amount: '', note: '' });
  const [messageOpen, setMessageOpen] = useState(true); const [invoiceOpen, setInvoiceOpen] = useState(true); const [messageBody, setMessageBody] = useState(''); const [messageLoading, setMessageLoading] = useState(false); const [messageError, setMessageError] = useState(''); const [copyNotice, setCopyNotice] = useState('');
  const load = useCallback(async () => { setLoading(true); setError(''); try { setRows(await transactionService.list({ limit: 250, offset: 0, status: null })); } catch (e) { setError(String(e)); } finally { setLoading(false); } }, []);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { let unlisten: (() => void) | undefined; void listen<ExtractionProgressEvent>('flip://extraction-progress', () => { void load(); }).then(fn => { unlisten = fn; }); return () => unlisten?.(); }, [load]);
  useEffect(() => { if (!reviewRow || !messageOpen) return; let cancelled = false; setMessageBody(''); setMessageError(''); setMessageLoading(true); invoke<string>('get_gmail_message_body', { accountId: reviewRow.gmailAccountId, messageId: reviewRow.gmailMessageId }).then(body => { if (!cancelled) setMessageBody(body); }).catch(error => { if (!cancelled) setMessageError(String(error)); }).finally(() => { if (!cancelled) setMessageLoading(false); }); return () => { cancelled = true; }; }, [reviewRow, messageOpen]);
  const accounts = useMemo(() => Array.from(new Set(rows.map(r => r.accountEmail).filter(Boolean))).sort(), [rows]);
  const counts = useMemo(() => Object.fromEntries(filters.slice(1).map(status => [status, rows.filter(r => r.status === status).length])), [rows]);
  const filteredRows = useMemo(() => rows.filter(r => (filter === 'Semua' || r.status === filter) && (accountFilter === 'all' || r.accountEmail === accountFilter) && dateMatches(r.transactionDate, dateFilter, customStart, customEnd) && (subPage !== 'latest' || !(r.firstCopiedAt || r.lastCopiedAt)) && (!search.trim() || [r.recipientName, r.transferNote, r.amount == null ? '' : String(r.amount), r.flipTransactionId, r.accountEmail].some(v => String(v ?? '').toLowerCase().includes(search.trim().toLowerCase())))), [rows, filter, accountFilter, dateFilter, customStart, customEnd, subPage, search]);
  const pageCount = Math.max(1, Math.ceil(filteredRows.length / pageSize)); const pageRows = filteredRows.slice((page - 1) * pageSize, page * pageSize); const selectedRow = rows.find(r => r.id === selectedId) ?? null;
  useEffect(() => { setPage(1); }, [filter, dateFilter, customStart, customEnd, accountFilter, subPage, search]);
  async function retry(id: number) { setSavingId(id); setError(''); try { await transactionService.retry(id); await load(); } catch (e) { setError(String(e)); } finally { setSavingId(null); } }
  function beginEdit(row: ExtractionTransaction) { setEditingId(row.id); setNoteDraft(row.transferNote ?? ''); }
  function openReview(row: ExtractionTransaction) { setMessageOpen(true); setInvoiceOpen(true); setReviewRow(row); setReviewDraft({ date: inputDate(row.transactionDate), recipient: row.recipientName ?? '', transactionId: row.flipTransactionId ?? '', amount: row.amount == null ? '' : String(row.amount), note: row.transferNote ?? '' }); }
  async function saveNote(id: number) { setSavingId(id); setError(''); try { await transactionService.correctNote(id, noteDraft.trim()); setEditingId(null); await load(); } catch (e) { setError(String(e)); } finally { setSavingId(null); } }
  async function saveReviewCorrection() { if (!reviewRow) return; const amount = Number(reviewDraft.amount); if (!reviewDraft.date || !reviewDraft.recipient.trim() || !reviewDraft.transactionId.trim() || !Number.isSafeInteger(amount) || amount <= 0) { setError('Tanggal, nama, Transaction ID, dan nominal wajib valid.'); return; } setSavingId(reviewRow.id); setError(''); try { const status = await transactionService.saveCorrection({ transactionId: reviewRow.id, transactionDate: reviewDraft.date, recipientName: reviewDraft.recipient.trim(), flipTransactionId: reviewDraft.transactionId.trim(), amount, transferNote: reviewDraft.note.trim() }); setReviewRow(null); await load(); setCopyNotice(status === 'Completed' ? 'Koreksi tersimpan. Transaksi berstatus Completed.' : 'Koreksi tersimpan, tetapi Transaction ID terdeteksi duplikat.'); } catch (e) { setError(String(e)); } finally { setSavingId(null); } }
  function toggleSelection(id: number) { setSelectedIds(prev => prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]); }
  function toggleVisible() { const ids = pageRows.map(r => r.id); const all = ids.length > 0 && ids.every(id => selectedIds.includes(id)); setSelectedIds(prev => all ? prev.filter(id => !ids.includes(id)) : [...new Set([...prev, ...ids])]); }
  async function copyRows(sourceRows: ExtractionTransaction[], actionLabel: string) {
    // Prevent re-copying transactions that were already exported to clipboard before.
    const eligible = sourceRows.filter(r => !['Failed', 'Duplicate Review'].includes(r.status) && !(r.firstCopiedAt || r.lastCopiedAt));
    const skippedCopiedCount = sourceRows.filter(r => r.firstCopiedAt || r.lastCopiedAt).length;
    const reviewCount = eligible.filter(r => r.status === 'Needs Review').length;
    if (reviewCount && !window.confirm(`Ada ${reviewCount} transaksi yang perlu ditinjau. Tetap salin?`)) return;
    if (!eligible.length) {
      setCopyNotice(skippedCopiedCount ? 'Semua transaksi pada pilihan ini sudah pernah di-copy.' : 'Tidak ada transaksi yang bisa disalin pada pilihan ini.');
      return;
    }
    const text = eligible.map(r => [exportDate(r.transactionDate), r.transferNote ?? '', r.recipientName ?? '', '', formatClipboardAmount(r.amount), '', ''].join('\t')).join('\n');
    try {
      await navigator.clipboard.writeText(text);
      try {
        await tauriBridge.markTransactionsCopied(eligible.map(row => row.id));
        const skippedMessage = skippedCopiedCount ? ` (${skippedCopiedCount} transaksi yang sudah pernah di-copy dilewati)` : '';
        setCopyNotice(`${eligible.length} transaksi ${actionLabel} berhasil disalin${skippedMessage}. Tracking tersimpan.`);
        await load();
      } catch (trackingError) { setCopyNotice(`${eligible.length} transaksi disalin, tetapi tracking gagal disimpan: ${String(trackingError)}`); }
    } catch { setCopyNotice('Clipboard tidak dapat diakses.'); }
  }
  async function copySelected() { await copyRows(rows.filter(r => selectedIds.includes(r.id)), 'terpilih'); }
  async function copyAllFiltered() { await copyRows(filteredRows, 'dari filter aktif'); }
  function accountInitials(email: string) { return email.split('@')[0].split(/[._-]/).filter(Boolean).slice(0, 2).map(part => part[0]?.toUpperCase() ?? '').join('') || 'G'; }
  return <section className="transactions-page transactions-workspace">
    <header className="tx-page-header"><div><h2>Transaksi</h2><p>Kelola dan proses hasil ekstraksi email transaksi Flip.</p></div><div className="tx-header-actions"><button className="subtle tx-detail-toggle" onClick={() => setDetailOpen((open) => !open)} aria-expanded={detailOpen}><span>{detailOpen ? '▣' : '▤'}</span>{detailOpen ? 'Sembunyikan Detail' : 'Tampilkan Detail'}</button><button className="subtle tx-refresh" onClick={() => void load()} disabled={loading}><span>↻</span>{loading ? 'Memuat…' : 'Refresh'}</button></div></header>
    {error && <div className="error-banner" role="alert">{error}</div>}
    <div className="tx-status-tabs" role="tablist" aria-label="Filter transaksi">
      <button role="tab" aria-selected={subPage === 'latest'} className={`tx-latest-tab ${subPage === 'latest' ? 'active' : ''}`} onClick={() => { setSubPage('latest'); setFilter('Semua'); }}><span className="tx-tab-icon latest-icon">✦</span>Terbaru<span className="tx-tab-count new-count">{rows.filter(r => !(r.firstCopiedAt || r.lastCopiedAt)).length.toLocaleString('id-ID')}</span></button>
      {filters.slice(0,5).map(status => <button key={status} role="tab" aria-selected={subPage === 'all' && filter === status} className={subPage === 'all' && filter === status ? 'active' : ''} onClick={() => { setSubPage('all'); setFilter(status); }}><span className={`tx-tab-icon ${statusClass(status)}`}>{status === 'Semua' ? '▱' : status === 'Completed' ? '✓' : status === 'Needs Review' ? '△' : status === 'Duplicate Review' ? '▢' : '⊗'}</span>{labels[status] ?? status}<span className="tx-tab-count">{status === 'Semua' ? rows.length.toLocaleString('id-ID') : (counts[status] ?? 0).toLocaleString('id-ID')}</span></button>)}
    </div>
    <div className="tx-toolbar">
      <div className="tx-date-filter"><span className="tx-filter-caption">Rentang tanggal</span><select className="tx-date-select" value={dateFilter} onChange={e => setDateFilter(e.target.value)} aria-label="Filter rentang tanggal"><option value="all">Semua tanggal</option><option value="today">Hari ini</option><option value="yesterday">Kemarin</option><option value="week">Minggu ini</option><option value="custom">Custom</option></select></div>
      {dateFilter === 'custom' && <div className="tx-custom-range"><label>Dari<input type="date" value={customStart} onChange={e => setCustomStart(e.target.value)} /></label><label>Sampai<input type="date" value={customEnd} onChange={e => setCustomEnd(e.target.value)} /></label></div>}
      <div className="tx-account-dropdown"><span className="tx-filter-caption">Akun Gmail</span><button type="button" className="tx-account-trigger" onClick={() => setAccountMenuOpen(open => !open)} aria-expanded={accountMenuOpen}><span className="tx-account-avatar">{accountFilter === 'all' ? '✉' : accountInitials(accountFilter)}</span><span className="tx-account-trigger-label">{accountFilter === 'all' ? 'Semua akun' : accountFilter}</span><span className="tx-dropdown-chevron">⌄</span></button>
        {accountMenuOpen && <div className="tx-account-menu"><button type="button" className={accountFilter === 'all' ? 'selected' : ''} onClick={() => { setAccountFilter('all'); setAccountMenuOpen(false); }}><span className="tx-account-avatar all-avatar">✉</span><span className="tx-account-option-text"><strong>Semua akun</strong><small>Gabungan transaksi</small></span>{accountFilter === 'all' && <span className="account-check">✓</span>}</button>{accounts.map(email => <button type="button" key={email} className={accountFilter === email ? 'selected' : ''} onClick={() => { setAccountFilter(email); setAccountMenuOpen(false); }}><span className="tx-account-avatar">{accountInitials(email)}</span><span className="tx-account-option-text"><strong>{email}</strong><small>Akun Gmail terhubung</small></span>{accountFilter === email && <span className="account-check">✓</span>}</button>)}</div>}
      </div>
      <label className="tx-search"><span>⌕</span><input value={search} onChange={e => setSearch(e.target.value)} placeholder="Cari penerima, catatan transfer, atau nominal…"/></label>
      <button className="primary tx-copy-all" onClick={() => void copyAllFiltered()} disabled={!filteredRows.some(r => !['Failed', 'Duplicate Review'].includes(r.status))}>▣ Copy semua <small>{filteredRows.filter(r => !['Failed', 'Duplicate Review'].includes(r.status)).length}</small></button>
    </div>
    <div className={`tx-workspace-grid ${detailOpen ? '' : 'detail-closed'}`}><div className="tx-main-panel"><div className="tx-table-scroll"><table className="tx-table"><thead><tr><th className="tx-check-col"><input type="checkbox" checked={pageRows.length > 0 && pageRows.every(r => selectedIds.includes(r.id))} onChange={toggleVisible} aria-label="Pilih semua di halaman"/></th><th>Tanggal</th><th>Penerima</th><th>Nominal</th><th>Catatan Transfer</th><th>Status</th><th>Aksi</th></tr></thead><tbody>
    {pageRows.map(row => <tr key={row.id} className={`${selectedId === row.id ? 'selected' : ''} ${row.status === 'Needs Review' ? 'needs-review-row' : row.status === 'Failed' ? 'failed-row' : ''}`} onClick={() => { setSelectedId(row.id); setDetailOpen(true); }}><td className="tx-check-col" onClick={e => e.stopPropagation()}><input type="checkbox" checked={selectedIds.includes(row.id)} onChange={() => toggleSelection(row.id)} aria-label={`Pilih transaksi ${row.id}`}/></td><td className="tx-date-cell">{dateLabel(row.transactionDate)}<small>{dateTimeLabel(row.createdAt)}</small></td><td className="tx-recipient-cell"><strong>{row.recipientName || '—'}</strong></td><td className="tx-amount-cell">{formatAmount(row.amount)}</td><td className="tx-note-cell">{editingId === row.id ? <div className="tx-note-edit" onClick={e => e.stopPropagation()}><textarea value={noteDraft} onChange={e => setNoteDraft(e.target.value)} rows={2}/><div><button className="primary small" onClick={() => void saveNote(row.id)} disabled={savingId === row.id}>Simpan</button><button className="subtle small" onClick={() => setEditingId(null)}>Batal</button></div></div> : <><span>{row.transferNote || '—'}</span>{row.status === 'Needs Review' && <small className="tx-note-warning">Catatan perlu diperiksa</small>}</>}</td><td><span className={`status-chip ${statusClass(row.status)}`}>{labels[row.status] ?? row.status}</span></td><td className="tx-row-actions" onClick={e => e.stopPropagation()}>{row.status === 'Needs Review' ? <button className="primary small" onClick={() => openReview(row)}>Review</button> : <button className="tx-icon-btn" title="Edit catatan" onClick={() => beginEdit(row)}>✎</button>}{['Failed','Needs Review','Duplicate Review'].includes(row.status) && <button className="tx-icon-btn" title="Coba ulang" onClick={() => void retry(row.id)} disabled={savingId === row.id}>{savingId === row.id ? '…' : '↻'}</button>}<button className="tx-icon-btn" title="Detail transaksi" onClick={() => { setSelectedId(row.id); setDetailOpen(true); }}>⋮</button></td></tr>)}
    {!pageRows.length && <tr><td colSpan={7} className="empty-cell">{loading ? 'Memuat transaksi…' : 'Tidak ada transaksi yang cocok dengan filter.'}</td></tr>}
    </tbody></table></div><footer className="tx-pagination"><span>Menampilkan {filteredRows.length ? (page-1)*pageSize+1 : 0}–{Math.min(page*pageSize,filteredRows.length)} dari {filteredRows.length.toLocaleString('id-ID')} transaksi <small>· Maks. 250 data dimuat</small></span><div><button className="tx-page-btn" disabled={page<=1} onClick={() => setPage(p=>p-1)}>‹</button><span className="tx-page-current">{page}</span><span> / {pageCount}</span><button className="tx-page-btn" disabled={page>=pageCount} onClick={() => setPage(p=>p+1)}>›</button></div></footer></div>
    {detailOpen && <aside className={`tx-detail-panel ${selectedRow ? '' : 'empty'}`}><div className="tx-detail-header"><div><h3>Detail Transaksi</h3><p>{selectedRow ? 'Informasi transaksi terpilih' : 'Pilih transaksi untuk melihat detail'}</p></div>{selectedRow && <button className="tx-close-detail" onClick={() => setDetailOpen(false)} aria-label="Tutup detail">×</button>}</div>
    {selectedRow ? <><div className="tx-detail-status-line"><span className={`status-chip ${statusClass(selectedRow.status)}`}>{labels[selectedRow.status] ?? selectedRow.status}</span><span>{dateTimeLabel(selectedRow.createdAt)}</span></div><div className="tx-detail-fields"><div><label>Penerima</label><strong>{selectedRow.recipientName || 'Belum terbaca'}</strong></div><div><label>Nominal</label><strong className="tx-detail-amount">{formatAmount(selectedRow.amount)}</strong></div><div><label>Catatan Transfer</label><p>{selectedRow.transferNote || 'Belum ada catatan'}</p></div><div><label>Akun Gmail</label><p>{selectedRow.accountEmail || '—'}</p></div><div><label>Flip Transaction ID</label><p className="tx-detail-id">{selectedRow.flipTransactionId || 'Tidak tersedia'}</p></div><div><label>Alasan / Error</label><p className="tx-detail-reason">{selectedRow.reviewReason || selectedRow.lastError || 'Tidak ada catatan masalah.'}</p></div></div><div className="tx-detail-invoice"><div className="tx-detail-section-title"><strong>Lampiran Invoice</strong><span>{selectedRow.pngPath ? 'PNG' : 'Tidak tersedia'}</span></div>{selectedRow.pngPath ? <div className="tx-invoice-preview"><img src={convertFileSrc(selectedRow.pngPath)} alt="Lampiran invoice transaksi"/></div> : <div className="tx-invoice-empty">Lampiran PNG tidak tersedia untuk transaksi ini.</div>}</div><div className="tx-detail-actions">{selectedRow.status === 'Needs Review' && <button className="primary" onClick={() => openReview(selectedRow)}>Review & Edit</button>}<button className="subtle" onClick={() => openReview(selectedRow)}>Lihat Email Sumber</button><button className="subtle" onClick={() => beginEdit(selectedRow)}>Edit Catatan</button>{['Failed','Needs Review','Duplicate Review'].includes(selectedRow.status) && <button className="subtle" onClick={() => void retry(selectedRow.id)} disabled={savingId===selectedRow.id}>{savingId===selectedRow.id ? 'Mencoba…' : 'Coba Ulang'}</button>}</div></> : <div className="tx-detail-empty-state"><span>⌁</span><strong>Belum ada transaksi dipilih</strong><p>Klik salah satu baris untuk melihat informasi lengkap dan lampiran.</p></div>}</aside>} </div>
    {selectedIds.length>0 && <div className="tx-bulk-bar"><strong>{selectedIds.length} transaksi dipilih</strong><button className="subtle" onClick={() => setSelectedIds([])}>Batal Pilih</button><button className="primary" onClick={() => void copySelected()}>▣ Copy yang dipilih</button></div>}{copyNotice && <div className="tx-copy-notice" role="status">{copyNotice}<button onClick={() => setCopyNotice('')}>×</button></div>}<p className="tx-footnote">{subPage === 'latest' ? 'Terbaru menampilkan transaksi yang belum pernah di-copy.' : 'Data transaksi lokal · Catatan OCR dapat dikoreksi · Transaksi bermasalah dapat ditinjau ulang.'}</p>
      {reviewRow && (
        <>
        <style>{`
          .review-overlay{position:fixed;inset:0;z-index:1000;background:rgba(12,24,44,.48);backdrop-filter:blur(5px);display:flex;align-items:center;justify-content:center;padding:24px}
          .review-modal{width:min(1180px,96vw);height:min(820px,92vh);background:#f7faff;border:1px solid #dce7f3;border-radius:20px;box-shadow:0 24px 80px rgba(10,35,70,.25);display:flex;flex-direction:column;overflow:hidden;color:#172b45}
          .review-header{display:flex;justify-content:space-between;align-items:center;padding:20px 24px;border-bottom:1px solid #e0eaf4;background:white}
          .review-header h3{margin:4px 0;font-size:22px}.review-header p{margin:0;color:#718198;font-size:13px}.review-kicker{font-size:10px;font-weight:800;letter-spacing:1.5px;color:#2f78d0}
          .review-close{width:38px;height:38px;border:1px solid #dbe5ef;border-radius:11px;background:#f7faff;font-size:25px;color:#50657e;cursor:pointer}
          .review-content{display:grid;grid-template-columns:minmax(330px,430px) minmax(0,1fr);gap:18px;padding:18px;flex:1;min-height:0}
          .review-source-column{display:flex;flex-direction:column;gap:12px;min-width:0;min-height:0}
          .source-panel{background:white;border:1px solid #e0e9f3;border-radius:15px;overflow:hidden;display:flex;flex-direction:column;min-width:0}
          .source-panel.message-panel{flex:0 0 auto}
          .source-panel.invoice-panel{flex:1;min-height:0}
          .source-panel-toggle{width:100%;display:flex;align-items:center;justify-content:space-between;gap:12px;padding:13px 15px;border:0;background:white;color:#23374e;text-align:left;cursor:pointer}
          .source-panel-toggle strong{font-size:13px}
          .source-panel-toggle span{font-size:11px;color:#8190a4}
          .source-panel-content{padding:14px;border-top:1px solid #e8eef5}
          .message-meta{display:grid;gap:8px;font-size:12px;color:#64778e}
          .message-meta strong{color:#354a63;overflow-wrap:anywhere}
          .message-unavailable{font-size:12px;line-height:1.55;color:#7c8da2;background:#f5f8fc;border:1px dashed #d6e2ef;border-radius:10px;padding:12px;margin-top:12px}
          .message-body{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;line-height:1.6;color:#344b65;background:#f8fbff;border:1px solid #e1eaf4;border-radius:10px;padding:12px;margin-top:12px;max-height:360px;overflow:auto}
          .message-loading,.message-error{font-size:12px;line-height:1.55;padding:12px;margin-top:12px;border-radius:10px}
          .message-loading{color:#718198;background:#f5f8fc;border:1px dashed #d6e2ef}
          .message-error{color:#9a4e43;background:#fff1ef;border:1px solid #f2d4ce;overflow-wrap:anywhere}
          .message-reason{font-size:12px;color:#a36c18;background:#fff8e9;border-radius:8px;padding:9px 10px;margin-top:10px}
          .source-panel .invoice-canvas{min-height:220px}
          .review-form{background:white;border:1px solid #e0e9f3;border-radius:15px;padding:18px;overflow:auto;display:flex;flex-direction:column;gap:13px}
          .review-form label{display:flex;flex-direction:column;gap:6px;font-size:12px;font-weight:700;color:#516780}
          .review-form input,.review-form textarea{font:inherit;font-size:14px;font-weight:500;color:#172b45;background:#fbfdff;border:1px solid #dce6f0;border-radius:9px;padding:10px 11px;outline:none;width:100%;box-sizing:border-box}
          .review-form input:focus,.review-form textarea:focus{border-color:#6ba8ec;box-shadow:0 0 0 3px rgba(75,145,220,.12)}.review-form textarea{resize:vertical}
          .review-save-notice{font-size:12px;line-height:1.5;color:#76591d;background:#fff8e6;border:1px solid #f1dfae;padding:10px;border-radius:10px}
          .review-save-disabled{opacity:.55;cursor:not-allowed;justify-content:center;width:100%}
          .invoice-panel{display:flex;flex-direction:column;min-width:0;background:white;border:1px solid #e0e9f3;border-radius:15px;overflow:hidden}
          .invoice-toolbar{height:46px;display:flex;align-items:center;justify-content:space-between;padding:0 14px;border-bottom:1px solid #e5edf5;font-size:13px}.invoice-toolbar span{font-size:11px;color:#8291a5}
          .invoice-canvas{flex:1;min-height:0;overflow:auto;background:#edf3f9;display:flex;align-items:flex-start;justify-content:center;padding:16px}
          .invoice-canvas img{max-width:100%;height:auto;object-fit:contain;box-shadow:0 5px 20px rgba(24,45,70,.14);background:white;border-radius:5px}
          .invoice-empty{margin:auto;text-align:center;color:#8291a5}.invoice-empty span{font-size:40px}.invoice-empty strong{display:block;color:#536984;margin:8px}.invoice-empty p{font-size:12px}
          @media(max-width:760px){.review-overlay{padding:8px}.review-modal{height:96vh;width:100%;border-radius:14px}.review-header{padding:14px}.review-content{grid-template-columns:1fr;grid-template-rows:auto auto;overflow:auto;padding:10px;gap:10px}.review-form{overflow:visible}.review-source-column{min-height:0}.source-panel.invoice-panel{min-height:320px}.invoice-canvas img{max-height:none}}
        `}</style>
        <div className="review-overlay" role="dialog" aria-modal="true" aria-label="Review transaksi">
          <div className="review-modal">
            <header className="review-header">
              <div><span className="review-kicker">MANUAL REVIEW</span><h3>Periksa transaksi</h3><p>Bandingkan hasil ekstraksi dengan invoice asli.</p></div>
              <button className="review-close" onClick={() => setReviewRow(null)} aria-label="Tutup">×</button>
            </header>
            <div className="review-content">
              <form className="review-form" onSubmit={e => e.preventDefault()}>
                <label>Tanggal transaksi<input type="date" value={reviewDraft.date} onChange={e => setReviewDraft(d => ({ ...d, date: e.target.value }))}/></label>
                <label>Nama penerima<input value={reviewDraft.recipient} onChange={e => setReviewDraft(d => ({ ...d, recipient: e.target.value }))} placeholder="Nama penerima"/></label>
                <label>Flip Transaction ID<input value={reviewDraft.transactionId} onChange={e => setReviewDraft(d => ({ ...d, transactionId: e.target.value }))} placeholder="Transaction ID"/></label>
                <label>Nominal (Rp)<input inputMode="numeric" value={reviewDraft.amount} onChange={e => setReviewDraft(d => ({ ...d, amount: e.target.value.replace(/[^0-9]/g, '') }))} placeholder="Nominal transfer"/></label>
                <label>Catatan transfer / Uraian<textarea rows={4} value={reviewDraft.note} onChange={e => setReviewDraft(d => ({ ...d, note: e.target.value }))} placeholder="Uraian transfer"/></label>
                <div className="review-save-notice">Semua perubahan akan dicatat ke riwayat koreksi. Transaction ID duplikat tetap ditandai untuk review.</div>
                <button type="button" className="primary" disabled={savingId === reviewRow.id} onClick={() => void saveReviewCorrection()}>{savingId === reviewRow.id ? 'Menyimpan…' : 'Simpan Koreksi'}</button>
              </form>
              <div className="review-source-column">
                <section className="source-panel message-panel">
                  <button className="source-panel-toggle" type="button" onClick={() => setMessageOpen(open => !open)} aria-expanded={messageOpen}>
                    <strong>Pesan</strong><span>{messageOpen ? 'Tutup ▲' : 'Buka ▼'}</span>
                  </button>
                  {messageOpen && <div className="source-panel-content">
                    <div className="message-meta">
                      <div>Akun Gmail<strong>{reviewRow.accountEmail || '—'}</strong></div>
                      <div>Gmail Message ID<strong>{reviewRow.gmailMessageId || '—'}</strong></div>
                    </div>
                    {messageLoading && <div className="message-loading">Memuat isi pesan email…</div>}
                    {!messageLoading && messageError && <div className="message-error">Gagal mengambil pesan: {messageError}</div>}
                    {!messageLoading && !messageError && messageBody && <div className="message-body">{formatEmailBody(messageBody)}</div>}
                    {!messageLoading && !messageError && !messageBody && <div className="message-unavailable">Body email kosong atau tidak tersedia.</div>}
                    {reviewRow.reviewReason && <div className="message-reason"><strong>Alasan review:</strong> {reviewRow.reviewReason}</div>}
                    {reviewRow.lastError && <div className="message-reason"><strong>Error terakhir:</strong> {reviewRow.lastError}</div>}
                  </div>}
                </section>
                <section className="source-panel invoice-panel">
                  <button className="source-panel-toggle" type="button" onClick={() => setInvoiceOpen(open => !open)} aria-expanded={invoiceOpen}>
                    <strong>Invoice</strong><span>{reviewRow.pngPath ? 'PNG attachment' : 'Tidak tersedia'} · {invoiceOpen ? 'Tutup ▲' : 'Buka ▼'}</span>
                  </button>
                  {invoiceOpen && <div className="invoice-canvas">
                    {reviewRow.pngPath ? <img src={convertFileSrc(reviewRow.pngPath)} alt="Invoice transaksi"/> : <div className="invoice-empty"><span>▧</span><strong>Invoice tidak ditemukan</strong><p>File invoice belum tersimpan untuk transaksi ini.</p></div>}
                  </div>}
                </section>
              </div>
            </div>
          </div>
        </div>
        </>
      )}
    </section>
  ;
}
