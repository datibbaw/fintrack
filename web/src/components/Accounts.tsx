import { signal, effect } from '@preact/signals'
import { useSignal } from '@preact/signals'
import { useRef, useEffect } from 'preact/hooks'
import { api } from '../api'
import { activeTab, accounts } from '../store'
import type { Account } from '../types'

// ── Module-level data ─────────────────────────────────────────────────────────

const currencies  = signal<string[]>([])
const loading     = signal(false)
const fetchError  = signal<string | null>(null)
const refreshTick = signal(0)

effect(() => {
  if (activeTab.value !== 'accounts') return
  refreshTick.value

  loading.value    = true
  fetchError.value = null
  Promise.all([api.accounts(), api.currencies()])
    .then(([accs, curs]) => {
      accounts.value   = accs   // writes to shared store signal
      currencies.value = curs
    })
    .catch(e => { fetchError.value = String(e) })
    .finally(() => { loading.value = false })
})

function reload() { refreshTick.value++ }

// ── Icons ─────────────────────────────────────────────────────────────────────

function IconEdit() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
      aria-hidden="true">
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
    </svg>
  )
}

function IconTrash() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
      aria-hidden="true">
      <polyline points="3 6 5 6 21 6"/>
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/>
      <line x1="10" y1="11" x2="10" y2="17"/>
      <line x1="14" y1="11" x2="14" y2="17"/>
    </svg>
  )
}

// ── Form modal ────────────────────────────────────────────────────────────────

type FormMode =
  | { type: 'add' }
  | { type: 'edit'; account: Account }

interface FormProps {
  mode: FormMode
  currencies: string[]
  onSave: () => void
  onCancel: () => void
}

function AccountForm({ mode, currencies: currencyList, onSave, onCancel }: FormProps) {
  const initial = mode.type === 'edit' ? mode.account : { name: '', number: '', bank: '', currency: 'SGD' }
  const name     = useSignal(initial.name)
  const number   = useSignal(initial.number)
  const bank     = useSignal(initial.bank)
  const currency = useSignal(initial.currency)
  const saving   = useSignal(false)
  const error    = useSignal<string | null>(null)
  const nameRef  = useRef<HTMLInputElement>(null)

  useEffect(() => { nameRef.current?.focus() }, [])

  const currencyLocked = mode.type === 'edit' && mode.account.transaction_count > 0

  async function submit(e: Event) {
    e.preventDefault()
    if (saving.value) return
    const trimmedName     = name.value.trim()
    const trimmedNumber   = number.value.trim()
    const trimmedBank     = bank.value.trim()
    const trimmedCurrency = currency.value.trim().toUpperCase()
    if (!trimmedName || !trimmedNumber) {
      error.value = 'Name and account number are required.'
      return
    }
    saving.value = true
    error.value  = null
    const data = {
      name:     trimmedName,
      number:   trimmedNumber,
      bank:     trimmedBank,
      currency: trimmedCurrency,
    }
    try {
      if (mode.type === 'edit') {
        await api.updateAccount(mode.account.id, data)
      } else {
        await api.createAccount(data)
      }
      onSave()
    } catch (e: any) {
      error.value = e.message ?? String(e)
    } finally {
      saving.value = false
    }
  }

  const title = mode.type === 'add' ? 'Add Account' : 'Edit Account'

  return (
    <div class="modal-backdrop" onClick={e => { if (e.target === e.currentTarget) onCancel() }}>
      <div class="modal">
        <h3 class="modal-title">{title}</h3>
        <form class="modal-form" onSubmit={submit}>
          <label class="modal-label" for="acc-name">
            <span>Name <span class="required-mark">*</span></span>
            <input id="acc-name" name="name" class="filter-input" required
              ref={nameRef} value={name.value}
              onInput={e => { name.value = (e.target as HTMLInputElement).value }} />
          </label>
          <label class="modal-label" for="acc-number">
            <span>Account number <span class="required-mark">*</span></span>
            <input id="acc-number" name="number" class="filter-input" required value={number.value}
              onInput={e => { number.value = (e.target as HTMLInputElement).value }} />
          </label>
          <label class="modal-label" for="acc-bank">
            Bank
            <input id="acc-bank" name="bank" class="filter-input" value={bank.value}
              onInput={e => { bank.value = (e.target as HTMLInputElement).value }} />
          </label>
          <label class="modal-label" for="acc-currency">
            <span>Currency <span class="required-mark">*</span></span>
            {currencyLocked ? (
              <span class="modal-currency-locked" title="Currency cannot be changed while transactions exist">
                <input id="acc-currency" name="currency" class="filter-input" value={currency.value} disabled />
                <span class="modal-lock-hint">locked — account has transactions</span>
              </span>
            ) : (
              <>
                <input id="acc-currency" name="currency" class="filter-input" required value={currency.value}
                  list="currency-list"
                  onInput={e => { currency.value = (e.target as HTMLInputElement).value }} />
                <datalist id="currency-list">
                  {currencyList.map(c => <option key={c} value={c} />)}
                </datalist>
              </>
            )}
          </label>
          {error.value && <p class="modal-error">{error.value}</p>}
          <div class="modal-btns">
            <button type="submit" class="cat-btn cat-btn-save" disabled={saving.value}>
              {saving.value ? 'Saving…' : 'Save'}
            </button>
            <button type="button" class="cat-btn cat-btn-cancel" onClick={onCancel}>Cancel</button>
          </div>
        </form>
      </div>
    </div>
  )
}

// ── Component ─────────────────────────────────────────────────────────────────

export function Accounts() {
  const formMode  = useSignal<FormMode | null>(null)
  const saveError = useSignal<string | null>(null)

  function startAdd() {
    formMode.value  = { type: 'add' }
    saveError.value = null
  }

  function startEdit(acc: Account) {
    formMode.value  = { type: 'edit', account: acc }
    saveError.value = null
  }

  function closeForm() { formMode.value = null }

  function onSaved() {
    formMode.value = null
    reload()
  }

  async function remove(acc: Account) {
    const txCount = acc.transaction_count
    const txNote  = txCount > 0
      ? `\n\n⚠ ${txCount} transaction${txCount === 1 ? '' : 's'} will be permanently deleted.`
      : ''
    if (!confirm(`Delete account "${acc.name}"?${txNote}`)) return
    saveError.value = null
    try {
      await api.deleteAccount(acc.id)
      reload()
    } catch (e: any) {
      saveError.value = e.message ?? String(e)
    }
  }

  if (loading.value)    return <div class="state-message">Loading…</div>
  if (fetchError.value) return <div class="state-message error">Error: {fetchError.value}</div>

  return (
    <div class="categories">
      <div class="categories-header">
        <h2 class="section-title">Accounts</h2>
        <button class="preset-btn cat-add-btn" onClick={startAdd}>+ Add Account</button>
      </div>

      {saveError.value && <p class="state-message error">{saveError.value}</p>}

      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Number</th>
              <th>Bank</th>
              <th>Currency</th>
              <th class="col-number">Transactions</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {accounts.value.map(acc => (
              <tr key={acc.id}>
                <td>{acc.name}</td>
                <td class="mono">{acc.number}</td>
                <td>{acc.bank}</td>
                <td class="mono">{acc.currency}</td>
                <td class="col-number">{acc.transaction_count}</td>
                <td>
                  <div class="cat-icon-btns cat-icon-btns-always">
                    <button class="cat-icon-btn" title="Edit" onClick={() => startEdit(acc)}>
                      <IconEdit />
                    </button>
                    <button class="cat-icon-btn cat-icon-btn-danger" title="Delete" onClick={() => void remove(acc)}>
                      <IconTrash />
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>

        {accounts.value.length === 0 && (
          <p class="state-message">No accounts yet. Click "+ Add Account" to get started.</p>
        )}
      </div>

      {formMode.value && (
        <AccountForm
          mode={formMode.value}
          currencies={currencies.value}
          onSave={onSaved}
          onCancel={closeForm}
        />
      )}
    </div>
  )
}
