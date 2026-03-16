import type { Agent, WindowEvent, KeySession, UrlVisit, ActivityEvent } from './types'

interface PageParams {
  limit?:  number
  offset?: number
}

async function get<T>(url: string): Promise<T> {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`HTTP ${res.status} – ${url}`)
  return res.json() as Promise<T>
}

export const api = {
  agents: (): Promise<{ agents: Agent[] }> =>
    get('/api/agents'),

  windows: (id: string, { limit = 100, offset = 0 }: PageParams = {}): Promise<{ rows: WindowEvent[] }> =>
    get(`/api/agents/${id}/windows?limit=${limit}&offset=${offset}`),

  keys: (id: string, { limit = 100, offset = 0 }: PageParams = {}): Promise<{ rows: KeySession[] }> =>
    get(`/api/agents/${id}/keys?limit=${limit}&offset=${offset}`),

  urls: (id: string, { limit = 100, offset = 0 }: PageParams = {}): Promise<{ rows: UrlVisit[] }> =>
    get(`/api/agents/${id}/urls?limit=${limit}&offset=${offset}`),

  activity: (id: string, { limit = 100, offset = 0 }: PageParams = {}): Promise<{ rows: ActivityEvent[] }> =>
    get(`/api/agents/${id}/activity?limit=${limit}&offset=${offset}`),

  mjpegUrl: (id: string) => `/api/agents/${id}/mjpeg`,
}
