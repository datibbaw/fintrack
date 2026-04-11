import type { Account, Category, SummaryResponse, TransactionsResponse } from './types'

async function get<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(path, window.location.href)
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v !== '' && v !== undefined) url.searchParams.set(k, v)
    }
  }
  const res = await fetch(url.toString())
  if (!res.ok) throw new Error(`API error ${res.status}: ${await res.text()}`)
  return res.json()
}

export const api = {
  accounts(): Promise<Account[]> {
    return get('/api/accounts')
  },

  categories(): Promise<Category[]> {
    return get('/api/categories')
  },

  summary(params: {
    from?: string
    to?: string
    account?: string
  }): Promise<SummaryResponse> {
    return get('/api/summary', {
      from: params.from ?? '',
      to: params.to ?? '',
      account: params.account ?? '',
    })
  },

  transactions(params: {
    from?: string
    to?: string
    category?: string
    account?: string
    uncategorized?: boolean
    limit?: number
    offset?: number
  }): Promise<TransactionsResponse> {
    return get('/api/transactions', {
      from: params.from ?? '',
      to: params.to ?? '',
      category: params.category ?? '',
      account: params.account ?? '',
      uncategorized: params.uncategorized ? 'true' : '',
      limit: params.limit?.toString() ?? '',
      offset: params.offset?.toString() ?? '',
    })
  },
}
