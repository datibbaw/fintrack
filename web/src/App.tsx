import { useSignal } from '@preact/signals'
import { useEffect } from 'preact/hooks'
import { api } from './api'
import type { Account, Category } from './types'
import { activeTab, filterAccount } from './store'
import { FilterBar } from './components/FilterBar'
import { Summary } from './components/Summary'
import { Transactions } from './components/Transactions'
import { Categories } from './components/Categories'
import { Accounts } from './components/Accounts'

// ── App ───────────────────────────────────────────────────────────────────────

export function App() {
  const accounts = useSignal<Account[]>([])
  const categories = useSignal<Category[]>([])

  useEffect(() => {
    api.accounts().then(a => {
      accounts.value = a
      if (localStorage.getItem('fintrack.account') === null && a.length > 0) {
        filterAccount.value = a[0].name
      }
    })
    api.categories().then(c => { categories.value = c })
  }, [])

  return (
    <div class="app">
      <header class="header">
        <div class="header-brand">
          <svg class="logo" viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="9" stroke="var(--text)"/>
            <path d="M12 5v14" stroke="var(--credit)"/>
            <path d="M15.5 7.5H10.5a2.2 2.5 0 0 0 0 5h3a2.2 2.5 0 0 1 0 5H8.5" stroke="var(--credit)"/>
          </svg>
          <span class="brand-name">Tiny Financial Tracker</span>
        </div>
        <nav class="tab-nav">
          <button
            class={`tab-btn ${activeTab.value === 'summary' ? 'active' : ''}`}
            onClick={() => { activeTab.value = 'summary' }}
          >
            Summary
          </button>
          <button
            class={`tab-btn ${activeTab.value === 'transactions' ? 'active' : ''}`}
            onClick={() => { activeTab.value = 'transactions' }}
          >
            Transactions
          </button>
          <button
            class={`tab-btn ${activeTab.value === 'categories' ? 'active' : ''}`}
            onClick={() => { activeTab.value = 'categories' }}
          >
            Categories
          </button>
          <button
            class={`tab-btn ${activeTab.value === 'accounts' ? 'active' : ''}`}
            onClick={() => { activeTab.value = 'accounts' }}
          >
            Accounts
          </button>
        </nav>
      </header>

      {activeTab.value !== 'categories' && activeTab.value !== 'accounts' && <FilterBar accounts={accounts.value} />}

      <main class="main">
        {activeTab.value === 'summary'
          ? <Summary categories={categories.value} />
          : activeTab.value === 'transactions'
          ? <Transactions categories={categories.value} />
          : activeTab.value === 'categories'
          ? <Categories />
          : <Accounts />
        }
      </main>
    </div>
  )
}
