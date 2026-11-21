import { useState } from "react";
import "./App.css";
import GmailAccountTest from "../features/gmail/components/GmailAccountTest";
import EmailHistory from "../features/email-history/components/EmailHistory";

type AppPage = "gmail" | "history";


function App() {
  const [page, setPage] = useState<AppPage>("gmail");
  return (
    <main className="container">
      <header className="app-header">
        <h1>Flip V2</h1>
        <p>Pengelolaan akun Gmail dan sinkronisasi email transaksi Flip.</p>
      </header>
      <nav className="app-navigation" aria-label="Navigasi utama">
        <button className={page === "gmail" ? "active" : ""} onClick={() => setPage("gmail")}>Akun Gmail</button>
        <button className={page === "history" ? "active" : ""} onClick={() => setPage("history")}>Riwayat Email</button>
      </nav>
      {page === "gmail" ? <GmailAccountTest /> : <EmailHistory />}
    </main>
  );
}


export default App;
