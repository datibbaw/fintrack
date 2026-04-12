import { signal, effect } from '@preact/signals'
import { useSignal } from '@preact/signals'
import { Fragment } from 'preact'
import Prism from 'prismjs'
import 'prismjs/components/prism-regex'
import { api } from '../api'
import { activeTab } from '../store'
import type { Category, Rule } from '../types'

// ── Regex syntax highlight ─────────────────────────────────────────────────────

function RegexHighlight({ pattern }: { pattern: string }) {
  const html = Prism.highlight(pattern, Prism.languages['regex'], 'regex')
  return <code class="regex-hl" dangerouslySetInnerHTML={{ __html: html }} />
}

type EditMode =
  | { type: 'idle' }
  | { type: 'edit'; id: number }
  | { type: 'add'; parentId: number | null }

// ── Module-level data & rules cache ───────────────────────────────────────────

const categories  = signal<Category[]>([])
const loading     = signal(false)
const fetchError  = signal<string | null>(null)
const refreshTick = signal(0)
const rulesCache  = signal<Record<number, Rule[]>>({})

effect(() => {
  if (activeTab.value !== 'categories') return
  refreshTick.value

  loading.value    = true
  fetchError.value = null
  api.categories()
    .then(cats => { categories.value = cats })
    .catch(e   => { fetchError.value = String(e) })
    .finally(  () => { loading.value = false })
})

function reload() {
  rulesCache.value = {}   // invalidate cached rules so panels re-fetch
  refreshTick.value++
}

// ── Drag guard variables ───────────────────────────────────────────────────────

let _dragFromHandle = false
let _dragCancelled  = false

// ── Icons ──────────────────────────────────────────────────────────────────────

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

function IconAddSub() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"
      aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2"/>
      <line x1="12" y1="8" x2="12" y2="16"/>
      <line x1="8"  y1="12" x2="16" y2="12"/>
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

function IconClose() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" stroke-width="2.5" stroke-linecap="round"
      aria-hidden="true">
      <line x1="18" y1="6"  x2="6"  y2="18"/>
      <line x1="6"  y1="6"  x2="18" y2="18"/>
    </svg>
  )
}

// ── Component ─────────────────────────────────────────────────────────────────

export function Categories() {
  const mode           = useSignal<EditMode>({ type: 'idle' })
  const inputValue     = useSignal('')
  const selectedParent = useSignal<number | null>(null)
  const saving         = useSignal(false)
  const saveError      = useSignal<string | null>(null)
  const draggingId     = useSignal<number | null>(null)
  const dropTargetId   = useSignal<number | 'top' | null>(null)
  const openRulesId    = useSignal<number | null>(null)
  const rulesLoading   = useSignal(false)

  const topLevel = categories.value.filter(c => c.parent_id === null)

  function childrenOf(id: number) {
    return categories.value.filter(c => c.parent_id === id)
  }

  // ── Edit / add ─────────────────────────────────────────────────────────────

  function startEdit(cat: Category) {
    mode.value           = { type: 'edit', id: cat.id }
    inputValue.value     = cat.name
    selectedParent.value = cat.parent_id
    saveError.value      = null
  }

  function startAdd(parentId: number | null) {
    mode.value           = { type: 'add', parentId }
    inputValue.value     = ''
    selectedParent.value = parentId
    saveError.value      = null
  }

  function cancel() {
    mode.value      = { type: 'idle' }
    saveError.value = null
  }

  async function save() {
    const name = inputValue.value.trim()
    if (!name || saving.value) return
    saving.value    = true
    saveError.value = null
    const m = mode.value
    try {
      if (m.type === 'edit') {
        await api.updateCategory(m.id, name, selectedParent.value)
      } else if (m.type === 'add') {
        await api.createCategory(name, selectedParent.value)
      }
      mode.value = { type: 'idle' }
      reload()
    } catch (e: any) {
      saveError.value = e.message ?? String(e)
    } finally {
      saving.value = false
    }
  }

  async function remove(cat: Category) {
    const txCount = cat.transaction_count
    const txNote  = txCount > 0
      ? `\n\n⚠ ${txCount} transaction${txCount === 1 ? '' : 's'} assigned to it will become uncategorized.`
      : ''
    if (!confirm(`Delete "${cat.name}"?${txNote}`)) return
    saveError.value = null
    try {
      await api.deleteCategory(cat.id)
      reload()
    } catch (e: any) {
      saveError.value = e.message ?? String(e)
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter')  void save()
    if (e.key === 'Escape') cancel()
  }

  // ── Rules panel ────────────────────────────────────────────────────────────

  async function toggleRules(catId: number) {
    if (openRulesId.value === catId) { openRulesId.value = null; return }
    openRulesId.value = catId
    if (rulesCache.value[catId] !== undefined) return   // already cached
    rulesLoading.value = true
    try {
      const rules = await api.getCategoryRules(catId)
      rulesCache.value = { ...rulesCache.value, [catId]: rules }
    } catch (e: any) {
      saveError.value = e.message ?? String(e)
      openRulesId.value = null
    } finally {
      rulesLoading.value = false
    }
  }

  function renderRulesPanel(cat: Category) {
    const rules     = rulesCache.value[cat.id]
    const isLoading = rulesLoading.value && openRulesId.value === cat.id
    return (
      <div class="cat-rules-panel">
        <div class="cat-rules-header">
          <span class="cat-rules-title">Rules</span>
          <button class="cat-icon-btn" title="Close" onClick={() => { openRulesId.value = null }}>
            <IconClose />
          </button>
        </div>
        {isLoading ? (
          <p class="cat-rules-state">Loading…</p>
        ) : !rules || rules.length === 0 ? (
          <p class="cat-rules-state">No rules defined for this category.</p>
        ) : (
          <table class="data-table cat-rules-table">
            <thead>
              <tr>
                <th>Field</th>
                <th>Pattern</th>
                <th class="col-number">Priority</th>
              </tr>
            </thead>
            <tbody>
              {rules.map(rule => (
                <tr key={rule.id}>
                  <td class="mono">{rule.field}</td>
                  <td><RegexHighlight pattern={rule.pattern} /></td>
                  <td class="mono col-number">{rule.priority}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    )
  }

  // ── Drag & drop ────────────────────────────────────────────────────────────

  function onHandlePointerDown() { _dragFromHandle = true }
  function onHandlePointerUp()   { _dragFromHandle = false }

  function onDragStart(e: DragEvent, cat: Category) {
    if (!_dragFromHandle) { e.preventDefault(); return }
    _dragFromHandle = false
    _dragCancelled  = false
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
    const id = cat.id
    setTimeout(() => {
      if (!_dragCancelled) draggingId.value = id
    }, 0)
  }

  function onDragEnd() {
    _dragCancelled     = true
    draggingId.value   = null
    dropTargetId.value = null
  }

  function onDragEnter(e: DragEvent, targetId: number | 'top') {
    if (draggingId.value === null) return
    if (targetId !== 'top' && draggingId.value === targetId) return
    e.preventDefault()
    dropTargetId.value = targetId
  }

  function onDragLeave(e: DragEvent) {
    const rt = e.relatedTarget as Element | null
    const ct = e.currentTarget as Element
    if (!rt || !ct.contains(rt)) dropTargetId.value = null
  }

  async function onDrop(e: DragEvent, targetId: number | 'top') {
    e.preventDefault()
    const id = draggingId.value
    draggingId.value   = null
    dropTargetId.value = null
    if (id === null) return

    const cat = categories.value.find(c => c.id === id)
    if (!cat) return

    const newParentId: number | null = targetId === 'top' ? null : targetId
    if (newParentId === cat.parent_id) return
    if (newParentId === cat.id) return

    saveError.value = null
    try {
      await api.updateCategory(id, cat.name, newParentId)
      reload()
    } catch (e: any) {
      saveError.value = e.message ?? String(e)
    }
  }

  // ── Derived values ─────────────────────────────────────────────────────────

  const m           = mode.value
  const isIdle      = m.type === 'idle'
  const isAddingTop = m.type === 'add' && m.parentId === null
  const isDragging  = draggingId.value !== null
  const editingId   = m.type === 'edit' ? m.id : null
  const parentOpts  = topLevel.filter(c => c.id !== editingId)

  // ── Render helpers ─────────────────────────────────────────────────────────

  function renderHandle() {
    return (
      <span class="cat-drag-handle" title="Drag to reparent"
        onPointerDown={onHandlePointerDown} onPointerUp={onHandlePointerUp}
        aria-hidden="true">
        <svg width="8" height="13" viewBox="0 0 8 13" fill="currentColor">
          <circle cx="2"  cy="1.5"  r="1.5"/><circle cx="6"  cy="1.5"  r="1.5"/>
          <circle cx="2"  cy="6.5"  r="1.5"/><circle cx="6"  cy="6.5"  r="1.5"/>
          <circle cx="2"  cy="11.5" r="1.5"/><circle cx="6"  cy="11.5" r="1.5"/>
        </svg>
      </span>
    )
  }

  function renderRulesBtn(cat: Category) {
    const isOpen   = openRulesId.value === cat.id
    const hasRules = cat.rule_count > 0
    return (
      <button
        class={`cat-rules-btn ${hasRules ? 'cat-rules-btn-has' : 'cat-rules-btn-none'} ${isOpen ? 'cat-rules-btn-open' : ''}`}
        onClick={() => void toggleRules(cat.id)}
      >
        {cat.rule_count} {cat.rule_count === 1 ? 'rule' : 'rules'}
      </button>
    )
  }

  function renderEditRow() {
    return (
      <div class="cat-edit-row">
        <input
          class="cat-input filter-input"
          value={inputValue.value}
          onInput={e => { inputValue.value = (e.target as HTMLInputElement).value }}
          onKeyDown={onKey}
          autoFocus
        />
        <select
          class="cat-parent-select filter-input"
          value={selectedParent.value ?? ''}
          onChange={e => {
            const v = (e.target as HTMLSelectElement).value
            selectedParent.value = v === '' ? null : Number(v)
          }}
        >
          <option value="">None (top-level)</option>
          {parentOpts.map(p => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        <div class="cat-edit-btns">
          <button class="cat-btn cat-btn-save" onClick={save} disabled={saving.value}>Save</button>
          <button class="cat-btn cat-btn-cancel" onClick={cancel}>Cancel</button>
        </div>
      </div>
    )
  }

  function renderIconBtns(cat: Category, isTopLevel: boolean) {
    return (
      <div class="cat-icon-btns">
        <button class="cat-icon-btn" title="Edit" onClick={() => startEdit(cat)} disabled={!isIdle}>
          <IconEdit />
        </button>
        {isTopLevel && (
          <button class="cat-icon-btn" title="Add sub-category" onClick={() => startAdd(cat.id)} disabled={!isIdle}>
            <IconAddSub />
          </button>
        )}
        <button class="cat-icon-btn cat-icon-btn-danger" title="Delete" onClick={() => void remove(cat)} disabled={!isIdle}>
          <IconTrash />
        </button>
      </div>
    )
  }

  // ── Early returns ──────────────────────────────────────────────────────────

  if (loading.value)    return <div class="state-message">Loading…</div>
  if (fetchError.value) return <div class="state-message error">Error: {fetchError.value}</div>

  // ── Main render ────────────────────────────────────────────────────────────

  return (
    <div class="categories">
      <div class="categories-header">
        <h2 class="section-title">Categories</h2>
        <button class="preset-btn cat-add-btn" onClick={() => startAdd(null)} disabled={!isIdle}>
          + Add Category
        </button>
      </div>

      {saveError.value && <p class="state-message error">{saveError.value}</p>}

      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th class="cat-col-handle" />
              <th>Name</th>
            </tr>
          </thead>
          <tbody>
            {isDragging && (
              <tr
                class={`cat-drop-zone ${dropTargetId.value === 'top' ? 'cat-drop-active' : ''}`}
                onDragEnter={e => onDragEnter(e, 'top')}
                onDragOver={e => e.preventDefault()}
                onDragLeave={onDragLeave}
                onDrop={e => void onDrop(e, 'top')}
              >
                <td colSpan={2}>Drop here to promote to top-level</td>
              </tr>
            )}

            {topLevel.map(cat => {
              const kids            = childrenOf(cat.id)
              const isEditingThis   = m.type === 'edit' && m.id === cat.id
              const isAddingSubHere = m.type === 'add' && m.parentId === cat.id
              const isDraggingThis  = draggingId.value === cat.id
              const isDropTarget    = dropTargetId.value === cat.id
              const isRulesOpen     = openRulesId.value === cat.id

              return (
                <Fragment key={cat.id}>
                  {/* Top-level category row */}
                  <tr
                    class={`cat-row ${isDraggingThis ? 'cat-dragging' : ''} ${isDropTarget ? 'cat-drop-over' : ''}`}
                    draggable={isIdle}
                    onDragStart={e => onDragStart(e, cat)}
                    onDragEnd={onDragEnd}
                    onDragEnter={e => onDragEnter(e, cat.id)}
                    onDragOver={e => e.preventDefault()}
                    onDragLeave={onDragLeave}
                    onDrop={e => void onDrop(e, cat.id)}
                  >
                    <td class="cat-col-handle">{isIdle && renderHandle()}</td>
                    <td>
                      {isEditingThis ? renderEditRow() : (
                        <div class="cat-name-row">
                          <strong class="cat-name-text">{cat.name}</strong>
                          {renderRulesBtn(cat)}
                          {renderIconBtns(cat, true)}
                        </div>
                      )}
                    </td>
                  </tr>

                  {/* Rules panel for top-level category */}
                  {isRulesOpen && (
                    <tr class="cat-rules-panel-row">
                      <td colSpan={2}>{renderRulesPanel(cat)}</td>
                    </tr>
                  )}

                  {/* Sub-category rows */}
                  {kids.map(child => {
                    const isEditingChild  = m.type === 'edit' && m.id === child.id
                    const isDraggingChild = draggingId.value === child.id
                    const isChildRulesOpen = openRulesId.value === child.id
                    return (
                      <Fragment key={child.id}>
                        <tr
                          class={`cat-row cat-row-sub ${isDraggingChild ? 'cat-dragging' : ''}`}
                          draggable={isIdle}
                          onDragStart={e => onDragStart(e, child)}
                          onDragEnd={onDragEnd}
                        >
                          <td class="cat-col-handle">{isIdle && renderHandle()}</td>
                          <td>
                            {isEditingChild ? (
                              <div class="cat-sub-edit">
                                <span class="cat-sub-indent">└</span>
                                {renderEditRow()}
                              </div>
                            ) : (
                              <div class="cat-name-row">
                                <span class="cat-name-text">
                                  <span class="cat-sub-indent">└</span>{child.name}
                                </span>
                                {renderRulesBtn(child)}
                                {renderIconBtns(child, false)}
                              </div>
                            )}
                          </td>
                        </tr>

                        {/* Rules panel for sub-category */}
                        {isChildRulesOpen && (
                          <tr class="cat-rules-panel-row">
                            <td colSpan={2}>{renderRulesPanel(child)}</td>
                          </tr>
                        )}
                      </Fragment>
                    )
                  })}

                  {/* Inline add-sub row */}
                  {isAddingSubHere && (
                    <tr class="cat-row cat-row-sub cat-row-new">
                      <td class="cat-col-handle" />
                      <td>
                        <div class="cat-sub-edit">
                          <span class="cat-sub-indent">└</span>
                          <div class="cat-edit-row">
                            <input
                              class="cat-input filter-input"
                              placeholder="Sub-category name"
                              value={inputValue.value}
                              onInput={e => { inputValue.value = (e.target as HTMLInputElement).value }}
                              onKeyDown={onKey}
                              autoFocus
                            />
                            <div class="cat-edit-btns">
                              <button class="cat-btn cat-btn-save" onClick={save} disabled={saving.value}>Save</button>
                              <button class="cat-btn cat-btn-cancel" onClick={cancel}>Cancel</button>
                            </div>
                          </div>
                        </div>
                      </td>
                    </tr>
                  )}
                </Fragment>
              )
            })}

            {isAddingTop && (
              <tr class="cat-row cat-row-new">
                <td class="cat-col-handle" />
                <td>
                  <div class="cat-edit-row">
                    <input
                      class="cat-input filter-input"
                      placeholder="Category name"
                      value={inputValue.value}
                      onInput={e => { inputValue.value = (e.target as HTMLInputElement).value }}
                      onKeyDown={onKey}
                      autoFocus
                    />
                    <div class="cat-edit-btns">
                      <button class="cat-btn cat-btn-save" onClick={save} disabled={saving.value}>Save</button>
                      <button class="cat-btn cat-btn-cancel" onClick={cancel}>Cancel</button>
                    </div>
                  </div>
                </td>
              </tr>
            )}
          </tbody>
        </table>

        {categories.value.length === 0 && !isAddingTop && (
          <p class="state-message">No categories yet. Click "+ Add Category" to get started.</p>
        )}
      </div>
    </div>
  )
}
