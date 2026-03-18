-- Track agent connection history (each websocket session)

CREATE TABLE IF NOT EXISTS agent_sessions (
    id              BIGSERIAL   PRIMARY KEY,
    agent_id        UUID        NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    connected_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    disconnected_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_agent_connected
    ON agent_sessions (agent_id, connected_at DESC);

