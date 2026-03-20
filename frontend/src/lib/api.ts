import type {
  Agent,
  WindowEvent,
  KeySession,
  UrlVisit,
  ActivityEvent,
  AgentInfo,
  RetentionPolicy,
  LocalUiPasswordGlobalState,
  LocalUiPasswordAgentState,
} from "./types";

interface PageParams {
  limit?: number;
  offset?: number;
}

async function get<T>(url: string): Promise<T> {
  const res = await fetch(url, { credentials: "include" });
  if (!res.ok) throw new Error(`HTTP ${res.status} – ${url}`);
  return res.json() as Promise<T>;
}

async function putJson<T>(url: string, body: unknown): Promise<T> {
  const res = await fetch(url, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    credentials: "include",
  });
  if (!res.ok) {
    const errBody = (await res.json().catch(() => ({}))) as { error?: string };
    throw new Error(errBody.error ?? `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

export const api = {
  // ── Auth ──────────────────────────────────────────────────────────────────

  /** Check whether the current session is valid (or no password is set). */
  authStatus: async (): Promise<{
    authenticated: boolean;
    password_required: boolean;
  }> => {
    const res = await fetch("/api/auth/status", { credentials: "include" });
    // 401 is a normal "not logged in" response — still parse it.
    if (!res.ok && res.status !== 401) throw new Error(`HTTP ${res.status}`);
    return res.json();
  },

  /** Submit the UI password; throws with the server error message on failure. */
  login: async (password: string): Promise<void> => {
    const res = await fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password }),
      credentials: "include",
    });
    if (!res.ok) {
      const body = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(body.error ?? "Login failed");
    }
  },

  /** Clear the current session cookie. */
  logout: async (): Promise<void> => {
    await fetch("/api/logout", { method: "POST", credentials: "include" });
  },

  // ── Dashboard data ────────────────────────────────────────────────────────

  agents: (): Promise<{ agents: Agent[] }> => get("/api/agents"),

  windows: (
    id: string,
    { limit = 100, offset = 0 }: PageParams = {},
  ): Promise<{ rows: WindowEvent[] }> =>
    get(`/api/agents/${id}/windows?limit=${limit}&offset=${offset}`),

  keys: (
    id: string,
    { limit = 100, offset = 0 }: PageParams = {},
  ): Promise<{ rows: KeySession[] }> =>
    get(`/api/agents/${id}/keys?limit=${limit}&offset=${offset}`),

  urls: (
    id: string,
    { limit = 100, offset = 0 }: PageParams = {},
  ): Promise<{ rows: UrlVisit[] }> =>
    get(`/api/agents/${id}/urls?limit=${limit}&offset=${offset}`),

  activity: (
    id: string,
    { limit = 100, offset = 0 }: PageParams = {},
  ): Promise<{ rows: ActivityEvent[] }> =>
    get(`/api/agents/${id}/activity?limit=${limit}&offset=${offset}`),

  agentInfo: (id: string): Promise<{ info: AgentInfo | null }> =>
    get(`/api/agents/${id}/info`),

  // ── Destructive actions ────────────────────────────────────────────────
  /** Clear all stored telemetry history for this agent (windows/keys/urls/activity). */
  clearAgentHistory: async (id: string): Promise<{ cleared_rows: number }> => {
    const res = await fetch(`/api/agents/${id}/history/clear`, {
      method: "POST",
      credentials: "include",
    });
    if (!res.ok) {
      const body = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(body.error ?? `HTTP ${res.status}`);
    }
    return (await res.json()) as { cleared_rows: number };
  },

  mjpegUrl: (id: string) => `/api/agents/${id}/mjpeg`,

  // ── Retention (server) ───────────────────────────────────────────────────

  retentionGlobalGet: (): Promise<RetentionPolicy> =>
    get("/api/settings/retention"),

  retentionGlobalPut: (body: RetentionPolicy): Promise<RetentionPolicy> =>
    putJson("/api/settings/retention", body),

  retentionAgentGet: (
    id: string,
  ): Promise<{ global: RetentionPolicy; override: RetentionPolicy | null }> =>
    get(`/api/agents/${id}/retention`),

  retentionAgentPut: (
    id: string,
    body: RetentionPolicy,
  ): Promise<{ global: RetentionPolicy; override: RetentionPolicy | null }> =>
    putJson(`/api/agents/${id}/retention`, body),

  retentionAgentDelete: async (
    id: string,
  ): Promise<{ global: RetentionPolicy; override: RetentionPolicy | null }> => {
    const res = await fetch(`/api/agents/${id}/retention`, { method: "DELETE" });
    if (!res.ok) {
      const errBody = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(errBody.error ?? `HTTP ${res.status}`);
    }
    return res.json() as Promise<{
      global: RetentionPolicy;
      override: RetentionPolicy | null;
    }>;
  },

  // ── Agent local settings window password (pushed to Windows agents) ───────

  localUiPasswordGlobalGet: (): Promise<LocalUiPasswordGlobalState> =>
    get("/api/settings/local-ui-password"),

  localUiPasswordGlobalPut: (body: {
    password: string | null;
  }): Promise<LocalUiPasswordGlobalState> =>
    putJson("/api/settings/local-ui-password", body),

  localUiPasswordAgentGet: (
    id: string,
  ): Promise<LocalUiPasswordAgentState> =>
    get(`/api/agents/${id}/local-ui-password`),

  localUiPasswordAgentPut: (
    id: string,
    body: { password: string | null },
  ): Promise<LocalUiPasswordAgentState> =>
    putJson(`/api/agents/${id}/local-ui-password`, body),

  localUiPasswordAgentDelete: async (
    id: string,
  ): Promise<LocalUiPasswordAgentState> => {
    const res = await fetch(`/api/agents/${id}/local-ui-password`, {
      method: "DELETE",
      credentials: "include",
    });
    if (!res.ok) {
      const errBody = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(errBody.error ?? `HTTP ${res.status}`);
    }
    return res.json() as Promise<LocalUiPasswordAgentState>;
  },
};
