-- Initial schema for the monitoring server
-- Uses pgcrypto for UUID generation (gen_random_uuid()).

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ─── Agents ───────────────────────────────────────────────────────────────────
-- One row per monitored machine.  The agent name (hostname) is the natural key.

CREATE TABLE IF NOT EXISTS agents (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        UNIQUE NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ─── Window focus events ──────────────────────────────────────────────────────
-- Recorded each time the foreground window changes on the agent.

CREATE TABLE IF NOT EXISTS window_events (
    id       BIGSERIAL   PRIMARY KEY,
    agent_id UUID        NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    title    TEXT        NOT NULL DEFAULT '',
    app      TEXT        NOT NULL DEFAULT '',
    hwnd     BIGINT      NOT NULL DEFAULT 0,
    ts       TIMESTAMPTZ NOT NULL,
    created  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_window_events_agent_ts
    ON window_events (agent_id, ts DESC);

-- ─── Key sessions ─────────────────────────────────────────────────────────────
-- Batched keystroke buffers.  The server appends to an open session (same
-- agent/app/window, touched within the last 30 s) rather than creating a new
-- row for every flush from the agent.

CREATE TABLE IF NOT EXISTS key_sessions (
    id           BIGSERIAL   PRIMARY KEY,
    agent_id     UUID        NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    app          TEXT        NOT NULL DEFAULT '',
    window_title TEXT        NOT NULL DEFAULT '',
    text         TEXT        NOT NULL DEFAULT '',
    started_at   TIMESTAMPTZ NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_key_sessions_agent_updated
    ON key_sessions (agent_id, updated_at DESC);

-- ─── URL visits ───────────────────────────────────────────────────────────────
-- One row per distinct URL visited (consecutive duplicates are suppressed).

CREATE TABLE IF NOT EXISTS url_visits (
    id       BIGSERIAL   PRIMARY KEY,
    agent_id UUID        NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    url      TEXT        NOT NULL DEFAULT '',
    title    TEXT,
    browser  TEXT,
    ts       TIMESTAMPTZ NOT NULL,
    created  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_url_visits_agent_ts
    ON url_visits (agent_id, ts DESC);

-- ─── Activity log ─────────────────────────────────────────────────────────────
-- AFK / active transitions.

CREATE TABLE IF NOT EXISTS activity_log (
    id         BIGSERIAL   PRIMARY KEY,
    agent_id   UUID        NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    event_type TEXT        NOT NULL DEFAULT '',   -- 'afk' | 'active'
    idle_secs  BIGINT,
    ts         TIMESTAMPTZ NOT NULL,
    created    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_activity_log_agent_ts
    ON activity_log (agent_id, ts DESC);
