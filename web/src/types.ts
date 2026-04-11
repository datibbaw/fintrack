export interface Account {
  id: number
  name: string
  number: string
  bank: string
  currency: string
}

export interface Category {
  id: number
  name: string
  parent_id: number | null
  parent: string | null
}

export interface SummaryRow {
  category: string
  category_id: number | null
  parent: string | null
  parent_id: number | null
  debit: number
  credit: number
  net: number
  count: number
}

export interface SummaryResponse {
  rows: SummaryRow[]
  total_debit: number
  total_credit: number
  total_net: number
}

export interface Transaction {
  id: number
  date: string
  code: string
  description: string
  ref1: string
  ref2: string
  ref3: string
  status: string
  debit: number | null
  credit: number | null
  category: string | null
  category_id: number | null
  account: string
  account_id: number
}

export interface TransactionsResponse {
  rows: Transaction[]
  total: number
}

export interface Filters {
  from: string
  to: string
  account: string
}
