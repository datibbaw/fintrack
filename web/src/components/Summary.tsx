import { signal, effect } from '@preact/signals'
import type { Category, SummaryResponse, SummaryRow } from '../types'
import { filterFrom, filterTo, filterAccount, drillIntoCategory } from '../store'
import { api } from '../api'

interface Props {
  categories: Category[]
}

const data   = signal<SummaryResponse | null>(null)
const loading = signal(false)
const error   = signal<string | null>(null)

function fmt(n: number): string {
  return n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

function netClass(n: number): string {
  if (n > 0) return 'positive'
  if (n < 0) return 'negative'
  return ''
}

effect(() => {
  const from    = filterFrom.value
  const to      = filterTo.value
  const account = filterAccount.value

  loading.value = true
  error.value   = null

  api.summary({ from, to, account })
    .then(d => { data.value = d })
    .catch(e => { error.value = String(e) })
    .finally(() => { loading.value = false })
})

export function Summary(_props: Props) {
  if (loading.value) {
    return <div class="state-message">Loading…</div>
  }

  if (error.value) {
    return <div class="state-message error">Error: {error.value}</div>
  }

  const d = data.value
  if (!d || d.rows.length === 0) {
    return <div class="state-message">No transactions found for this period.</div>
  }

  const maxAbsNet = Math.max(...d.rows.map(r => Math.abs(r.net)), 0.01)

  return (
    <div class="summary">
      <div class="summary-totals">
        <div class="total-card debit-card">
          <div class="total-label">Total spending</div>
          <div class="total-value">{fmt(d.total_debit)}</div>
        </div>
        <div class="total-card credit-card">
          <div class="total-label">Total income</div>
          <div class="total-value">{fmt(d.total_credit)}</div>
        </div>
        <div class={`total-card net-card ${netClass(d.total_net)}`}>
          <div class="total-label">Net</div>
          <div class="total-value">{d.total_net >= 0 ? '+' : ''}{fmt(d.total_net)}</div>
        </div>
      </div>

      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th class="col-category">Category</th>
              <th class="col-number">Debit</th>
              <th class="col-number">Credit</th>
              <th class="col-number">Net</th>
              <th class="col-count">Txns</th>
              <th class="col-bar"></th>
            </tr>
          </thead>
          <tbody>
            {d.rows.map(row => (
              <SummaryRowEl key={row.category} row={row} maxAbsNet={maxAbsNet} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function SummaryRowEl({ row, maxAbsNet }: { row: SummaryRow; maxAbsNet: number }) {
  const barPct = maxAbsNet > 0 ? (Math.abs(row.net) / maxAbsNet) * 100 : 0
  const barClass = row.net > 0 ? 'bar-fill positive' : row.net < 0 ? 'bar-fill negative' : 'bar-fill'

  return (
    <tr>
      <td class="col-category">
        {row.parent && <span class="parent-badge">{row.parent} /</span>}
        <span
          class={`category-link ${row.category === 'Uncategorized' ? 'text-muted' : ''}`}
          onClick={() => drillIntoCategory(row.category, row.category_id)}
          title="Show transactions"
        >
          {row.category}
        </span>
      </td>
      <td class="col-number mono debit-value">{row.debit > 0 ? fmt(row.debit) : '–'}</td>
      <td class="col-number mono credit-value">{row.credit > 0 ? fmt(row.credit) : '–'}</td>
      <td class={`col-number mono ${netClass(row.net)}`}>
        {row.net !== 0 ? (row.net > 0 ? '+' : '') + fmt(row.net) : '–'}
      </td>
      <td class="col-count mono">{row.count}</td>
      <td class="col-bar">
        <div class="bar-track">
          <div class={barClass} style={{ width: `${barPct}%` }} />
        </div>
      </td>
    </tr>
  )
}
