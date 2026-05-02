import { signal, computed } from '@preact/signals'
import type { Account } from './types'

function startOfMonth(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-01`
}

function today(): string {
  return new Date().toISOString().slice(0, 10)
}

export const accounts      = signal<Account[]>([])

export const filterFrom    = signal(startOfMonth())
export const filterTo      = signal(today())
export const filterAccount = signal(localStorage.getItem('fintrack.account') ?? '')

export type Tab = 'summary' | 'transactions' | 'categories'
export const activeTab      = signal<Tab>('summary')
export const categoryFilter = signal('')
export const uncategorized  = signal(false)

export const currencyFractionDigits = computed(() => {
  const name = filterAccount.value
  if (!name) return 2
  const acc = accounts.value.find(a => a.name === name || a.number === name)
  if (!acc) return 2
  try {
    return new Intl.NumberFormat('en-US', { style: 'currency', currency: acc.currency })
      .resolvedOptions().maximumFractionDigits
  } catch {
    return 2
  }
})

export function drillIntoCategory(category: string | null, categoryId: number | null) {
  if (categoryId === null) {
    uncategorized.value  = true
    categoryFilter.value = ''
  } else {
    uncategorized.value  = false
    categoryFilter.value = category ?? ''
  }
  activeTab.value = 'transactions'
}
