import type {
  Agent,
  WindowEvent,
  KeySession,
  UrlVisit,
  ActivityEvent,
} from "./types";

interface PageParams {
  limit?: number;
  offset?: number;
}

async function get<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status} – ${url}`);
  return res.json() as Promise<T>;
}

export const api = {
  // ── Auth ──────────────────────────────────────────────────────────────────

  /** Check whether the current session is valid (or no password is set). */
  authStatus: async (): Promise<{
    authenticated: boolean;
    password_required: boolean;
  }> => {
    const res = await fetch("/api/auth/status");
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
    });
    if (!res.ok) {
      const body = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(body.error ?? "Login failed");
    }
  },

  /** Clear the current session cookie. */
  logout: async (): Promise<void> => {
    await fetch("/api/logout", { method: "POST" });
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

  mjpegUrl: (id: string) => `/api/agents/${id}/mjpeg`,
};
