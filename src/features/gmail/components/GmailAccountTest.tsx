import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

export default function GmailAccountTest() {
  const [accounts, setAccounts] = useState<GmailAccount[]>([]);
  const [loading, setLoading] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const loadAccounts = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setAccounts(await invoke<GmailAccount[]>("get_gmail_accounts"));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadAccounts();
  }, [loadAccounts]);

  async function connectGmail() {
    setConnecting(true);
    setError(null);
    setMessage("Menyiapkan autentikasi Google...");
    try {
      const authorizationUrl = await invoke<string>("start_gmail_oauth");
      try {
        await openUrl(authorizationUrl);
      } catch (openError) {
        await invoke("cancel_gmail_oauth");
        throw openError;
      }

      setMessage("Selesaikan login dan berikan izin Gmail di browser. Flip V2 sedang menunggu callback...");
      const account = await invoke<GmailAccount>("complete_gmail_oauth");
      setMessage(`Akun ${account.emailAddress} berhasil terhubung.`);
      await loadAccounts();
    } catch (err) {
      setError(String(err));
      setMessage(null);
    } finally {
      setConnecting(false);
    }
  }

  async function disconnectGmail(account: GmailAccount) {
    const confirmed = window.confirm(`Putuskan koneksi ${account.emailAddress}?`);
    if (!confirmed) return;
    setError(null);
    setMessage(null);
    try {
      await invoke<boolean>("disconnect_gmail_account", { accountId: account.id });
      setMessage(`Koneksi ${account.emailAddress} diputus.`);
      await loadAccounts();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section aria-labelledby="gmail-accounts-title">
      <h2 id="gmail-accounts-title">Akun Gmail</h2>
      <p>Hubungkan akun Gmail untuk mengambil email transaksi Flip.</p>

      <div className="gmail-actions">
        <button onClick={connectGmail} disabled={connecting}>
          {connecting ? "Menghubungkan..." : "Hubungkan akun Gmail"}
        </button>
        <button onClick={() => void loadAccounts()} disabled={loading || connecting}>
          {loading ? "Memuat..." : "Refresh daftar"}
        </button>
      </div>

      {message && <p role="status">{message}</p>}
      {error && <p role="alert" className="gmail-error">{error}</p>}

      {loading && accounts.length === 0 ? (
        <p>Memuat akun Gmail...</p>
      ) : accounts.length === 0 ? (
        <p>Belum ada akun Gmail yang terhubung.</p>
      ) : (
        <ul className="gmail-account-list">
          {accounts.map((account) => (
            <li key={account.id}>
              <div>
                <strong>{account.emailAddress}</strong>
                <span className="gmail-account-status">{account.accountStatus}</span>
              </div>
              {account.accountStatus === "connected" && (
                <button
                  className="gmail-disconnect"
                  onClick={() => void disconnectGmail(account)}
                  disabled={connecting}
                >
                  Putuskan
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <small>Flip V2 hanya meminta izin membaca email (gmail.readonly).</small>
    </section>
  );
}
