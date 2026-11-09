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
  transaction_count: number
  rule_count: number
}

export type TextField = 'code' | 'description' | 'ref1' | 'ref2' | 'ref3'
/** Integer fields only support `equals` with an integer value. */
export type IntegerField = 'amount'
export type RuleField = TextField | IntegerField

/** Exactly one of `equals` / `includes` / `matches` is present. */
export type Condition =
  | { field: IntegerField; equals: number }
  | ({ field: TextField } & ({ equals: string } | { includes: string } | { matches: string }))

/** All `and` conditions must match, plus at least one `or` condition (if any). */
export interface Criteria {
  and?: Condition[]
  or?: Condition[]
}

export interface Rule {
  id: number
  category_id: number
  /** When set, the rule only applies to this account's transactions. */
  account_id: number | null
  criteria: Criteria
  priority: number
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
