import { useState } from 'react';
import './App.css';
import Dashboard from '../features/dashboard/pages/Dashboard';
import Transactions from '../features/transactions/pages/Transactions';
import Settings from '../features/settings/pages/Settings';

type AppPage = 'dashboard' | 'transactions' | 'settings';

const navigation: { id: AppPage; label: string; icon: string }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: '◫' },
  { id: 'transactions', label: 'Transaksi', icon: '⇄' },
  { id: 'settings', label: 'Pengaturan', icon: '⚙' },
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
              : <Settings />}
        </div>
      </main>
    </div>
  );
}


export default App;
