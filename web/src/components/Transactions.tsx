import { signal, effect, computed } from '@preact/signals'
import type { Category, Transaction, TransactionsResponse } from '../types'
import { filterFrom, filterTo, filterAccount, categoryFilter, uncategorized } from '../store'
import { api } from '../api'

interface Props {
  categories: Category[]
}

const PAGE_SIZE = 50

const data    = signal<TransactionsResponse | null>(null)
const loading = signal(false)
const error   = signal<string | null>(null)
const page    = signal(0)
const search  = signal('')

// Reset page when filters change
effect(() => {
  // Access all filter signals to track them
  filterFrom.value; filterTo.value; filterAccount.value
  categoryFilter.value; uncategorized.value
  page.value = 0
})

// Fetch data
effect(() => {
  const from    = filterFrom.value
  const to      = filterTo.value
  const account = filterAccount.value
  const cat     = uncategorized.value ? '' : categoryFilter.value
  const offset  = page.value * PAGE_SIZE

  loading.value = true
  error.value   = null

  api.transactions({
    from,
    to,
    account,
    category: cat,
    uncategorized: uncategorized.value || undefined,
    limit: PAGE_SIZE,
    offset,
  })
    .then(d => { data.value = d })
    .catch(e => { error.value = String(e) })
    .finally(() => { loading.value = false })
})

// Client-side search filter (applied on top of server results)
const visibleRows = computed(() => {
  const q = search.value.trim().toLowerCase()
  if (!q || !data.value) return data.value?.rows ?? []
  return data.value.rows.filter(tx =>
    tx.description.toLowerCase().includes(q) ||
    tx.ref1.toLowerCase().includes(q) ||
    tx.ref2.toLowerCase().includes(q) ||
    tx.ref3.toLowerCase().includes(q) ||
    (tx.category ?? '').toLowerCase().includes(q)
  )
})

function fmt(n: number): string {
  return n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

export function Transactions({ categories }: Props) {
  const d     = data.value
  const total = d?.total ?? 0
  const pages = Math.ceil(total / PAGE_SIZE)

  return (
    <div class="transactions">
      <div class="tx-controls">
        <input
          type="search"
          class="filter-input search-input"
          placeholder="Search description, ref…"
          value={search.value}
          onInput={e => { search.value = (e.target as HTMLInputElement).value }}
        />
        <select
          class="filter-input"
          value={uncategorized.value ? '__uncategorized__' : categoryFilter.value}
          onChange={e => {
            const v = (e.target as HTMLSelectElement).value
            if (v === '__uncategorized__') {
              uncategorized.value  = true
              categoryFilter.value = ''
            } else {
              uncategorized.value  = false
              categoryFilter.value = v
            }
          }}
          disabled={uncategorized.value && false}
        >
          <option value="">All categories</option>
          <option value="__uncategorized__">Uncategorized only</option>
          {categories.map(c => (
            <option key={c.id} value={c.name}>
              {c.parent ? `${c.parent} / ${c.name}` : c.name}
            </option>
          ))}
        </select>
        {total > 0 && (
          <span class="tx-count text-muted">
            {total.toLocaleString()} transaction{total !== 1 ? 's' : ''}
          </span>
        )}
      </div>

      {loading.value && <div class="state-message">Loading…</div>}
      {error.value   && <div class="state-message error">Error: {error.value}</div>}

      {!loading.value && !error.value && (
        <>
          {visibleRows.value.length === 0
            ? <div class="state-message">No transactions found.</div>
            : (
              <div class="table-wrap">
                <table class="data-table tx-table">
                  <thead>
                    <tr>
                      <th class="col-date">Date</th>
                      <th class="col-desc">Description</th>
                      <th class="col-ref">Ref</th>
                      <th class="col-category">Category</th>
                      <th class="col-number">Debit</th>
                      <th class="col-number">Credit</th>
                      <th class="col-account">Account</th>
                    </tr>
                  </thead>
                  <tbody>
                    {visibleRows.value.map(tx => (
                      <TxRow key={tx.id} tx={tx} />
                    ))}
                  </tbody>
                </table>
              </div>
            )
          }

          {pages > 1 && (
            <div class="pagination">
              <button
                class="page-btn"
                disabled={page.value === 0}
                onClick={() => { page.value = Math.max(0, page.value - 1) }}
              >
                ← Prev
              </button>
              <span class="page-info">
                Page {page.value + 1} of {pages}
              </span>
              <button
                class="page-btn"
                disabled={page.value >= pages - 1}
                onClick={() => { page.value = Math.min(pages - 1, page.value + 1) }}
              >
                Next →
              </button>
            </div>
          )}
        </>
      )}
    </div>
  )
}

function TxRow({ tx }: { tx: Transaction }) {
  const ref = [tx.ref1, tx.ref2, tx.ref3].filter(Boolean).join(' · ')

  return (
    <tr>
      <td class="col-date mono">{tx.date}</td>
      <td class="col-desc" title={tx.description}>{tx.description || tx.code}</td>
      <td class="col-ref text-muted" title={ref}>{ref || '–'}</td>
      <td class="col-category">
        {tx.category
          ? <span class="category-badge">{tx.category}</span>
          : <span class="text-muted">–</span>
        }
      </td>
      <td class="col-number mono debit-value">{tx.debit != null ? fmt(tx.debit) : ''}</td>
      <td class="col-number mono credit-value">{tx.credit != null ? fmt(tx.credit) : ''}</td>
      <td class="col-account text-muted">{tx.account}</td>
    </tr>
  )
}
