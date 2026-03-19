// ── Domain models ─────────────────────────────────────────────────────────────

export interface Agent {
  id: string;
  name: string;
  first_seen: string;
  last_seen: string;
  online: boolean;
  connected_at: string | null;       // null when offline
  last_connected_at: string | null;
  last_disconnected_at: string | null;
}

/** Live status tracked from WebSocket events per agent. */
export interface AgentLiveStatus {
  window?: string; // last focused window title
  app?: string; // last focused app exe name
  url?: string; // last active browser URL
  activity?: "afk" | "active";
  idleSecs?: number;
}

export interface WindowEvent {
  title: string;
  app: string;
  hwnd: number;
  ts: string;
  created: string;
}

export interface KeySession {
  app: string;
  window_title: string;
  text: string;
  started_at: string;
  updated_at: string;
}

export interface UrlVisit {
  url: string;
  browser: string;
  ts: string;
}

export interface ActivityEvent {
  kind: "afk" | "active";
  idle_secs?: number;
  ts: string;
}

export interface NetworkAdapterInfo {
  name?: string;
  description?: string;
  mac?: string;
  ips?: string[];
  gateways?: string[];
  dns?: string[];
}

export interface AgentInfo {
  hostname?: string;
  os_name?: string;
  os_version?: string | null;
  os_long_version?: string | null;
  kernel_version?: string | null;
  cpu_brand?: string;
  cpu_cores?: number;
  memory_total_mb?: number;
  memory_used_mb?: number;
  adapters?: NetworkAdapterInfo[];
  ts?: number;
}

// ── WebSocket event envelope ──────────────────────────────────────────────────
//
// The WS viewer sends `event` for its own envelopes (init).
// Agent broadcasts use `type`, which is normalised to `event` by useWebSocket.

export type WsEvent =
  | { event: "init"; agents: Agent[] }
  | { event: "agent_connected"; agent_id: string; name: string; connected_at: string }
  | { event: "agent_disconnected"; agent_id: string; disconnected_at?: string }
  | { event: "window_focus"; agent_id: string; title?: string; app?: string }
  | { event: "agent_info"; agent_id: string; data?: AgentInfo }
  | {
      event: "keys";
      agent_id: string;
      app?: string;
      window_title?: string;
      text?: string;
    }
  | { event: "url"; agent_id: string; url?: string; browser?: string }
  | { event: "afk"; agent_id: string; idle_secs?: number }
  | { event: "active"; agent_id: string };

// ── UI ────────────────────────────────────────────────────────────────────────

export type TabKey = "specs" | "screen" | "keys" | "windows" | "urls" | "activity";
