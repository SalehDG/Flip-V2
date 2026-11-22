import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./GmailAccountTest.css";

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
    <section className="gmail-page" aria-labelledby="gmail-accounts-title">
      <div className="gmail-page-header">
        <div className="gmail-heading-mark" aria-hidden="true">✉</div>
        <div className="gmail-heading-copy">
          <h2 id="gmail-accounts-title">Akun Gmail</h2>
          <p>Kelola akun Gmail untuk mengambil email transaksi Flip.</p>
        </div>
        <div className="gmail-header-actions">
          <button className="gmail-btn gmail-btn-secondary" onClick={() => void loadAccounts()} disabled={loading || connecting}>
            <span className={loading ? "gmail-spin" : ""}>↻</span> {loading ? "Memuat..." : "Refresh"}
          </button>
          <button className="gmail-btn gmail-btn-primary" onClick={connectGmail} disabled={connecting || syncingAccountIds.size > 0}>
            ＋ {connecting ? "Menghubungkan..." : "Hubungkan Akun Gmail"}
          </button>
          {connecting && <button className="gmail-btn gmail-btn-quiet" onClick={() => void cancelConnectGmail()}>Batalkan</button>}
        </div>
      </div>


      <div className="gmail-privacy-note">
        <span className="gmail-info-icon">i</span>
        <div><strong>Akses hanya untuk membaca email.</strong><span>Flip V2 tidak dapat mengirim, mengubah, atau menghapus email Anda.</span></div>
      </div>

      {message && <div className="gmail-feedback gmail-feedback-success" role="status">{message}</div>}
      {error && <div className="gmail-feedback gmail-feedback-error" role="alert">{error}</div>}


      {loading && accounts.length === 0 ? <div className="gmail-empty-state">Memuat akun Gmail...</div> : accounts.length === 0 ? (
        <div className="gmail-empty-state">
          <div className="gmail-empty-icon">✉</div>
          <strong>Belum ada akun Gmail terhubung</strong>
          <span>Hubungkan akun untuk mulai mengambil email transaksi Flip.</span>
          <button className="gmail-btn gmail-btn-primary" onClick={connectGmail} disabled={connecting}>＋ Hubungkan akun Gmail</button>
        </div>
      ) : (
        <div className="gmail-account-list">
          {accounts.map((account) => {
            const current = progress[account.id];
            const savedState = syncStates[account.id];
            const isSyncing = syncingAccountIds.has(account.id);
            const displayStatus = isSyncing ? (current?.status ?? "processing") : (savedState?.status ?? account.accountStatus);
            const isConnected = account.accountStatus === "connected";
            const initials = account.emailAddress.split("@")[0].slice(0, 2).toUpperCase();
            return (
              <article className="gmail-account-card" key={account.id}>
                <div className="gmail-account-top">
                  <div className="gmail-avatar" aria-label={`Avatar ${account.emailAddress}`}><span>{initials}</span><i>G</i></div>
                  <div className="gmail-account-identity">
                    <strong>{account.emailAddress}</strong>
                    <div className="gmail-account-badges">
                      <span className={`gmail-badge ${isConnected ? "is-connected" : "is-disconnected"}`}><i />{isConnected ? "Terhubung" : "Terputus"}</span>
                    </div>
                  </div>
                  <div className="gmail-account-meta">
                    <div className="gmail-meta-item"><span className="gmail-meta-icon">◷</span><div><small>Sinkronisasi terakhir</small><strong>{account.lastSyncAt ? formatLocalSyncTime(account.lastSyncAt) : "Belum pernah"}</strong></div></div>
                    <div className="gmail-meta-item"><span className="gmail-meta-icon">✉</span><div><small>Email diproses</small><strong>{savedState?.processedMessages ?? current?.processedMessages ?? 0}</strong></div></div>
                  </div>
                </div>

                <div className="gmail-sync-area">
                  <div className="gmail-sync-title"><span className="gmail-sync-symbol">↻</span><strong>Sinkronisasi</strong></div>
                  <div className="gmail-sync-row">
                    <label className="gmail-date-field"><span>Mulai dari tanggal</span><input id={`sync-date-${account.id}`} type="date" value={startDates[account.id] ?? ""} disabled={isSyncing} onChange={(e) => setStartDates((old) => ({ ...old, [account.id]: e.target.value }))} /></label>
                    <button className="gmail-btn gmail-btn-primary gmail-sync-initial" onClick={() => void startSync(account)} disabled={isSyncing || !isConnected}>
                      <span>{isSyncing ? "◌" : "▶"}</span>{isSyncing ? "Sinkronisasi berjalan..." : "Mulai Sync Awal"}
                    </button>
                    {account.historyId && <button className="gmail-btn gmail-btn-secondary gmail-sync-incremental" onClick={() => void startIncrementalSync(account)} disabled={isSyncing || !isConnected}><span>↻</span> Sync Email Baru</button>}
                  </div>
                  <p className="gmail-sync-helper">Sync awal mengambil email sejak tanggal yang dipilih. Sync berikutnya hanya mengambil email baru.</p>
                </div>

                <div className={`gmail-sync-status ${isSyncing ? "status-running" : savedState?.lastError ? "status-error" : "status-idle"}`}>
                  <span className="gmail-status-dot" />
                  <div className="gmail-status-copy"><strong>{isSyncing ? "Sinkronisasi berlangsung" : savedState?.lastError ? "Sinkronisasi bermasalah" : "Sinkronisasi siap"}</strong><span>{isSyncing ? `${displayStatus} · ${current?.processedMessages ?? 0} email diproses${current?.attachmentCount !== undefined ? ` · ${current.attachmentCount} lampiran` : ""}` : savedState?.lastError ? savedState.lastError : "Tidak ada proses sinkronisasi yang berjalan."}</span></div>
                  {isConnected && <button className="gmail-disconnect-link" onClick={() => void disconnectGmail(account)} disabled={connecting || isSyncing}>Putuskan akun</button>}
                </div>
              </article>
            );
          })}
        </div>
      )}
      <button className="gmail-add-account-card" onClick={connectGmail} disabled={connecting || syncingAccountIds.size > 0}>
        <span className="gmail-add-icon">＋</span><span><strong>Tambah akun Gmail lain</strong><small>Hubungkan akun tambahan untuk mengambil transaksi dari beberapa akun Flip.</small></span><span className="gmail-add-chevron">›</span>
      </button>
      <p className="gmail-footer-note">Flip V2 menggunakan izin <code>gmail.readonly</code>. Sinkronisasi email baru memanfaatkan Gmail History API.</p>
    </section>
  );
}
