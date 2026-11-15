
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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
  const [error, setError] = useState<string | null>(null);

  async function loadAccounts() {
    setLoading(true);
    setError(null);

    try {
      const result = await invoke<GmailAccount[]>(
        "get_gmail_accounts"
      );

      setAccounts(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section>
      <h2>Gmail Account Backend Test</h2>

      <button onClick={loadAccounts} disabled={loading}>
        {loading ? "Loading..." : "Load Gmail Accounts"}
      </button>

      {error && <p role="alert">Error: {error}</p>}

      {!loading && !error && (
        <p>Total akun: {accounts.length}</p>
      )}

      <ul>
        {accounts.map((account) => (
          <li key={account.id}>
            <strong>{account.emailAddress}</strong>
            {" — "}
            {account.accountStatus}
          </li>
        ))}
      </ul>
    </section>
  );
}
