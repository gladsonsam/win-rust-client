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
import {
  Layout,
  Globe,
  Keyboard,
  Clock,
  ChevronDown,
  ChevronRight,
  Search,
  Trash2,
  Loader2,
} from "lucide-react";
import { api } from "../lib/api";
import { cn, truncate } from "../lib/utils";
import type { WindowEvent, UrlVisit, KeySession } from "../lib/types";

// ── Types ─────────────────────────────────────────────────────────────────────

interface Session {
  /** Representative window (used for the collapsed header). */
  window: WindowEvent;
  /** ISO timestamp when this session ended (i.e. when the next window took focus). */
  endTs: string | null;
  durationSecs: number | null;
  /** All window-focus events within this merged app session (chronological). */
  windows: WindowEvent[];
  urls: UrlVisit[];
  keys: KeySession[];
}

function sessionId(s: Session): string {
  const start = s.windows[0]?.ts ?? s.window.ts;
  const app = s.window.app ?? "";
  return `${app}|${start}`;
}

function applyKeyCorrections(raw: string): string {
  // Agent encodes special keys as bracketed tokens like `[⌫]`.
  // This renders a "what they meant" view by applying backspaces.
  const out: string[] = [];

  // Split on tokens while keeping them.
  const parts = raw.split(/(\[⌫\]|\[Del\]|\[⇥\])/g);
  for (const p of parts) {
    if (!p) continue;
    if (p === "[⌫]") {
      if (out.length > 0) out.pop();
      continue;
    }
    if (p === "[Del]") {
      // Without cursor tracking, treat Delete as a no-op in corrected view.
      continue;
    }
    if (p === "[⇥]") {
      out.push("\t");
      continue;
    }
    // Normal text chunk
    for (const ch of p) out.push(ch);
  }

  return out.join("");
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

    return {
      window: win,
      endTs,
      durationSecs,
      windows: [win],
      urls: sessionUrls,
      keys: sessionKeys,
    };
  });
}

/** Merge consecutive sessions with the same executable (window.app).
 *  This keeps the Activity tab from showing "chrome.exe" multiple times
 *  side-by-side when the user is switching tabs/windows inside the same app.
 */
function mergeAdjacentByApp(sessions: Session[]): Session[] {
  const out: Session[] = [];

  for (const s of sessions) {
    const last = out[out.length - 1];
    if (!last) {
      out.push(s);
      continue;
    }

    // Only merge when both are known executables and match.
    if (
      last.window.app &&
      s.window.app &&
      last.window.app.toLowerCase() === s.window.app.toLowerCase()
    ) {
      const mergedStartMs = toMs(last.window.ts);
      const mergedEndTs = s.endTs ?? null;
      const mergedEndMs = mergedEndTs ? toMs(mergedEndTs) : null;

      const nextTitle =
        s.window.title && s.window.title.trim() ? s.window.title : last.window.title;

      const merged: Session = {
        // Keep the app constant; use the latest title for nicer UX.
        window: { ...last.window, title: nextTitle, app: last.window.app },
        endTs: mergedEndTs,
        durationSecs: mergedEndMs ? Math.round((mergedEndMs - mergedStartMs) / 1000) : null,
        windows: [...last.windows, ...s.windows],
        urls: [...last.urls, ...s.urls],
        keys: [...last.keys, ...s.keys],
      };

      // Keep the UI order consistent with existing (DESC by timestamp).
      merged.urls.sort((a, b) => toMs(b.ts) - toMs(a.ts));
      merged.keys.sort((a, b) => toMs(b.updated_at || b.started_at) - toMs(a.updated_at || a.started_at));
      merged.windows.sort((a, b) => toMs(a.ts) - toMs(b.ts));

      out[out.length - 1] = merged;
    } else {
      out.push(s);
    }
  }

  return out;
}

// ── Session card ──────────────────────────────────────────────────────────────

function SessionCard({
  session,
  expanded,
  onToggle,
  correctedKeys,
}: {
  session: Session;
  expanded: boolean;
  onToggle: () => void;
  correctedKeys: boolean;
}) {
  const { window: win, durationSecs, urls, keys, windows } = session;
  const hasDetail = urls.length > 0 || keys.length > 0;

  type TimelineItem =
    | { kind: "window"; ts: string; tsMs: number; title: string }
    | { kind: "url"; ts: string; tsMs: number; title: string; url: UrlVisit }
    | { kind: "keys"; ts: string; tsMs: number; title: string; keys: KeySession };

  const getActiveTitleAt = (tsMs: number): string => {
    // Windows are chronological. Find the last window event <= ts.
    let active: WindowEvent | undefined = windows[0];
    for (const w of windows) {
      if (toMs(w.ts) <= tsMs) active = w;
      else break;
    }
    const t = active?.title?.trim();
    return t && t.length > 0 ? t : active?.app?.trim() || "Unknown tab";
  };

  const timeline: TimelineItem[] = expanded
    ? [
        ...windows.map((w) => ({
          kind: "window" as const,
          ts: String(w.ts),
          tsMs: toMs(w.ts),
          title: (w.title || w.app || "Unknown tab").trim() || "Unknown tab",
        })),
        ...urls.map((u) => {
          const ms = toMs(u.ts);
          return {
            kind: "url" as const,
            ts: String(u.ts),
            tsMs: ms,
            title: getActiveTitleAt(ms),
            url: u,
          };
        }),
        ...keys.map((k) => {
          // Use updated_at for timeline ordering; started_at can be much earlier
          // for long buffers, which makes keys appear "before" later URLs.
          const ms = toMs(k.updated_at || k.started_at);
          const title =
            (k.window_title || "").trim() || getActiveTitleAt(ms);
          return {
            kind: "keys" as const,
            ts: String(k.updated_at || k.started_at),
            tsMs: ms,
            title,
            keys: k,
          };
        }),
      ].sort((a, b) => a.tsMs - b.tsMs)
    : [];

  return (
    <div className="bg-surface border border-border rounded-lg overflow-hidden">
      {/* ── Header row ── */}
      <div
        className={cn(
          "w-full flex items-start gap-3 px-4 py-3 text-left transition-colors",
          hasDetail && "hover:bg-white/[.03] cursor-pointer",
          !hasDetail && "cursor-default",
        )}
        onClick={() => hasDetail && onToggle()}
        onKeyDown={(e) => {
          if (!hasDetail) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        role={hasDetail ? "button" : undefined}
        tabIndex={hasDetail ? 0 : -1}
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
      </div>

      {/* ── Expanded detail ── */}
      {expanded && (
        <div className="border-t border-border touch-pan-y">
          <div className="px-4 py-3">
            <p className="text-[10px] uppercase tracking-widest text-muted font-semibold mb-2">
              Timeline
            </p>

            <div className="flex flex-col gap-2 touch-pan-y">
              {(() => {
                let lastTitle = "";
                return timeline.map((it, idx) => {
                  const showTitle = it.title !== lastTitle;
                  if (showTitle) lastTitle = it.title;

                  return (
                    <div key={idx} className="flex flex-col gap-1">
                      {showTitle && (
                        <div className="text-[11px] text-muted font-medium truncate">
                          {it.title}
                        </div>
                      )}

                      <div className="flex items-start gap-2 min-w-0">
                        <span className="text-[11px] text-muted tabular-nums mt-0.5 flex-shrink-0 w-14 text-right">
                          {fmtTime(it.ts)}
                        </span>

                        {it.kind === "window" ? (
                          <span className="flex items-center gap-1 text-[12px] text-primary/70">
                            <Layout size={12} className="text-muted" />
                            Focus
                          </span>
                        ) : it.kind === "url" ? (
                          <div className="flex items-start gap-2 min-w-0">
                            <span className="flex items-center gap-1 text-[12px] text-accent flex-shrink-0">
                              <Globe size={12} />
                              URL
                            </span>
                            <a
                              href={it.url.url}
                              target="_blank"
                              rel="noreferrer"
                              className="text-[12px] text-accent hover:underline truncate min-w-0"
                              title={it.url.url}
                            >
                              {truncate(it.url.url, 90)}
                            </a>
                            {it.url.browser && (
                              <span className="text-[10px] text-muted flex-shrink-0">
                                {it.url.browser}
                              </span>
                            )}
                          </div>
                        ) : (
                          <div className="flex items-start gap-2 min-w-0">
                            <span className="flex items-center gap-1 text-[12px] text-ok flex-shrink-0">
                              <Keyboard size={12} />
                              Keys
                            </span>
                            <code className="text-[12px] bg-bg/70 px-2 py-0.5 rounded text-primary/80 break-all leading-relaxed font-mono">
                              {it.keys.text?.trim()
                                ? (correctedKeys
                                    ? applyKeyCorrections(it.keys.text)
                                    : it.keys.text)
                                : <span className="text-muted italic">empty</span>}
                            </code>
                          </div>
                        )}
                      </div>
                    </div>
                  );
                });
              })()}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

interface Props {
  agentId: string;
  refreshKey: number;
  onHistoryCleared: () => void;
}

export function ActivityTab({ agentId, refreshKey, onHistoryCleared }: Props) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [clearing, setClearing] = useState(false);
  const [expandedIds, setExpandedIds] = useState<Record<string, boolean>>({});
  const [correctedKeys, setCorrectedKeys] = useState(true);

  // Reset UI state only when switching agents (not on live refresh).
  useEffect(() => {
    setSearch("");
    setExpandedIds({});
  }, [agentId]);

  useEffect(() => {
    // Only show the big spinner on first load; silently refresh afterwards
    // so the UI doesn't flash every time live data arrives.
    setLoading((prev) => (sessions.length === 0 ? true : prev));
    setError(null);

    Promise.all([
      api.windows(agentId, { limit: 200 }),
      api.urls(agentId, { limit: 500 }),
      api.keys(agentId, { limit: 500 }),
    ])
      .then(([{ rows: wins }, { rows: urls }, { rows: keys }]) => {
        // buildSessions returns chronological sessions; merge adjacent by app
        // before rendering newest-first.
        const built = buildSessions(wins, urls, keys);
        setSessions(mergeAdjacentByApp(built));
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
  const q = search.trim().toLowerCase();
  const filtered = !q
    ? sorted
    : sorted.filter((s) => {
        const hay = [
          s.window.app,
          s.window.title,
          ...s.windows.map((w) => w.title ?? ""),
          ...s.urls.map((u) => u.url),
          ...s.urls.map((u) => u.browser ?? ""),
          ...s.keys.map((k) => k.text ?? ""),
          ...s.keys.map((k) => k.app ?? ""),
          ...s.keys.map((k) => k.window_title ?? ""),
        ]
          .join(" ")
          .toLowerCase();
        return hay.includes(q);
      });

  const toggleExpanded = (id: string) => {
    setExpandedIds((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const correctedLabel = correctedKeys ? "Corrected" : "Raw";

  return (
    <div className="flex flex-col gap-2 touch-pan-y">
      {/* Toolbar */}
      <div className="flex items-center gap-2 flex-wrap">
        <div className="relative flex-1 max-w-sm min-w-[220px]">
          <Search
            size={13}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-muted pointer-events-none"
          />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search app, URLs, or keystrokes…"
            className="w-full bg-surface border border-border rounded-md
                       pl-8 pr-3 py-1.5 text-sm text-primary placeholder-muted
                       focus:outline-none focus:border-accent transition-colors"
          />
        </div>

        <button
          onClick={() => setCorrectedKeys((v) => !v)}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium",
            "border border-border bg-surface text-muted hover:text-primary transition-colors",
          )}
          title="Toggle whether backspaces are applied"
        >
          <Keyboard size={14} />
          {correctedLabel}
        </button>

        <button
          onClick={async () => {
            if (clearing) return;
            const ok = window.confirm(
              `Clear activity history for this agent?\n\nThis will delete windows, keystrokes, URLs, and AFK/active history for the selected client.`,
            );
            if (!ok) return;
            setClearing(true);
            setError(null);
            try {
              await api.clearAgentHistory(agentId);
              onHistoryCleared();
            } catch (e) {
              setError(String(e));
            } finally {
              setClearing(false);
            }
          }}
          disabled={clearing}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium",
            "border border-danger text-danger hover:bg-white/[.03] transition-colors",
            "disabled:opacity-50 disabled:cursor-not-allowed",
          )}
          title="Clear this agent's stored history"
        >
          {clearing ? <Loader2 size={14} className="animate-spin" /> : <Trash2 size={14} />}
          Clear history
        </button>
      </div>

      <p className="text-[11px] text-muted">
        {filtered.length} of {sorted.length} window sessions · click to expand URLs &amp; keystrokes
      </p>

      {filtered.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-10 text-muted">
          <Layout size={28} strokeWidth={1} />
          <p className="text-sm mt-2">
            {q ? (
              <>
                No matches for <span className="text-primary">"{search.trim()}"</span>
              </>
            ) : (
              "No activity recorded yet"
            )}
          </p>
        </div>
      ) : (
        filtered.map((s) => {
          const id = sessionId(s);
          return (
            <SessionCard
              key={id}
              session={s}
              expanded={!!expandedIds[id]}
              onToggle={() => toggleExpanded(id)}
              correctedKeys={correctedKeys}
            />
          );
        })
      )}
    </div>
  );
}
