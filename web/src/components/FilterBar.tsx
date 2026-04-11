import type { Account } from '../types'
import { filterFrom, filterTo, filterAccount } from '../store'

interface Props {
  accounts: Account[]
}

function startOfMonth(offset = 0): string {
  const d = new Date()
  d.setMonth(d.getMonth() + offset, 1)
  return d.toISOString().slice(0, 10)
}

function endOfMonth(offset = 0): string {
  const d = new Date()
  d.setMonth(d.getMonth() + 1 + offset, 0)
  return d.toISOString().slice(0, 10)
}

function startOfYear(): string {
  return `${new Date().getFullYear()}-01-01`
}

function today(): string {
  return new Date().toISOString().slice(0, 10)
}

export function FilterBar({ accounts }: Props) {
  const presets = [
    {
      label: 'This month',
      apply: () => { filterFrom.value = startOfMonth(); filterTo.value = today() },
    },
    {
      label: 'Last month',
      apply: () => { filterFrom.value = startOfMonth(-1); filterTo.value = endOfMonth(-1) },
    },
    {
      label: 'This year',
      apply: () => { filterFrom.value = startOfYear(); filterTo.value = today() },
    },
    {
      label: 'All time',
      apply: () => { filterFrom.value = ''; filterTo.value = '' },
    },
  ]

  return (
    <div class="filter-bar">
      <div class="filter-group">
        <label class="filter-label">From</label>
        <input
          type="date"
          class="filter-input"
          value={filterFrom.value}
          onInput={e => { filterFrom.value = (e.target as HTMLInputElement).value }}
        />
      </div>
      <div class="filter-group">
        <label class="filter-label">To</label>
        <input
          type="date"
          class="filter-input"
          value={filterTo.value}
          onInput={e => { filterTo.value = (e.target as HTMLInputElement).value }}
        />
      </div>
      <div class="filter-group">
        <label class="filter-label">Account</label>
        <select
          class="filter-input"
          value={filterAccount.value}
          onChange={e => { filterAccount.value = (e.target as HTMLSelectElement).value }}
        >
          <option value="">All accounts</option>
          {accounts.map(a => (
            <option key={a.id} value={a.name}>{a.name}</option>
          ))}
        </select>
      </div>
      <div class="filter-presets">
        {presets.map(p => (
          <button key={p.label} class="preset-btn" onClick={p.apply}>
            {p.label}
          </button>
        ))}
      </div>
    </div>
  )
}
