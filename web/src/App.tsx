import { useSignal } from '@preact/signals'
import { useEffect } from 'preact/hooks'
import { api } from './api'
import type { Category } from './types'
import { accounts, activeTab, filterAccount } from './store'
import { FilterBar } from './components/FilterBar'
import { Summary } from './components/Summary'
import { Transactions } from './components/Transactions'
import { Categories } from './components/Categories'

// ── App ───────────────────────────────────────────────────────────────────────

export function App() {
  const categories = useSignal<Category[]>([])

  useEffect(() => {
    api.accounts().then(a => {
      accounts.value = a
      if (localStorage.getItem('fintrack.account') === null && a.length > 0) {
        filterAccount.value = a[0].number
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
        </nav>
      </header>

      {activeTab.value !== 'categories' && <FilterBar accounts={accounts.value} />}

      <main class="main">
        {activeTab.value === 'summary'
          ? <Summary categories={categories.value} />
          : activeTab.value === 'transactions'
          ? <Transactions categories={categories.value} />
          : <Categories />
        }
      </main>
    </div>
  )
}
