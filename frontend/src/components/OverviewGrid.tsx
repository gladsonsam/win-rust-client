/**
 * Overview grid – one card per connected agent.
 *
 * Each card seeds itself from the REST API on mount so historical data shows
 * immediately, then live WebSocket events in `liveStatus` take over.
 */
import { useEffect, useState } from 'react'
import { Monitor, Globe, Layout, Clock, Zap, Moon } from 'lucide-react'
import { cn } from '../lib/utils'
import { api } from '../lib/api'
import type { Agent, AgentLiveStatus } from '../lib/types'

// ── Props ─────────────────────────────────────────────────────────────────────

interface Props {
  agents:     Record<string, Agent>
  liveStatus: Record<string, AgentLiveStatus>
  onOpen:     (id: string) => void
}

// ── Agent card ────────────────────────────────────────────────────────────────

function AgentCard({
  agent,
  liveStatus,
  onOpen,
}: {
  agent:      Agent
  liveStatus?: AgentLiveStatus
  onOpen:     () => void
}) {
  // Seed from the DB on mount; live events override as they arrive.
  const [seed, setSeed] = useState<AgentLiveStatus | null>(null)

  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        const [wins, urls, acts] = await Promise.allSettled([
          api.windows(agent.id,  { limit: 1 }),
          api.urls(agent.id,     { limit: 1 }),
          api.activity(agent.id, { limit: 1 }),
        ])
        if (cancelled) return
        const patch: AgentLiveStatus = {}
        if (wins.status === 'fulfilled' && wins.value.rows[0]) {
          patch.window = wins.value.rows[0].title
          patch.app    = wins.value.rows[0].app
        }
        if (urls.status === 'fulfilled' && urls.value.rows[0]) {
          patch.url = urls.value.rows[0].url
        }
        if (acts.status === 'fulfilled' && acts.value.rows[0]) {
          const row = acts.value.rows[0]
          patch.activity = row.kind
          patch.idleSecs = row.idle_secs
        }
        setSeed(patch)
      } catch { /* ignore */ }
    }
    load()
    return () => { cancelled = true }
  }, [agent.id])

  // Merge: live events win over the seeded data.
  const status  = liveStatus ?? seed ?? undefined
  const hasData = status && (status.window || status.url || status.activity)
  const isAfk   = status?.activity === 'afk'
  const isActive = status?.activity === 'active'

  return (
    <div
      className={cn(
        'bg-surface border rounded-lg flex flex-col overflow-hidden',
        'transition-colors cursor-pointer group',
        'hover:border-accent/60 active:scale-[0.99]',
        isAfk    ? 'border-yellow-500/30' :
        isActive ? 'border-ok/30'         : 'border-border',
      )}
      onClick={onOpen}
      role="button"
      tabIndex={0}
      onKeyDown={e => e.key === 'Enter' && onOpen()}
    >
      {/* ── Card header ── */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
        <span className="w-2 h-2 rounded-full bg-ok flex-shrink-0 animate-pulse" />
        <span className="font-semibold text-sm flex-1 truncate">{agent.name}</span>
        <span className="flex items-center gap-1 text-[10px] text-muted whitespace-nowrap">
          <Clock size={9} />
          {new Date(agent.connected_at).toLocaleTimeString([], {
            hour:   '2-digit',
            minute: '2-digit',
          })}
        </span>
      </div>

      {/* ── Live status ── */}
      <div className="px-4 py-3 flex flex-col gap-2 flex-1 min-h-[80px]">
        {!hasData ? (
          <p className="text-xs text-muted italic mt-auto mb-auto">Waiting for data…</p>
        ) : (
          <>
            {/* Window */}
            {status?.window && (
              <div className="flex items-start gap-2 min-w-0">
                <Layout size={11} className="text-muted flex-shrink-0 mt-0.5" />
                <span className="text-xs text-primary truncate leading-tight">
                  {status.window}
                </span>
              </div>
            )}

            {/* URL */}
            {status?.url && (
              <div className="flex items-start gap-2 min-w-0">
                <Globe size={11} className="text-muted flex-shrink-0 mt-0.5" />
                <span className="text-xs text-accent truncate leading-tight">
                  {status.url}
                </span>
              </div>
            )}

            {/* Activity badge */}
            {status?.activity && (
              <span
                className={cn(
                  'inline-flex items-center gap-1 self-start mt-auto',
                  'px-2 py-0.5 rounded-full text-[10px] font-semibold uppercase tracking-wide',
                  isActive
                    ? 'bg-ok/15 text-ok'
                    : 'bg-yellow-500/15 text-yellow-500',
                )}
              >
                {isActive
                  ? <><Zap size={9} /> Active</>
                  : <><Moon size={9} /> AFK{status.idleSecs ? ` · ${status.idleSecs}s` : ''}</>}
              </span>
            )}
          </>
        )}
      </div>

      {/* ── Footer button ── */}
      <div className="px-4 py-2.5 border-t border-border">
        <div
          className={cn(
            'w-full flex items-center justify-center gap-1.5 py-1.5 rounded',
            'bg-accent/[.08] group-hover:bg-accent/20',
            'text-accent text-xs font-medium transition-colors',
          )}
        >
          <Monitor size={11} />
          Monitor
        </div>
      </div>
    </div>
  )
}

// ── Grid ──────────────────────────────────────────────────────────────────────

export function OverviewGrid({ agents, liveStatus, onOpen }: Props) {
  const list = Object.values(agents)

  if (list.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-muted
                      min-h-[300px]">
        <Monitor size={44} strokeWidth={1} />
        <p className="text-sm font-medium">No agents connected</p>
        <p className="text-xs text-center max-w-xs">
          Start the agent on a Windows machine to begin monitoring.
          It will appear here automatically.
        </p>
      </div>
    )
  }

  return (
    <div className="grid gap-3
                    grid-cols-1
                    sm:grid-cols-2
                    lg:grid-cols-3
                    xl:grid-cols-4">
      {list.map(agent => (
        <AgentCard
          key={agent.id}
          agent={agent}
          liveStatus={liveStatus[agent.id]}
          onOpen={() => onOpen(agent.id)}
        />
      ))}
    </div>
  )
}
