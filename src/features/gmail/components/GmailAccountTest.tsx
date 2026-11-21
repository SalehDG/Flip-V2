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

function formatLocalSyncTime(value: string): string {
  // SQLite CURRENT_TIMESTAMP is UTC but stored without a timezone suffix.
  const normalized = value.includes("T") ? value : value.replace(" ", "T");
  const hasTimezone = /(?:Z|[+-]\d{2}:?\d{2})$/i.test(normalized);
  const date = new Date(hasTimezone ? normalized : `${normalized}Z`);

  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat("id-ID", {
    dateStyle: "medium",
    timeStyle: "medium",
    hour12: false,
  }).format(date);
}

interface SyncState {
  accountId: number;
  status: string;
  startDate: string | null;
  processedMessages: number;
  pageToken: string | null;
  lastError: string | null;
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
  const [syncingAccountIds, setSyncingAccountIds] = useState<Set<number>>(new Set());
  const [startDates, setStartDates] = useState<Record<number, string>>({});
  const [progress, setProgress] = useState<Record<number, SyncProgress>>({});
  const [syncStates, setSyncStates] = useState<Record<number, SyncState>>({});
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const loadAccounts = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await invoke<GmailAccount[]>("get_gmail_accounts");
      setAccounts(rows);
      const states = await Promise.all(
        rows.map(async (account) => {
          try {
            return [
              account.id,
              await invoke<SyncState>("get_gmail_sync_state", { accountId: account.id }),
            ] as const;
          } catch {
            return [account.id, null] as const;
          }
        }),
      );
      const nextStates: Record<number, SyncState> = {};
      for (const [accountId, state] of states) {
        if (state) nextStates[accountId] = state;
      }
      setSyncStates((current) => ({ ...current, ...nextStates }));
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
      setMessage("Selesaikan login di browser, atau batalkan koneksi jika browser ditutup...");
      const account = await invoke<GmailAccount>("complete_gmail_oauth");
      setMessage(`Akun ${account.emailAddress} berhasil terhubung.`);
      await loadAccounts();
    } catch (err) { setError(String(err)); setMessage(null); }
    finally { setConnecting(false); }
  }


  async function cancelConnectGmail() {
    try {
      await invoke("cancel_gmail_oauth");
      setMessage("Proses koneksi Gmail dibatalkan.");
      setError(null);
    } catch (err) {
      setError(`Gagal membatalkan koneksi: ${String(err)}`);
    } finally {
      setConnecting(false);
    }
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
    setSyncingAccountIds((current) => new Set(current).add(account.id));
    setError(null);
    setMessage(`Memulai sinkronisasi ${account.emailAddress}...`);
    try {
      const summary = await invoke<SyncSummary>("start_initial_gmail_sync", { accountId: account.id, startDate: date });
      setMessage(`Sinkronisasi selesai. ${summary.processedMessages} message baru, ${summary.skippedMessages} dilewati, ${summary.attachmentCount} attachment PNG/JPEG tersimpan.`);
      await loadAccounts();
    } catch (err) { setError(String(err)); setMessage(null); }
    finally {
      setSyncingAccountIds((current) => {
        const next = new Set(current);
        next.delete(account.id);
        return next;
      });
    }
  }

  async function startIncrementalSync(account: GmailAccount) {
    setSyncingAccountIds((current) => new Set(current).add(account.id));
    setError(null);
    setMessage(`Memeriksa email baru untuk ${account.emailAddress}...`);
    try {
      const summary = await invoke<SyncSummary>("start_incremental_gmail_sync", {
        accountId: account.id,
      });
      setMessage(
        `Sinkronisasi inkremental selesai. ${summary.processedMessages} message baru, ` +
        `${summary.skippedMessages} dilewati, ${summary.attachmentCount} attachment PNG/JPEG tersimpan.`,
      );
      await loadAccounts();
    } catch (err) {
      setError(String(err));
      setMessage(null);
    } finally {
      setSyncingAccountIds((current) => {
        const next = new Set(current);
        next.delete(account.id);
        return next;
      });
    }
  }

  return (
    <section aria-labelledby="gmail-accounts-title">
      <h2 id="gmail-accounts-title">Akun Gmail</h2>
      <p>Hubungkan akun Gmail untuk mengambil email transaksi Flip.</p>
      <div className="gmail-actions">
        <button onClick={connectGmail} disabled={connecting || syncingAccountIds.size > 0}>{connecting ? "Menghubungkan..." : "Hubungkan akun Gmail"}</button>
        {connecting && <button onClick={() => void cancelConnectGmail()}>Batalkan koneksi</button>}
        <button onClick={() => void loadAccounts()} disabled={loading || connecting}>{loading ? "Memuat..." : "Refresh daftar"}</button>
      </div>
      {message && <p role="status">{message}</p>}
      {error && <p role="alert" className="gmail-error">{error}</p>}
      {loading && accounts.length === 0 ? <p>Memuat akun Gmail...</p> : accounts.length === 0 ? <p>Belum ada akun Gmail yang terhubung.</p> : (
        <ul className="gmail-account-list">
          {accounts.map((account) => {
            const current = progress[account.id];
            const savedState = syncStates[account.id];
            const isSyncing = syncingAccountIds.has(account.id);
            return <li key={account.id}>
              <div>
                <strong>{account.emailAddress}</strong>
                <span className="gmail-account-status">{account.accountStatus}</span>
                <div className="gmail-sync-controls">
                  <label htmlFor={`sync-date-${account.id}`}>Tanggal awal sinkronisasi</label>
                  <input id={`sync-date-${account.id}`} type="date" value={startDates[account.id] ?? ""} disabled={isSyncing} onChange={(e) => setStartDates((old) => ({ ...old, [account.id]: e.target.value }))} />
                  <button onClick={() => void startSync(account)} disabled={isSyncing || account.accountStatus !== "connected"}>{isSyncing ? "Sinkronisasi..." : "Mulai / Lanjutkan Initial Sync"}</button>
                  {account.historyId && (
                    <button
                      onClick={() => void startIncrementalSync(account)}
                      disabled={isSyncing || account.accountStatus !== "connected"}
                    >
                      {isSyncing ? "Sinkronisasi..." : "Sinkronkan Email Baru"}
                    </button>
                  )}
                  {(current || savedState) && (
                    <small>
                      Status: {current?.status ?? savedState?.status} ·
                      Message: {current?.processedMessages ?? savedState?.processedMessages ?? 0}
                      {current?.attachmentCount !== undefined
                        ? ` · Attachment: ${current.attachmentCount}`
                        : ""}
                    </small>
                  )}
                  {savedState?.lastError && (
                    <small className="gmail-error">Error terakhir: {savedState.lastError}</small>
                  )}
                  {account.lastSyncAt && <small>Sinkronisasi terakhir: {formatLocalSyncTime(account.lastSyncAt)}</small>}
                </div>
              </div>
              {account.accountStatus === "connected" && <button className="gmail-disconnect" onClick={() => void disconnectGmail(account)} disabled={connecting || isSyncing}>Putuskan</button>}
            </li>;
          })}
        </ul>
      )}
      <small>Flip V2 hanya meminta izin membaca email (gmail.readonly). Initial Sync mengambil email mulai tanggal yang dipilih. Sinkronkan Email Baru menggunakan Gmail History API. Polling otomatis dan system tray termasuk fase Background & Retention.</small>
    </section>
  );
}
