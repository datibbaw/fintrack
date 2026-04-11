import { useSignal } from '@preact/signals'
import { useEffect } from 'preact/hooks'
import { api } from './api'
import type { Account, Category } from './types'
import { activeTab, filterAccount } from './store'
import { FilterBar } from './components/FilterBar'
import { Summary } from './components/Summary'
import { Transactions } from './components/Transactions'

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
          <svg class="logo" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2z"/>
            <path d="M12 6v6l4 2"/>
          </svg>
          <span class="brand-name">fintrack</span>
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
        </nav>
      </header>

      <FilterBar accounts={accounts.value} />

      <main class="main">
        {activeTab.value === 'summary'
          ? <Summary categories={categories.value} />
          : <Transactions categories={categories.value} />
        }
      </main>
    </div>
  )
}
