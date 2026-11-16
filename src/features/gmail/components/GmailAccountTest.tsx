import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

interface GmailAccount {
  id: number;
  emailAddress: string;
  historyId: string | null;
  syncStartDate: string | null;
  lastSyncAt: string | null;
  accountStatus: string;
  createdAt: string;
  updatedAt: string;
}
interface SyncSummary {
  accountId: number;
  processedMessages: number;
  skippedMessages: number;
  attachmentCount: number;
  completed: boolean;
}
interface SyncProgress {
  accountId: number;
  status: string;
  processedMessages?: number;
  attachmentCount?: number;
  estimatedTotal?: number;
}

export default function GmailAccountTest() {
  const [accounts, setAccounts] = useState<GmailAccount[]>([]);
  const [loading, setLoading] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [syncingAccountId, setSyncingAccountId] = useState<number | null>(null);
  const [startDates, setStartDates] = useState<Record<number, string>>({});
  const [progress, setProgress] = useState<Record<number, SyncProgress>>({});
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const loadAccounts = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await invoke<GmailAccount[]>("get_gmail_accounts");
      setAccounts(rows);
      setStartDates((current) => {
        const next = { ...current };
        for (const account of rows) {
          if (!next[account.id]) next[account.id] = account.syncStartDate ?? new Date().toISOString().slice(0, 10);
        }
        return next;
      });
    } catch (err) { setError(String(err)); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    void loadAccounts();
    let unlisten: (() => void) | undefined;
    void listen<SyncProgress>("gmail-sync-progress", (event) => {
      setProgress((current) => ({ ...current, [event.payload.accountId]: event.payload }));
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [loadAccounts]);

  async function connectGmail() {
    setConnecting(true); setError(null); setMessage("Menyiapkan autentikasi Google...");
    try {
      const authorizationUrl = await invoke<string>("start_gmail_oauth");
      try { await openUrl(authorizationUrl); }
      catch (openError) { await invoke("cancel_gmail_oauth"); throw openError; }
      setMessage("Selesaikan login dan berikan izin Gmail di browser...");
      const account = await invoke<GmailAccount>("complete_gmail_oauth");
      setMessage(`Akun ${account.emailAddress} berhasil terhubung.`);
      await loadAccounts();
    } catch (err) { setError(String(err)); setMessage(null); }
    finally { setConnecting(false); }
  }

  async function disconnectGmail(account: GmailAccount) {
    if (!window.confirm(`Putuskan koneksi ${account.emailAddress}?`)) return;
    setError(null); setMessage(null);
    try {
      await invoke<boolean>("disconnect_gmail_account", { accountId: account.id });
      setMessage(`Koneksi ${account.emailAddress} diputus.`);
      await loadAccounts();
    } catch (err) { setError(String(err)); }
  }

  async function startSync(account: GmailAccount) {
    const date = startDates[account.id];
    if (!date) { setError("Tentukan tanggal awal sinkronisasi."); return; }
    setSyncingAccountId(account.id); setError(null); setMessage(`Memulai sinkronisasi ${account.emailAddress}...`);
    try {
      const summary = await invoke<SyncSummary>("start_initial_gmail_sync", { accountId: account.id, startDate: date });
      setMessage(`Sinkronisasi selesai. ${summary.processedMessages} message baru, ${summary.skippedMessages} dilewati, ${summary.attachmentCount} attachment PNG/JPEG tersimpan.`);
      await loadAccounts();
    } catch (err) { setError(String(err)); setMessage(null); }
    finally { setSyncingAccountId(null); }
  }

  return (
    <section aria-labelledby="gmail-accounts-title">
      <h2 id="gmail-accounts-title">Akun Gmail</h2>
      <p>Hubungkan akun Gmail untuk mengambil email transaksi Flip.</p>
      <div className="gmail-actions">
        <button onClick={connectGmail} disabled={connecting || syncingAccountId !== null}>{connecting ? "Menghubungkan..." : "Hubungkan akun Gmail"}</button>
        <button onClick={() => void loadAccounts()} disabled={loading || connecting}>{loading ? "Memuat..." : "Refresh daftar"}</button>
      </div>
      {message && <p role="status">{message}</p>}
      {error && <p role="alert" className="gmail-error">{error}</p>}
      {loading && accounts.length === 0 ? <p>Memuat akun Gmail...</p> : accounts.length === 0 ? <p>Belum ada akun Gmail yang terhubung.</p> : (
        <ul className="gmail-account-list">
          {accounts.map((account) => {
            const current = progress[account.id];
            const isSyncing = syncingAccountId === account.id;
            return <li key={account.id}>
              <div>
                <strong>{account.emailAddress}</strong>
                <span className="gmail-account-status">{account.accountStatus}</span>
                <div className="gmail-sync-controls">
                  <label htmlFor={`sync-date-${account.id}`}>Tanggal awal sinkronisasi</label>
                  <input id={`sync-date-${account.id}`} type="date" value={startDates[account.id] ?? ""} disabled={isSyncing} onChange={(e) => setStartDates((old) => ({ ...old, [account.id]: e.target.value }))} />
                  <button onClick={() => void startSync(account)} disabled={isSyncing || account.accountStatus !== "connected"}>{isSyncing ? "Sinkronisasi..." : "Mulai / Lanjutkan Sync"}</button>
                  {current && <small>Status: {current.status} · Message: {current.processedMessages ?? 0} · Attachment: {current.attachmentCount ?? 0}</small>}
                  {account.lastSyncAt && <small>Sinkronisasi terakhir: {account.lastSyncAt}</small>}
                </div>
              </div>
              {account.accountStatus === "connected" && <button className="gmail-disconnect" onClick={() => void disconnectGmail(account)} disabled={connecting || isSyncing}>Putuskan</button>}
            </li>;
          })}
        </ul>
      )}
      <small>Flip V2 hanya meminta izin membaca email (gmail.readonly). Initial Sync mengambil email mulai tanggal yang dipilih; polling otomatis belum termasuk Phase 2.</small>
    </section>
  );
}
