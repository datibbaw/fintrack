import { signal, effect } from '@preact/signals'
import { useSignal } from '@preact/signals'
import { Fragment } from 'preact'
import { api } from '../api'
import { activeTab } from '../store'
import type { Category } from '../types'

type EditMode =
  | { type: 'idle' }
  | { type: 'edit'; id: number }
  | { type: 'add'; parentId: number | null }

// ── Module-level data state ────────────────────────────────────────────────────

const categories  = signal<Category[]>([])
const loading     = signal(false)
const fetchError  = signal<string | null>(null)
const refreshTick = signal(0)

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

function reload() { refreshTick.value++ }

// ── Drag guard variables ───────────────────────────────────────────────────────
// Plain variables (not signals) so that reading/writing them never triggers
// a re-render during the sensitive dragstart event.

let _dragFromHandle = false   // true only when pointer went down on a grip handle
let _dragCancelled  = false   // set in onDragEnd to invalidate any pending setTimeout

// ── Component ─────────────────────────────────────────────────────────────────

export function Categories() {
  const mode           = useSignal<EditMode>({ type: 'idle' })
  const inputValue     = useSignal('')
  const selectedParent = useSignal<number | null>(null)
  const saving         = useSignal(false)
  const saveError      = useSignal<string | null>(null)
  const draggingId     = useSignal<number | null>(null)
  const dropTargetId   = useSignal<number | 'top' | null>(null)

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
    if (!confirm(`Delete "${cat.name}"?\n\nTransactions assigned to it will become uncategorized.`)) return
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

  // ── Drag & drop ────────────────────────────────────────────────────────────

  function onHandlePointerDown() { _dragFromHandle = true }
  function onHandlePointerUp()   { _dragFromHandle = false }   // cancelled before dragstart

  function onDragStart(e: DragEvent, cat: Category) {
    // Cancel if the drag didn't originate from the grip handle
    if (!_dragFromHandle) { e.preventDefault(); return }
    _dragFromHandle = false
    _dragCancelled  = false
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'

    // Defer the signal write: setting it synchronously inside dragstart causes Preact
    // to re-render and insert the drop-zone row, which shifts the dragged element and
    // makes the browser cancel the operation.  Deferring past the event is safe.
    const id = cat.id
    setTimeout(() => {
      if (!_dragCancelled) draggingId.value = id
    }, 0)
  }

  function onDragEnd() {
    _dragCancelled     = true   // invalidate any pending setTimeout
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
    // Suppress spurious leaves when moving between child elements of the same row
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

  const editingId     = m.type === 'edit' ? m.id : null
  const parentOptions = topLevel.filter(c => c.id !== editingId)

  // ── Render helpers ─────────────────────────────────────────────────────────

  function renderEditFields() {
    return (
      <div class="cat-edit-fields">
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
          {parentOptions.map(p => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
      </div>
    )
  }

  function renderSaveCancelBtns() {
    return (
      <>
        <button class="cat-btn cat-btn-save" onClick={save} disabled={saving.value}>Save</button>
        <button class="cat-btn cat-btn-cancel" onClick={cancel}>Cancel</button>
      </>
    )
  }

  function renderHandle() {
    return (
      <span
        class="cat-drag-handle"
        onPointerDown={onHandlePointerDown}
        onPointerUp={onHandlePointerUp}
        title="Drag to reparent"
        aria-hidden="true"
      >
        <svg width="8" height="13" viewBox="0 0 8 13" fill="currentColor">
          <circle cx="2"  cy="1.5"  r="1.5"/>
          <circle cx="6"  cy="1.5"  r="1.5"/>
          <circle cx="2"  cy="6.5"  r="1.5"/>
          <circle cx="6"  cy="6.5"  r="1.5"/>
          <circle cx="2"  cy="11.5" r="1.5"/>
          <circle cx="6"  cy="11.5" r="1.5"/>
        </svg>
      </span>
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
              <th class="cat-col-actions" />
            </tr>
          </thead>
          <tbody>
            {/* "Promote to top-level" drop zone — appears only while dragging */}
            {isDragging && (
              <tr
                class={`cat-drop-zone ${dropTargetId.value === 'top' ? 'cat-drop-active' : ''}`}
                onDragEnter={e => onDragEnter(e, 'top')}
                onDragOver={e => e.preventDefault()}
                onDragLeave={onDragLeave}
                onDrop={e => void onDrop(e, 'top')}
              >
                <td colSpan={3}>Drop here to promote to top-level</td>
              </tr>
            )}

            {topLevel.map(cat => {
              const kids            = childrenOf(cat.id)
              const isEditingThis   = m.type === 'edit' && m.id === cat.id
              const isAddingSubHere = m.type === 'add' && m.parentId === cat.id
              const isDraggingThis  = draggingId.value === cat.id
              const isDropTarget    = dropTargetId.value === cat.id

              return (
                <Fragment key={cat.id}>
                  {/* Top-level row — valid drop target for reparenting */}
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
                    <td class="cat-col-handle">
                      {isIdle && renderHandle()}
                    </td>
                    <td>
                      {isEditingThis ? renderEditFields() : <strong>{cat.name}</strong>}
                    </td>
                    <td class="cat-col-actions">
                      {isEditingThis ? renderSaveCancelBtns() : (
                        <>
                          <button class="cat-btn cat-btn-sub" onClick={() => startAdd(cat.id)} disabled={!isIdle}>+ Sub</button>
                          <button class="cat-btn" onClick={() => startEdit(cat)} disabled={!isIdle}>Edit</button>
                          <button class="cat-btn cat-btn-delete" onClick={() => void remove(cat)} disabled={!isIdle}>Delete</button>
                        </>
                      )}
                    </td>
                  </tr>

                  {/* Sub-category rows — draggable but not drop targets */}
                  {kids.map(child => {
                    const isEditingChild  = m.type === 'edit' && m.id === child.id
                    const isDraggingChild = draggingId.value === child.id

                    return (
                      <tr
                        key={child.id}
                        class={`cat-row cat-row-sub ${isDraggingChild ? 'cat-dragging' : ''}`}
                        draggable={isIdle}
                        onDragStart={e => onDragStart(e, child)}
                        onDragEnd={onDragEnd}
                      >
                        <td class="cat-col-handle">
                          {isIdle && renderHandle()}
                        </td>
                        <td>
                          <span class="cat-sub-indent">└</span>
                          {isEditingChild ? renderEditFields() : <span>{child.name}</span>}
                        </td>
                        <td class="cat-col-actions">
                          {isEditingChild ? renderSaveCancelBtns() : (
                            <>
                              <button class="cat-btn" onClick={() => startEdit(child)} disabled={!isIdle}>Edit</button>
                              <button class="cat-btn cat-btn-delete" onClick={() => void remove(child)} disabled={!isIdle}>Delete</button>
                            </>
                          )}
                        </td>
                      </tr>
                    )
                  })}

                  {/* Inline add-sub row */}
                  {isAddingSubHere && (
                    <tr class="cat-row cat-row-sub cat-row-new">
                      <td class="cat-col-handle" />
                      <td>
                        <span class="cat-sub-indent">└</span>
                        <input
                          class="cat-input filter-input"
                          placeholder="Sub-category name"
                          value={inputValue.value}
                          onInput={e => { inputValue.value = (e.target as HTMLInputElement).value }}
                          onKeyDown={onKey}
                          autoFocus
                        />
                      </td>
                      <td class="cat-col-actions">{renderSaveCancelBtns()}</td>
                    </tr>
                  )}
                </Fragment>
              )
            })}

            {/* Inline add-top-level row */}
            {isAddingTop && (
              <tr class="cat-row cat-row-new">
                <td class="cat-col-handle" />
                <td>
                  <input
                    class="cat-input filter-input"
                    placeholder="Category name"
                    value={inputValue.value}
                    onInput={e => { inputValue.value = (e.target as HTMLInputElement).value }}
                    onKeyDown={onKey}
                    autoFocus
                  />
                </td>
                <td class="cat-col-actions">{renderSaveCancelBtns()}</td>
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
