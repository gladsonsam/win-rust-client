/**
 * Reusable data table with:
 *  - Global search (client-side, highlights matches)
 *  - Column-level sort (click header to toggle asc/desc)
 *  - Pagination
 *  - Optional per-column custom renderers
 *  - Loading / empty states
 */
import { useState, useMemo, type ReactNode } from 'react'
import {
  Search, ArrowUpDown, ArrowUp, ArrowDown,
  ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight,
  Loader2,
} from 'lucide-react'
import { cn } from '../lib/utils'

// ── Column definition ─────────────────────────────────────────────────────────

export interface Column {
  /** Must match a key in the data rows. */
  key:          string
  label:        string
  /** Default: true – include this column when searching. */
  searchable?:  boolean
  /** Default: true – clicking the header sorts by this column. */
  sortable?:    boolean
  /** Custom cell renderer. */
  render?:      (value: unknown, row: Row) => ReactNode
  /** Extra Tailwind classes for every cell in this column. */
  className?:   string
  /** Header Tailwind classes. */
  headerClass?: string
}

type Row = Record<string, unknown>
type SortDir = 'asc' | 'desc'

// ── Props ─────────────────────────────────────────────────────────────────────

interface DataTableProps {
  data:              Row[]
  columns:           Column[]
  searchPlaceholder?: string
  pageSize?:          number
  emptyMessage?:      string
  isLoading?:         boolean
  /** Prepend this node to the toolbar (right of the search bar). */
  toolbarRight?:      ReactNode
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DataTable({
  data,
  columns,
  searchPlaceholder = 'Search…',
  pageSize = 25,
  emptyMessage = 'No records found',
  isLoading = false,
  toolbarRight,
}: DataTableProps) {
  const [search,  setSearch]  = useState('')
  const [page,    setPage]    = useState(0)
  const [sortKey, setSortKey] = useState<string | null>(null)
  const [sortDir, setSortDir] = useState<SortDir>('desc')

  // Columns that participate in the global search.
  const searchableCols = useMemo(
    () => columns.filter(c => c.searchable !== false).map(c => c.key),
    [columns],
  )

  // 1. Filter
  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return data
    return data.filter(row =>
      searchableCols.some(k => String(row[k] ?? '').toLowerCase().includes(q)),
    )
  }, [data, search, searchableCols])

  // 2. Sort
  const sorted = useMemo(() => {
    if (!sortKey) return filtered
    return [...filtered].sort((a, b) => {
      const cmp = String(a[sortKey] ?? '').localeCompare(
        String(b[sortKey] ?? ''),
        undefined,
        { numeric: true, sensitivity: 'base' },
      )
      return sortDir === 'asc' ? cmp : -cmp
    })
  }, [filtered, sortKey, sortDir])

  // 3. Paginate
  const totalPages = Math.max(1, Math.ceil(sorted.length / pageSize))
  const safePage   = Math.min(page, totalPages - 1)
  const rows       = sorted.slice(safePage * pageSize, (safePage + 1) * pageSize)

  // Handlers
  const toggleSort = (key: string) => {
    if (sortKey === key) {
      setSortDir(d => (d === 'asc' ? 'desc' : 'asc'))
    } else {
      setSortKey(key)
      setSortDir('desc')
    }
    setPage(0)
  }

  const handleSearch = (q: string) => {
    setSearch(q)
    setPage(0)
  }

  // ── Render ─────────────────────────────────────────────────────────────────

  return (
    <div className="flex flex-col gap-3">
      {/* ── Toolbar ── */}
      <div className="flex items-center gap-2">
        <div className="relative flex-1 max-w-sm">
          <Search
            size={13}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-muted pointer-events-none"
          />
          <input
            type="text"
            value={search}
            onChange={e => handleSearch(e.target.value)}
            placeholder={searchPlaceholder}
            className="w-full bg-surface border border-border rounded-md
                       pl-8 pr-3 py-1.5 text-sm text-primary placeholder-muted
                       focus:outline-none focus:border-accent transition-colors"
          />
        </div>
        {toolbarRight}
        <span className="ml-auto text-xs text-muted tabular-nums">
          {filtered.length} row{filtered.length !== 1 ? 's' : ''}
        </span>
      </div>

      {/* ── Table ── */}
      <div className="bg-surface border border-border rounded-md overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                {columns.map(col => {
                  const isSorted = sortKey === col.key
                  const canSort  = col.sortable !== false
                  return (
                    <th
                      key={col.key}
                      onClick={() => canSort && toggleSort(col.key)}
                      className={cn(
                        'px-3 py-2.5 text-left text-[11px] uppercase tracking-wide',
                        'text-muted font-semibold whitespace-nowrap',
                        canSort && 'cursor-pointer hover:text-primary select-none',
                        col.headerClass,
                      )}
                    >
                      <span className="flex items-center gap-1">
                        {col.label}
                        {canSort && (
                          isSorted
                            ? sortDir === 'asc'
                              ? <ArrowUp size={10} className="text-accent" />
                              : <ArrowDown size={10} className="text-accent" />
                            : <ArrowUpDown size={10} className="opacity-30" />
                        )}
                      </span>
                    </th>
                  )
                })}
              </tr>
            </thead>

            <tbody>
              {isLoading ? (
                <tr>
                  <td colSpan={columns.length} className="py-12 text-center text-muted">
                    <Loader2 size={18} className="animate-spin inline-block mr-2" />
                    Loading…
                  </td>
                </tr>
              ) : rows.length === 0 ? (
                <tr>
                  <td colSpan={columns.length} className="py-12 text-center text-muted">
                    {search.trim()
                      ? <>No results for <span className="text-primary">"{search}"</span></>
                      : emptyMessage}
                  </td>
                </tr>
              ) : (
                rows.map((row, i) => (
                  <tr
                    key={i}
                    className="border-b border-border last:border-0
                               hover:bg-white/[.02] transition-colors"
                  >
                    {columns.map(col => (
                      <td
                        key={col.key}
                        className={cn('px-3 py-2 text-primary align-top', col.className)}
                      >
                        {col.render
                          ? col.render(row[col.key], row)
                          : <span className="truncate max-w-xs block">{String(row[col.key] ?? '')}</span>}
                      </td>
                    ))}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* ── Pagination ── */}
      {totalPages > 1 && (
        <div className="flex items-center justify-end gap-1 text-xs text-muted">
          <PBtn onClick={() => setPage(0)}           disabled={safePage === 0}>
            <ChevronsLeft size={12} />
          </PBtn>
          <PBtn onClick={() => setPage(p => p - 1)} disabled={safePage === 0}>
            <ChevronLeft size={12} />
          </PBtn>
          <span className="px-2 tabular-nums">
            {safePage + 1} / {totalPages}
          </span>
          <PBtn onClick={() => setPage(p => p + 1)} disabled={safePage >= totalPages - 1}>
            <ChevronRight size={12} />
          </PBtn>
          <PBtn onClick={() => setPage(totalPages - 1)} disabled={safePage >= totalPages - 1}>
            <ChevronsRight size={12} />
          </PBtn>
        </div>
      )}
    </div>
  )
}

// ── Tiny pagination button ────────────────────────────────────────────────────

function PBtn({
  children, onClick, disabled,
}: { children: ReactNode; onClick: () => void; disabled: boolean }) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="p-1.5 rounded hover:bg-border disabled:opacity-25
                 disabled:cursor-not-allowed transition-colors"
    >
      {children}
    </button>
  )
}
