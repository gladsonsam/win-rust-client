import { X, LayoutGrid } from 'lucide-react'
import { cn } from '../lib/utils'
import type { Agent } from '../lib/types'

interface Props {
  agents:     Record<string, Agent>
  selectedId: string | null
  view:       'overview' | 'detail'
  onSelect:   (id: string) => void
  onOverview: () => void
  open:       boolean
  onClose:    () => void
}

export function Sidebar({ agents, selectedId, view, onSelect, onOverview, open, onClose }: Props) {
  const list = Object.values(agents)

  return (
    <aside
      className={cn(
        // Dimensions & colour
        'w-52 flex-shrink-0 bg-surface border-r border-border flex flex-col',
        // Mobile: fixed overlay slide-in; Desktop: normal in-flow column.
        'fixed md:relative inset-y-0 left-0 z-30',
        'transition-transform duration-200 ease-in-out',
        'md:translate-x-0',
        open ? 'translate-x-0' : '-translate-x-full md:translate-x-0',
      )}
    >
      {/* ── Header ── */}
      <div className="flex items-center justify-between px-3.5 py-2.5
                      border-b border-border flex-shrink-0">

        {/* Overview link */}
        <button
          onClick={onOverview}
          className={cn(
            'flex items-center gap-1.5 text-[11px] uppercase tracking-widest font-semibold',
            'transition-colors',
            view === 'overview' ? 'text-accent' : 'text-muted hover:text-primary',
          )}
        >
          <LayoutGrid size={11} />
          Overview
          {list.length > 0 && (
            <span className="ml-0.5 text-[10px] bg-ok/20 text-ok rounded-full px-1.5">
              {list.length}
            </span>
          )}
        </button>

        {/* Close button (mobile only) */}
        <button
          onClick={onClose}
          className="md:hidden p-1 text-muted hover:text-primary transition-colors"
          aria-label="Close sidebar"
        >
          <X size={14} />
        </button>
      </div>

      {/* ── Section label ── */}
      <div className="px-3.5 pt-3 pb-1 flex-shrink-0">
        <span className="text-[10px] uppercase tracking-widest text-muted font-semibold">
          Agents
        </span>
      </div>

      {/* ── Agent list ── */}
      <div className="flex-1 overflow-y-auto">
        {list.length === 0 ? (
          <p className="px-3.5 py-3 text-xs text-muted italic">No agents online</p>
        ) : (
          list.map(agent => (
            <button
              key={agent.id}
              onClick={() => onSelect(agent.id)}
              className={cn(
                'w-full text-left px-3.5 py-2.5 flex flex-col gap-0.5',
                'border-l-2 transition-colors',
                selectedId === agent.id && view === 'detail'
                  ? 'border-accent bg-accent/[.08]'
                  : 'border-transparent hover:bg-white/[.03]',
              )}
            >
              <span className="text-[13px] font-medium flex items-center gap-1.5 truncate">
                <span className="w-1.5 h-1.5 rounded-full bg-ok flex-shrink-0 animate-pulse" />
                {agent.name}
              </span>
              <span className="text-[11px] text-muted tabular-nums">
                {new Date(agent.connected_at).toLocaleTimeString()}
              </span>
            </button>
          ))
        )}
      </div>
    </aside>
  )
}
