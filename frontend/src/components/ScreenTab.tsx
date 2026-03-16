import { useState, useRef, useCallback } from 'react'
import { MousePointer, Expand } from 'lucide-react'
import { cn } from '../lib/utils'
import { api } from '../lib/api'

interface Props {
  agentId:   string
  onControl: (cmd: unknown) => void
}

export function ScreenTab({ agentId, onControl }: Props) {
  const [remoteOn, setRemoteOn] = useState(false)
  const [coords,   setCoords]   = useState<{ x: number; y: number } | null>(null)
  const [fullscreen, setFullscreen] = useState(false)
  const throttle = useRef(0)
  const imgRef   = useRef<HTMLImageElement>(null)

  // ── Coordinate mapping ──────────────────────────────────────────────────────
  const toNative = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      const rect = e.currentTarget.getBoundingClientRect()
      const nw   = imgRef.current?.naturalWidth  ?? rect.width
      const nh   = imgRef.current?.naturalHeight ?? rect.height
      return {
        x: Math.round((e.clientX - rect.left) / rect.width  * nw),
        y: Math.round((e.clientY - rect.top)  / rect.height * nh),
      }
    },
    [],
  )

  // ── Event handlers ──────────────────────────────────────────────────────────
  const onMove = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const now = Date.now()
    if (now - throttle.current < 50) return   // ~20 events/s
    throttle.current = now
    const { x, y } = toNative(e)
    setCoords({ x, y })
    onControl({ type: 'MouseMove', x, y })
  }, [toNative, onControl])

  const onClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const { x, y } = toNative(e)
    onControl({ type: 'MouseClick', x, y, button: 'Left' })
  }, [toNative, onControl])

  const onRightClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault()
    const { x, y } = toNative(e)
    onControl({ type: 'MouseClick', x, y, button: 'Right' })
  }, [toNative, onControl])

  // ── Render ──────────────────────────────────────────────────────────────────
  const mjpegUrl = api.mjpegUrl(agentId)

  return (
    <div className={cn('flex flex-col gap-3', fullscreen && 'fixed inset-0 z-50 bg-bg p-4')}>
      {/* Toolbar */}
      <div className="flex items-center gap-2 flex-shrink-0 flex-wrap">
        {/* Live badge */}
        <span className="flex items-center gap-1.5 text-xs font-medium">
          <span className="w-2 h-2 rounded-full bg-ok animate-pulse" />
          <span className="text-ok">LIVE</span>
        </span>

        {/* Remote control toggle */}
        <button
          onClick={() => setRemoteOn(r => !r)}
          className={cn(
            'flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium',
            'border transition-colors',
            remoteOn
              ? 'bg-accent/15 border-accent text-accent'
              : 'bg-surface border-border text-muted hover:text-primary',
          )}
        >
          {remoteOn
            ? <><MousePointer size={12} /> Remote control ON</>
            : <><MousePointer size={12} className="opacity-40" /> Remote control OFF</>}
        </button>

        {/* Cursor co-ordinates */}
        {remoteOn && coords && (
          <span className="text-xs text-muted tabular-nums">
            {coords.x} × {coords.y}
          </span>
        )}

        {/* Fullscreen toggle */}
        <button
          onClick={() => setFullscreen(f => !f)}
          className="ml-auto p-1.5 text-muted hover:text-primary transition-colors"
          title={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}
        >
          <Expand size={14} />
        </button>
      </div>

      {/* Stream + overlay */}
      <div className="relative flex-1 flex items-start overflow-auto">
        <div className="relative border border-border rounded-md overflow-hidden bg-black
                        inline-block max-w-full">
          {/* MJPEG image — browser natively renders the stream */}
          <img
            ref={imgRef}
            src={mjpegUrl}
            alt="Live screen"
            className="block max-w-full h-auto"
            draggable={false}
          />

          {/* Transparent click/move overlay for remote control */}
          {remoteOn && (
            <div
              className="absolute inset-0 cursor-crosshair"
              onMouseMove={onMove}
              onClick={onClick}
              onContextMenu={onRightClick}
            />
          )}
        </div>
      </div>

      {/* Hint when remote is off */}
      {!remoteOn && (
        <p className="text-xs text-muted">
          Enable <strong className="text-primary">Remote control</strong> to click/right-click on the stream.
        </p>
      )}
    </div>
  )
}
