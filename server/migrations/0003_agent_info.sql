-- Per-agent machine specs / network info (latest snapshot).

CREATE TABLE IF NOT EXISTS agent_info (
    agent_id    UUID        PRIMARY KEY REFERENCES agents(id) ON DELETE CASCADE,
    info        JSONB       NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_info_updated_at
    ON agent_info (updated_at DESC);

