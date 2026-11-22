import { useState } from 'react';
import './App.css';
import GmailAccountTest from '../features/gmail/components/GmailAccountTest';
import EmailHistory from '../features/email-history/components/EmailHistory';
import Dashboard from '../features/dashboard/pages/Dashboard';
import Transactions from '../features/transactions/pages/Transactions';

type AppPage = 'dashboard' | 'transactions' | 'gmail' | 'history';
const navigation: { id: AppPage; label: string; icon: string }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: '◫' }, { id: 'transactions', label: 'Transaksi', icon: '⇄' },
  { id: 'gmail', label: 'Akun Gmail', icon: '✉' }, { id: 'history', label: 'Riwayat Email', icon: '▤' },
];
function App() {
  const [page, setPage] = useState<AppPage>('dashboard');
  return <div className="app-shell"><aside className="sidebar"><div className="brand"><div className="brand-mark">F</div><div><strong>Flip V2</strong><small>Transaction Processor</small></div></div><div className="nav-label">WORKSPACE</div><nav className="app-navigation" aria-label="Navigasi utama">{navigation.map(item => <button key={item.id} className={page === item.id ? 'active' : ''} onClick={() => setPage(item.id)}><span className="nav-icon">{item.icon}</span>{item.label}</button>)}</nav><div className="sidebar-bottom"><span className="connection-dot"/>Local processing enabled<small>Private · Offline OCR</small></div></aside>
    <main className="main-content"><header className="topbar"><span className="breadcrumb">Flip V2 <span>/</span> {navigation.find(item => item.id === page)?.label}</span><span className="local-badge"><span className="connection-dot"/> Database lokal</span></header><div className="page-content">{page === 'dashboard' ? <Dashboard onOpenTransactions={() => setPage('transactions')} /> : page === 'transactions' ? <Transactions /> : page === 'gmail' ? <GmailAccountTest /> : <EmailHistory />}</div></main></div>;
}
export default App;
