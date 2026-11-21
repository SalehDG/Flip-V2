import { useEffect, useMemo, useState } from "react";
import { getGmailHistory } from "../services/emailHistoryService";
import type { GmailHistoryEmail } from "../types/emailHistory";
import "./EmailHistory.css";

type Filter = "all" | "with" | "without" | "missing";


function formatDate(value?: string | null) {
  if (!value) return "Tanggal tidak tersedia";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("id-ID", { dateStyle: "medium", timeStyle: "short", hour12: false }).format(date);
}
function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}
function senderName(sender?: string | null) {
  if (!sender) return "Pengirim tidak diketahui";
  const match = sender.match(/^\s*"?([^"<]+?)"?\s*<.*>\s*$/);
  return match?.[1]?.trim() || sender;
}


export default function EmailHistory() {
  const [emails, setEmails] = useState<GmailHistoryEmail[]>([]);
  const [selected, setSelected] = useState<GmailHistoryEmail | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const rows = await getGmailHistory();
      setEmails(rows);
      setSelected((current) => current ? rows.find((row) => row.accountId === current.accountId && row.messageId === current.messageId) ?? null : rows[0] ?? null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => { void refresh(); }, []);

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    return emails.filter((email) => {
      const hasAttachments = email.attachments.length > 0;
      const hasMissing = email.attachments.some((attachment) => !attachment.fileExists);
      const passesFilter = filter === "all" || (filter === "with" && hasAttachments) || (filter === "without" && !hasAttachments) || (filter === "missing" && hasMissing);
      const searchable = [email.accountEmail, email.sender, email.subject, email.snippet, ...email.attachments.map((attachment) => attachment.filename)].join(" ").toLowerCase();
      return passesFilter && (!query || searchable.includes(query));
    });
  }, [emails, filter, search]);

  const attachmentTotal = emails.reduce((total, email) => total + email.attachments.length, 0);
  const savedTotal = emails.reduce((total, email) => total + email.attachments.filter((attachment) => attachment.fileExists).length, 0);
  const missingTotal = attachmentTotal - savedTotal;

  return (
    <section className="history-page" aria-label="Riwayat email">
      <header className="history-header">
        <div>
          <h2>Riwayat Email</h2>
          <p>Audit email dan attachment yang sudah tersimpan secara lokal.</p>
        </div>
        <button className="history-refresh" onClick={() => void refresh()} disabled={loading}>{loading ? "Memuat..." : "Refresh"}</button>
      </header>

      <div className="history-stats">
        <article><span>Total email</span><strong>{emails.length.toLocaleString("id-ID")}</strong></article>
        <article><span>Attachment</span><strong>{attachmentTotal.toLocaleString("id-ID")}</strong></article>
        <article><span>File ditemukan</span><strong className="history-success">{savedTotal.toLocaleString("id-ID")}</strong></article>
        <article><span>File hilang</span><strong className={missingTotal ? "history-danger" : ""}>{missingTotal.toLocaleString("id-ID")}</strong></article>
      </div>

      {error && <div className="history-error" role="alert">Gagal mengambil riwayat: {error}</div>}

      <div className="history-toolbar">
        <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Cari email, pengirim, atau attachment..." aria-label="Cari riwayat email" />
        <div className="history-filter-tabs" role="group" aria-label="Filter email">
          {([ ["all", "Semua"], ["with", "Attachment"], ["without", "Tanpa file"], ["missing", "File hilang"] ] as [Filter, string][]).map(([value, label]) => (
            <button key={value} className={filter === value ? "active" : ""} onClick={() => setFilter(value)}>{label}</button>
          ))}
        </div>
      </div>


      <div className="history-content">
        <div className="history-list-panel">
          <div className="history-list-heading"><span>{filtered.length.toLocaleString("id-ID")} email</span><span>Terbaru</span></div>
          {loading && emails.length === 0 ? <div className="history-empty">Memuat data email...</div> : filtered.length === 0 ? <div className="history-empty">Tidak ada email yang cocok.</div> : (
            <div className="history-list">
              {filtered.map((email) => {
                const active = selected?.accountId === email.accountId && selected.messageId === email.messageId;
                const allFilesPresent = email.attachments.every((attachment) => attachment.fileExists);
                return <button key={`${email.accountId}-${email.messageId}`} className={`history-email-row ${active ? "selected" : ""}`} onClick={() => setSelected(email)}>
                  <div className="history-row-top"><strong>{senderName(email.sender)}</strong><time>{formatDate(email.internalDate)}</time></div>
                  <div className="history-row-subject">{email.subject || "(Tanpa subjek)"}</div>
                  <div className="history-row-snippet">{email.snippet || "Tidak ada cuplikan email."}</div>
                  <div className="history-row-meta">
                    <span className="history-account-tag">{email.accountEmail}</span>
                    {email.attachments.length > 0 && <span className={allFilesPresent ? "file-pill ok" : "file-pill missing"}>{email.attachments.length} file · {allFilesPresent ? "tersimpan" : "periksa"}</span>}
                    {email.attachments.length === 0 && <span className="file-pill neutral">Tanpa attachment</span>}
                  </div>
                </button>;
              })}
            </div>
          )}
        </div>

        <aside className="history-detail-panel">
          {!selected ? <div className="history-empty detail-empty">Pilih email untuk melihat detail.</div> : (
            <>
              <div className="detail-title"><span className="detail-icon">✉</span><h3>{selected.subject || "(Tanpa subjek)"}</h3></div>
              <dl className="history-email-details">
                <div><dt>Dari</dt><dd>{selected.sender || "Tidak tersedia"}</dd></div>
                <div><dt>Akun</dt><dd>{selected.accountEmail}</dd></div>
                <div><dt>Tanggal email</dt><dd>{formatDate(selected.internalDate)}</dd></div>
                <div><dt>Diambil pada</dt><dd>{formatDate(selected.retrievedAt)}</dd></div>
                <div><dt>Message ID</dt><dd className="detail-mono">{selected.messageId}</dd></div>
                <div><dt>Thread ID</dt><dd className="detail-mono">{selected.threadId}</dd></div>
              </dl>
              {selected.snippet && <div className="history-snippet-block"><label>Cuplikan email</label><p>{selected.snippet}</p></div>}
              <section className="history-attachments">
                <div className="attachments-title"><h4>Attachment</h4><span>{selected.attachments.length} file</span></div>
                {selected.attachments.length === 0 ? <div className="no-attachments">Email ini tidak memiliki attachment tersimpan.</div> : selected.attachments.map((attachment) => (
                  <article key={attachment.id} className="attachment-card">
                    <div className="attachment-icon">▧</div>
                    <div className="attachment-info">
                      <strong>{attachment.filename || "File tanpa nama"}</strong>
                      <span>{attachment.detectedFormat.toUpperCase()} · {formatSize(attachment.fileSize)}</span>
                      <span className={attachment.fileExists ? "history-success" : "history-danger"}>{attachment.fileExists ? "File ditemukan di penyimpanan" : "File tidak ditemukan di lokasi penyimpanan"}</span>
                      <code>{attachment.localPath}</code>
                    </div>
                  </article>
                ))}
              </section>
            </>
          )}
        </aside>
      </div>
      <p className="history-footnote">Riwayat dibaca dari database lokal. Membuka halaman ini tidak mengambil ulang email dari Gmail.</p>
    </section>
  );
}
