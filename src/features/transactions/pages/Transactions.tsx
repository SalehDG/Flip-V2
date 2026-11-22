import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { transactionService } from '../services/transactionService';
import type { ExtractionProgressEvent, ExtractionTransaction } from '../../../services/tauri/types';

const filters = ['Semua', 'Completed', 'Needs Review', 'Duplicate Review', 'Failed', 'Skipped', 'Detected', 'Parsed', 'OCR Processing'];
function formatAmount(value: number | null) {
  return value == null ? '—' : new Intl.NumberFormat('id-ID', { style: 'currency', currency: 'IDR', maximumFractionDigits: 0 }).format(value);
}
function dateLabel(value: string | null) {
  if (!value) return '—';
  const date = new Date(`${value}T00:00:00`);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString('id-ID');
}

function formatEmailBody(body: string) {
  // Decode HTML entities without injecting markup into the React tree.
  const decoder = document.createElement('textarea');
  decoder.innerHTML = body;
  const decoded = decoder.value.replace(/\u200b/g, '').replace(/\u00a0/g, ' ');
  return decoded
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map(line => line.replace(/[\t ]+/g, ' ').trim())
    .filter((line, index, lines) => line !== '' || (index > 0 && lines[index - 1] !== ''))
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

export default function Transactions() {
  const [rows, setRows] = useState<ExtractionTransaction[]>([]);
  const [filter, setFilter] = useState('Semua');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [savingId, setSavingId] = useState<number | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [noteDraft, setNoteDraft] = useState('');
  const [reviewRow, setReviewRow] = useState<ExtractionTransaction | null>(null);
  const [reviewDraft, setReviewDraft] = useState({ date: '', recipient: '', transactionId: '', amount: '', note: '' });
  const [messageOpen, setMessageOpen] = useState(true);
  const [invoiceOpen, setInvoiceOpen] = useState(true);
  const [messageBody, setMessageBody] = useState('');
  const [messageLoading, setMessageLoading] = useState(false);
  const [messageError, setMessageError] = useState('');

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
  useEffect(() => {
    if (!reviewRow || !messageOpen) return;
    let cancelled = false;
    setMessageBody('');
    setMessageError('');
    setMessageLoading(true);
    invoke<string>('get_gmail_message_body', {
      accountId: reviewRow.gmailAccountId,
      messageId: reviewRow.gmailMessageId,
    })
      .then(body => { if (!cancelled) setMessageBody(body); })
      .catch(error => { if (!cancelled) setMessageError(String(error)); })
      .finally(() => { if (!cancelled) setMessageLoading(false); });
    return () => { cancelled = true; };
  }, [reviewRow, messageOpen]);

  async function retry(id: number) {
    setSavingId(id); setError('');
    try { await transactionService.retry(id); await load(); }
    catch (e) { setError(String(e)); }
    finally { setSavingId(null); }
  }
  function beginEdit(row: ExtractionTransaction) { setEditingId(row.id); setNoteDraft(row.transferNote ?? ''); }
  function openReview(row: ExtractionTransaction) {
    setMessageOpen(true);
    setInvoiceOpen(true);
    setReviewRow(row);
    setReviewDraft({ date: row.transactionDate ?? '', recipient: row.recipientName ?? '', transactionId: row.flipTransactionId ?? '', amount: row.amount == null ? '' : String(row.amount), note: row.transferNote ?? '' });
  }
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
          <thead><tr><th>Tanggal</th><th>Nama Tujuan</th><th>Flip Transaction ID</th><th>Nominal</th><th>Catatan Transfer (OCR)</th><th>Status</th><th>Aksi</th></tr></thead>
          <tbody>
            {rows.map(row => (
              <tr key={row.id}>
                <td>{dateLabel(row.transactionDate)}</td>
                <td className="recipient-cell"><strong>{row.recipientName || '—'}</strong><small className="account-email">{row.accountEmail}</small></td>
                <td className="transaction-id-cell">{row.flipTransactionId || '—'}</td>
                <td className="amount">{formatAmount(row.amount)}</td>
                <td className="note-cell">
                  {editingId === row.id ? (
                    <div className="note-edit"><textarea value={noteDraft} onChange={e => setNoteDraft(e.target.value)} rows={2}/><div><button className="primary small" onClick={() => void saveNote(row.id)} disabled={savingId === row.id}>Simpan</button><button className="subtle small" onClick={() => setEditingId(null)}>Batal</button></div></div>
                  ) : (
                    <><span>{row.transferNote || '—'}</span>{row.reviewReason && <small className="review-reason">{row.reviewReason}</small>}{row.lastError && <small className="error-text">{row.lastError}</small>}</>
                  )}
                </td>
                <td><span className={`status-chip ${row.status.toLowerCase().replaceAll(' ', '-')}`}>{row.status}</span></td>
                <td className="actions-cell">{row.status === 'Needs Review' && <button className="primary small" onClick={() => openReview(row)}>Review & Edit</button>}<button className="subtle small" onClick={() => beginEdit(row)} disabled={savingId === row.id}>Edit Catatan</button>{['Failed', 'Needs Review', 'Duplicate Review'].includes(row.status) && <button className="subtle small" onClick={() => void retry(row.id)} disabled={savingId === row.id}>{savingId === row.id ? 'Retry…' : 'Retry'}</button>}</td>
              </tr>
            ))}
            {rows.length === 0 && <tr><td colSpan={7} className="empty-cell">{loading ? 'Memuat transaksi…' : 'Belum ada data transaksi pada status ini.'}</td></tr>}
          </tbody>
        </table>
      </div>
      <p className="muted table-footnote">Hasil OCR ditampilkan apa adanya. Koreksi Catatan Transfer tersimpan ke database lokal.</p>
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
                <div className="review-save-notice">Form edit sudah tersedia. Penyimpanan seluruh field belum diaktifkan karena command backend untuk koreksi manual belum ada pada source yang dikirim.</div>
                <button type="button" className="primary review-save-disabled" disabled title="Menunggu integrasi backend">Simpan & Selesaikan Review</button>
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
  );
}
