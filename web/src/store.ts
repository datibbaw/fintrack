import { signal } from '@preact/signals'

function startOfMonth(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-01`
}

function today(): string {
  return new Date().toISOString().slice(0, 10)
}

export const filterFrom    = signal(startOfMonth())
export const filterTo      = signal(today())
export const filterAccount = signal('')

export type Tab = 'summary' | 'transactions'
export const activeTab      = signal<Tab>('summary')
export const categoryFilter = signal('')
export const uncategorized  = signal(false)

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
