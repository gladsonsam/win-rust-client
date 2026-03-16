import { useState, useCallback, useRef } from 'react'
import {
  Monitor, Keyboard, Globe, Layout, Activity,
  Wifi, WifiOff, Loader2, Menu, ArrowLeft,
} from 'lucide-react'

import { useWebSocket, type WsStatus } from './hooks/useWebSocket'
import { Sidebar }      from './components/Sidebar'
import { OverviewGrid } from './components/OverviewGrid'
import { ScreenTab }    from './components/ScreenTab'
import { KeysTab }      from './components/KeysTab'
import { WindowsTab }   from './components/WindowsTab'
import { UrlsTab }      from './components/UrlsTab'
import { ActivityTab }  from './components/ActivityTab'
import { cn }           from './lib/utils'
import type { Agent, AgentLiveStatus, TabKey, WsEvent } from './lib/types'

// ── Tab definitions ───────────────────────────────────────────────────────────

const TABS: { key: TabKey; label: string; Icon: typeof Monitor }[] = [
  { key: 'screen',   label: 'Screen',   Icon: Monitor   },
  { key: 'keys',     label: 'Keys',     Icon: Keyboard  },
  { key: 'windows',  label: 'Windows',  Icon: Layout    },
  { key: 'urls',     label: 'URLs',     Icon: Globe     },
  { key: 'activity', label: 'Activity', Icon: Activity  },
]

// ── App ───────────────────────────────────────────────────────────────────────

export default function App() {
  const [agents,      setAgents]      = useState<Record<string, Agent>>({})
  const [liveStatus,  setLiveStatus]  = useState<Record<string, AgentLiveStatus>>({})
  const [selectedId,  setSelectedId]  = useState<string | null>(null)
  const [view,        setView]        = useState<'overview' | 'detail'>('overview')
  const [activeTab,   setActiveTab]   = useState<TabKey>('screen')
  const [wsStatus,    setWsStatus]    = useState<WsStatus>('connecting')
  const [sidebarOpen, setSidebarOpen] = useState(false)

  // Debounced refresh for data tabs.
  const [refreshKey,  setRefreshKey]  = useState(0)
  const refreshTimer  = useRef<ReturnType<typeof setTimeout> | null>(null)

  const sendRef    = useRef<((d: unknown) => void) | null>(null)
  const selectedRef = useRef<string | null>(null)
  selectedRef.current = selectedId

  // ── Live status helper ─────────────────────────────────────────────────────

  const updateLiveStatus = useCallback(
    (agentId: string, patch: Partial<AgentLiveStatus>) => {
      setLiveStatus(prev => ({
        ...prev,
        [agentId]: { ...prev[agentId], ...patch },
      }))
    },
    [],
  )

  // ── Debounced data tab refresh ─────────────────────────────────────────────

  const scheduleRefresh = useCallback((agentId: string) => {
    if (agentId !== selectedRef.current) return
    if (refreshTimer.current) return          // already scheduled
    refreshTimer.current = setTimeout(() => {
      setRefreshKey(k => k + 1)
      refreshTimer.current = null
    }, 3000)
  }, [])

  // ── WebSocket event handler ────────────────────────────────────────────────

  const handleMessage = useCallback((ev: WsEvent) => {
    switch (ev.event) {

      case 'init':
        setAgents(Object.fromEntries(ev.agents.map(a => [a.id, a])))
        break

      case 'agent_connected':
        setAgents(prev => ({
          ...prev,
          [ev.agent_id]: {
            id:           ev.agent_id,
            name:         ev.name,
            connected_at: new Date().toISOString(),
          },
        }))
        break

      case 'agent_disconnected':
        setAgents(prev => { const n = { ...prev }; delete n[ev.agent_id]; return n })
        // If viewing this agent, fall back to overview.
        if (selectedRef.current === ev.agent_id) {
          setView('overview')
          setSelectedId(null)
        }
        break

      case 'window_focus':
        updateLiveStatus(ev.agent_id, { window: ev.title, app: ev.app })
        scheduleRefresh(ev.agent_id)
        break

      case 'url':
        updateLiveStatus(ev.agent_id, { url: ev.url })
        scheduleRefresh(ev.agent_id)
        break

      case 'afk':
        updateLiveStatus(ev.agent_id, { activity: 'afk', idleSecs: ev.idle_secs })
        scheduleRefresh(ev.agent_id)
        break

      case 'active':
        updateLiveStatus(ev.agent_id, { activity: 'active', idleSecs: undefined })
        scheduleRefresh(ev.agent_id)
        break

      case 'keys':
        scheduleRefresh(ev.agent_id)
        break
    }
  }, [updateLiveStatus, scheduleRefresh])

  const { send } = useWebSocket({ onMessage: handleMessage, onStatusChange: setWsStatus })
  sendRef.current = send

  // ── Control command forwarder ──────────────────────────────────────────────

  const sendControl = useCallback((cmd: unknown) => {
    const id = selectedRef.current
    if (id) sendRef.current?.({ type: 'control', agent_id: id, cmd })
  }, [])

  // ── Navigation helpers ─────────────────────────────────────────────────────

  const selectAgent = useCallback((id: string) => {
    setSelectedId(id)
    setView('detail')
    setActiveTab('screen')
    setRefreshKey(0)
    setSidebarOpen(false)
  }, [])

  const goOverview = useCallback(() => {
    setView('overview')
    setSelectedId(null)
    setSidebarOpen(false)
  }, [])

  const selectedAgent = selectedId ? agents[selectedId] : null

  // ── Render ─────────────────────────────────────────────────────────────────

  return (
    <div className="flex flex-col h-screen bg-bg text-primary overflow-hidden">

      {/* ── Header ── */}
      <header className="flex items-center gap-2 px-3 md:px-4 h-12 bg-surface
                         border-b border-border flex-shrink-0 min-w-0">

        {/* Hamburger (mobile only) */}
        <button
          onClick={() => setSidebarOpen(o => !o)}
          className="md:hidden p-1.5 -ml-1 text-muted hover:text-primary
                     transition-colors flex-shrink-0"
          aria-label="Toggle sidebar"
        >
          <Menu size={16} />
        </button>

        {/* Title / breadcrumb */}
        {view === 'detail' ? (
          <div className="flex items-center gap-2 min-w-0">
            <button
              onClick={goOverview}
              className="flex items-center gap-1 text-muted hover:text-primary
                         text-sm transition-colors flex-shrink-0"
            >
              <ArrowLeft size={14} />
              <span className="hidden sm:inline">Overview</span>
            </button>
            <span className="text-border hidden sm:inline">/</span>
            <span className="text-sm font-medium truncate">
              {selectedAgent?.name ?? 'Agent'}
            </span>
          </div>
        ) : (
          <span className="text-[15px] font-semibold tracking-wide">🖥 Monitor</span>
        )}

        {/* WS status pill */}
        <div className="ml-auto flex items-center gap-1.5 text-xs text-muted flex-shrink-0">
          {wsStatus === 'connected'    && (
            <><Wifi    size={12} className="text-ok"     /><span className="hidden sm:inline">Connected</span></>
          )}
          {wsStatus === 'disconnected' && (
            <><WifiOff size={12} className="text-danger" /><span className="hidden sm:inline">Disconnected</span></>
          )}
          {wsStatus === 'connecting'   && (
            <><Loader2 size={12} className="animate-spin" /><span className="hidden sm:inline">Connecting…</span></>
          )}
        </div>
      </header>

      {/* ── Body ── */}
      <div className="flex flex-1 overflow-hidden relative">

        {/* Mobile sidebar backdrop */}
        {sidebarOpen && (
          <div
            className="fixed inset-0 z-20 bg-black/50 md:hidden"
            onClick={() => setSidebarOpen(false)}
          />
        )}

        {/* Sidebar */}
        <Sidebar
          agents={agents}
          selectedId={selectedId}
          view={view}
          onSelect={selectAgent}
          onOverview={goOverview}
          open={sidebarOpen}
          onClose={() => setSidebarOpen(false)}
        />

        {/* Main content */}
        <main className="flex flex-col flex-1 overflow-hidden min-w-0">

          {/* ── Overview ── */}
          {view === 'overview' && (
            <div className="flex-1 overflow-auto p-3 md:p-4">
              <OverviewGrid
                agents={agents}
                liveStatus={liveStatus}
                onOpen={selectAgent}
              />
            </div>
          )}

          {/* ── Detail ── */}
          {view === 'detail' && (
            <>
              {/* Tab bar */}
              <div className="flex bg-surface border-b border-border
                              flex-shrink-0 overflow-x-auto">
                {TABS.map(({ key, label, Icon }) => (
                  <button
                    key={key}
                    onClick={() => setActiveTab(key)}
                    className={cn(
                      'flex items-center gap-1.5 px-3 md:px-4 py-2.5 text-sm',
                      'border-b-2 transition-colors whitespace-nowrap flex-shrink-0',
                      activeTab === key
                        ? 'text-primary border-accent'
                        : 'text-muted border-transparent hover:text-primary',
                    )}
                  >
                    <Icon size={13} />
                    <span className="hidden sm:inline">{label}</span>
                  </button>
                ))}
              </div>

              {/* Tab content */}
              <div className="flex-1 overflow-auto p-3 md:p-4">
                {selectedId && (
                  <>
                    {activeTab === 'screen'   && (
                      <ScreenTab   key={selectedId} agentId={selectedId} onControl={sendControl} />
                    )}
                    {activeTab === 'keys'     && (
                      <KeysTab     agentId={selectedId} refreshKey={refreshKey} />
                    )}
                    {activeTab === 'windows'  && (
                      <WindowsTab  agentId={selectedId} refreshKey={refreshKey} />
                    )}
                    {activeTab === 'urls'     && (
                      <UrlsTab     agentId={selectedId} refreshKey={refreshKey} />
                    )}
                    {activeTab === 'activity' && (
                      <ActivityTab agentId={selectedId} refreshKey={refreshKey} />
                    )}
                  </>
                )}
              </div>
            </>
          )}
        </main>
      </div>
    </div>
  )
}
