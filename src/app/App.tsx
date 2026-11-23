import { useState } from 'react';
import './App.css';
import GmailAccountTest from '../features/gmail/components/GmailAccountTest';
import EmailHistory from '../features/email-history/components/EmailHistory';
import Dashboard from '../features/dashboard/pages/Dashboard';
import Transactions from '../features/transactions/pages/Transactions';
import CopyTracking from '../features/copy-tracking/pages/CopyTracking';
import Retention from '../features/retention/pages/Retention';
import Logs from '../features/logs/pages/Logs';

type AppPage = 'dashboard' | 'transactions' | 'copy-tracking' | 'retention' | 'gmail' | 'history' | 'logs';

const navigation: { id: AppPage; label: string; icon: string }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: '◫' },
  { id: 'transactions', label: 'Transaksi', icon: '⇄' },
  { id: 'copy-tracking', label: 'Copy Tracking', icon: '◷' },
  { id: 'retention', label: 'Data Retention', icon: '⌫' },
  { id: 'gmail', label: 'Akun Gmail', icon: '✉' },
  { id: 'history', label: 'Riwayat Email', icon: '▤' },
  { id: 'logs', label: 'Log Aktivitas', icon: '◷' },
];

function App() {
  const [page, setPage] = useState<AppPage>('dashboard');
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  const activePageLabel = navigation.find((item) => item.id === page)?.label ?? 'Dashboard';

  return (
    <div className={`app-shell ${sidebarCollapsed ? 'sidebar-collapsed' : ''}`}>
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">F</div>
          <div className="brand-copy">
            <strong>Flip V2</strong>
            <small>Transaction Processor</small>
          </div>
          <button
            className="sidebar-toggle"
            type="button"
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
            aria-label={sidebarCollapsed ? 'Lebarkan panel navigasi' : 'Minimalkan panel navigasi'}
            title={sidebarCollapsed ? 'Lebarkan panel' : 'Minimalkan panel'}
          >
            {sidebarCollapsed ? '›' : '‹'}
          </button>
        </div>

        <div className="nav-label">WORKSPACE</div>
        <nav className="app-navigation" aria-label="Navigasi utama">
          {navigation.map((item) => (
            <button
              key={item.id}
              className={page === item.id ? 'active' : ''}
              onClick={() => setPage(item.id)}
              title={sidebarCollapsed ? item.label : undefined}
              aria-current={page === item.id ? 'page' : undefined}
            >
              <span className="nav-icon" aria-hidden="true">{item.icon}</span>
              <span className="nav-text">{item.label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-bottom">
          <span className="connection-dot" />
          <span className="sidebar-bottom-text">Pemrosesan Lokal Aktif</span>
          <small>Private · Offline OCR</small>
        </div>
      </aside>


      <main className="main-content">
        {page !== 'dashboard' && page !== 'transactions' && <header className="topbar">
          <span className="breadcrumb">Flip V2 <span>/</span> {activePageLabel}</span>
          <span className="local-badge"><span className="connection-dot" /> Database lokal</span>
        </header>}
        <div className={`page-content ${page === 'dashboard' ? 'dashboard-content' : ''}`}>
          {page === 'dashboard' ? <Dashboard onOpenTransactions={() => setPage('transactions')} />
            : page === 'transactions' ? <Transactions />
              : page === 'copy-tracking' ? <CopyTracking />
              : page === 'retention' ? <Retention />
              : page === 'gmail' ? <GmailAccountTest />
                : <EmailHistory />}
        </div>
      </main>
    </div>
  );
}


export default App;
