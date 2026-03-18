/**
 * Activity Tab – Combined session timeline.
 *
 * Fetches windows, URLs, and keystroke sessions in parallel, then groups them
 * into "window focus sessions". Each session card shows:
 *  - the focused app + window title and how long it was active
 *  - any URLs visited while that window was focused
 *  - any keystroke buffers whose timestamps fall within the session window
 */
import { useEffect, useState } from "react";
import { Layout, Globe, Keyboard, Clock, ChevronDown, ChevronRight } from "lucide-react";
import { api } from "../lib/api";
import { cn, truncate } from "../lib/utils";
import type { WindowEvent, UrlVisit, KeySession } from "../lib/types";

// ── Types ─────────────────────────────────────────────────────────────────────

interface Session {
  window: WindowEvent;
  /** ISO timestamp when this session ended (i.e. when the next window took focus). */
  endTs: string | null;
  durationSecs: number | null;
  urls: UrlVisit[];
  keys: KeySession[];
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function toMs(ts: string | number): number {
  if (typeof ts === "number") return ts * 1000;
  return new Date(ts).getTime();
}

function fmtDuration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

function fmtTime(ts: string | number): string {
  const d = typeof ts === "number" ? new Date(ts * 1000) : new Date(ts);
  return isNaN(d.getTime()) ? String(ts) : d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** Build combined sessions from raw rows. */
function buildSessions(
  windows: WindowEvent[],
  urls: UrlVisit[],
  keys: KeySession[],
): Session[] {
  // windows come back newest-first from the API — reverse to chronological
  const wins = [...windows].reverse();

  return wins.map((win, idx) => {
    const startMs = toMs(win.ts);
    const nextWin = wins[idx + 1];
    const endMs = nextWin ? toMs(nextWin.ts) : null;
    const endTs = endMs ? new Date(endMs).toISOString() : null;
    const durationSecs = endMs ? Math.round((endMs - startMs) / 1000) : null;

    // URLs whose timestamp falls within this session window
    const sessionUrls = urls.filter((u) => {
      const ms = toMs(u.ts);
      return ms >= startMs && (endMs === null || ms < endMs);
    });

    // Key sessions whose start time falls within this session window
    const sessionKeys = keys.filter((k) => {
      const ms = toMs(k.started_at);
      return ms >= startMs && (endMs === null || ms < endMs);
    });

    return { window: win, endTs, durationSecs, urls: sessionUrls, keys: sessionKeys };
  });
}

// ── Session card ──────────────────────────────────────────────────────────────

function SessionCard({ session }: { session: Session }) {
  const [expanded, setExpanded] = useState(false);
  const { window: win, durationSecs, urls, keys } = session;
  const hasDetail = urls.length > 0 || keys.length > 0;

  return (
    <div className="bg-surface border border-border rounded-lg overflow-hidden">
      {/* ── Header row ── */}
      <button
        className={cn(
          "w-full flex items-start gap-3 px-4 py-3 text-left transition-colors",
          hasDetail && "hover:bg-white/[.03] cursor-pointer",
          !hasDetail && "cursor-default",
        )}
        onClick={() => hasDetail && setExpanded((e) => !e)}
        disabled={!hasDetail}
        aria-expanded={expanded}
      >
        {/* Expand toggle */}
        <span className="mt-0.5 text-muted flex-shrink-0 w-3.5">
          {hasDetail ? (
            expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />
          ) : null}
        </span>

        {/* App icon proxy */}
        <span className="flex-shrink-0 mt-0.5">
          <Layout size={14} className="text-accent" />
        </span>

        {/* Window / app info */}
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-primary truncate leading-tight">
            {win.title || win.app || "Unknown window"}
          </p>
          {win.app && win.title && (
            <p className="text-[11px] text-muted truncate mt-0.5">{win.app}</p>
          )}
        </div>

        {/* Meta: time + duration + badge counts */}
        <div className="flex-shrink-0 flex flex-col items-end gap-1 ml-2">
          <span className="flex items-center gap-1 text-[11px] text-muted tabular-nums">
            <Clock size={9} />
            {fmtTime(win.ts)}
          </span>
          <div className="flex items-center gap-1.5">
            {durationSecs != null && (
              <span className="text-[10px] text-muted tabular-nums">
                {fmtDuration(durationSecs)}
              </span>
            )}
            {urls.length > 0 && (
              <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 rounded-full bg-accent/10 text-accent">
                <Globe size={8} /> {urls.length}
              </span>
            )}
            {keys.length > 0 && (
              <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 rounded-full bg-ok/10 text-ok">
                <Keyboard size={8} /> {keys.length}
              </span>
            )}
          </div>
        </div>
      </button>

      {/* ── Expanded detail ── */}
      {expanded && (
        <div className="border-t border-border divide-y divide-border">
          {/* URLs */}
          {urls.length > 0 && (
            <div className="px-4 py-3">
              <p className="text-[10px] uppercase tracking-widest text-muted font-semibold mb-2 flex items-center gap-1.5">
                <Globe size={9} /> URLs visited
              </p>
              <div className="flex flex-col gap-1.5">
                {urls.map((u, i) => (
                  <div key={i} className="flex items-start gap-2 min-w-0">
                    <span className="text-[11px] text-muted tabular-nums mt-0.5 flex-shrink-0 w-14 text-right">
                      {fmtTime(u.ts)}
                    </span>
                    <a
                      href={u.url}
                      target="_blank"
                      rel="noreferrer"
                      className="text-[12px] text-accent hover:underline truncate min-w-0"
                      title={u.url}
                    >
                      {truncate(u.url, 80)}
                    </a>
                    {u.browser && (
                      <span className="text-[10px] text-muted flex-shrink-0">
                        {u.browser}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Key sessions */}
          {keys.length > 0 && (
            <div className="px-4 py-3">
              <p className="text-[10px] uppercase tracking-widest text-muted font-semibold mb-2 flex items-center gap-1.5">
                <Keyboard size={9} /> Keystrokes
              </p>
              <div className="flex flex-col gap-2">
                {keys.map((k, i) => (
                  <div key={i} className="flex items-start gap-2 min-w-0">
                    <span className="text-[11px] text-muted tabular-nums mt-0.5 flex-shrink-0 w-14 text-right">
                      {fmtTime(k.started_at)}
                    </span>
                    <code className="text-[12px] bg-bg/70 px-2 py-0.5 rounded text-primary/80 break-all leading-relaxed font-mono">
                      {k.text || <span className="text-muted italic">empty</span>}
                    </code>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

interface Props {
  agentId: string;
  refreshKey: number;
}

export function ActivityTab({ agentId, refreshKey }: Props) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    setError(null);

    Promise.all([
      api.windows(agentId, { limit: 200 }),
      api.urls(agentId, { limit: 500 }),
      api.keys(agentId, { limit: 500 }),
    ])
      .then(([{ rows: wins }, { rows: urls }, { rows: keys }]) => {
        setSessions(buildSessions(wins, urls, keys));
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, refreshKey]);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-40 text-muted text-sm">
        Loading activity…
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center justify-center h-40 text-danger text-sm">
        {error}
      </div>
    );
  }

  if (sessions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-40 gap-2 text-muted">
        <Layout size={32} strokeWidth={1} />
        <p className="text-sm">No activity recorded yet</p>
      </div>
    );
  }

  // Show newest sessions first
  const sorted = [...sessions].reverse();

  return (
    <div className="flex flex-col gap-2">
      <p className="text-[11px] text-muted">
        {sorted.length} window sessions · click to expand URLs &amp; keystrokes
      </p>
      {sorted.map((s, i) => (
        <SessionCard key={i} session={s} />
      ))}
    </div>
  );
}
