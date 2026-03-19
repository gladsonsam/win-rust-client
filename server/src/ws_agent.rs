//! WebSocket handler for Windows agents.
//!
//! Agents connect to `ws://<host>/ws/agent?name=<hostname>`.
//! Binary frames are treated as JPEG screenshots and cached in memory.
//! Text frames must be JSON objects with a `"type"` field.
//!
//! Each agent connection also gets a per-agent command channel so that
//! dashboard viewers can send mouse/keyboard control commands back to the
//! agent (via the server) without needing a direct connection.
//!
//! Screen capture is demand-driven: the MJPEG stream handler in `api.rs`
//! sends `start_capture` / `stop_capture` based on viewer count.  The agent
//! always stops capture when its WebSocket session ends, so each new session
//! starts idle until explicitly asked to capture.

use std::sync::Arc;

use axum::extract::ws::WebSocket;
use axum::{
    extract::{ws::Message, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::{db, state::{AppState, Frame}};

// ─── Connection handshake ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentQuery {
    name: Option<String>,
    /// Optional agent auth secret. Required when `AGENT_SECRET` is configured.
    secret: Option<String>,
}

pub async fn handler(
    ws: WebSocketUpgrade,
    Query(params): Query<AgentQuery>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Enforce agent authentication when configured.
    if let Some(expected) = state.agent_secret.as_deref() {
        let provided = params.secret.as_deref().unwrap_or("");
        
        // Secure timing attack mitigation with constant-time equality.
        let is_equal = subtle::ConstantTimeEq::ct_eq(provided.as_bytes(), expected.as_bytes());
        if !bool::from(is_equal) {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }

    let name = params.name.unwrap_or_else(|| "unknown".into());
    ws.on_upgrade(move |socket| run(socket, name, state))
}

// ─── Per-agent message loop ───────────────────────────────────────────────────

async fn run(mut ws: WebSocket, name: String, state: Arc<AppState>) {
    // Register / touch the agent row in Postgres.
    let agent_id = match db::upsert_agent(&state.db, &name).await {
        Ok(id) => id,
        Err(e) => {
            error!("upsert_agent({name}): {e}");
            return;
        }
    };

    // Record connection session (history).
    let session_id = match db::start_agent_session(&state.db, agent_id).await {
        Ok(id) => id,
        Err(e) => {
            error!("start_agent_session({agent_id}): {e}");
            return;
        }
    };

    info!("Agent connected: {name} ({agent_id})");
    let connected_at = chrono::Utc::now();

    // Add to in-memory agent map.
    {
        let mut map = state.agents.lock().unwrap();
        map.insert(
            agent_id,
            crate::state::AgentConn {
                id: agent_id,
                name: name.clone(),
                connected_at,
            },
        );
    }

    // Create a per-agent command channel so viewers can send control
    // commands (MouseMove / MouseClick) through the server to this agent.
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<String>();
    state
        .agent_cmds
        .lock()
        .unwrap()
        .insert(agent_id, cmd_tx.clone());

    // Notify dashboard viewers.
    state.broadcast(
        serde_json::json!({
            "event":    "agent_connected",
            "agent_id": agent_id,
            "name":     name,
            "connected_at": connected_at,
        })
        .to_string(),
    );

    // ── Message loop ──────────────────────────────────────────────────────────
    //
    // `select!` concurrently waits on:
    //   • inbound WS messages from the agent
    //   • outbound control commands forwarded by the viewer WS handler
    loop {
        tokio::select! {
            msg = ws.recv() => {
                match msg {
                    Some(Ok(Message::Binary(bytes))) => {
                        // Cache the latest screenshot frame with a monotonically increasing sequence.
                        let mut frames = state.frames.lock().unwrap();
                        let next_seq = frames.get(&agent_id).map(|f| f.seq.saturating_add(1)).unwrap_or(1);
                        frames.insert(
                            agent_id,
                            Frame {
                                seq: next_seq,
                                jpeg: bytes::Bytes::from(bytes),
                            },
                        );
                    }
                    Some(Ok(Message::Text(text))) => {
                        dispatch_text(text.as_str(), agent_id, &name, &state).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }

            // Control command (MouseMove / MouseClick JSON) from a viewer.
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(cmd_str) => {
                        if ws.send(Message::Text(cmd_str)).await.is_err() {
                            break; // Agent disconnected.
                        }
                    }
                    None => break, // All senders dropped.
                }
            }
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    let disconnected_at = chrono::Utc::now();
    state.agents.lock().unwrap().remove(&agent_id);
    state.agent_cmds.lock().unwrap().remove(&agent_id);
    // Clear stale frame so MJPEG stream goes blank rather than serving the
    // last screenshot of a disconnected agent.
    state.frames.lock().unwrap().remove(&agent_id);
    let _ = db::touch_agent(&state.db, agent_id).await;
    let _ = db::end_agent_session(&state.db, session_id).await;

    state.broadcast(
        serde_json::json!({
            "event":    "agent_disconnected",
            "agent_id": agent_id,
            "disconnected_at": disconnected_at,
        })
        .to_string(),
    );

    info!("Agent disconnected: {name} ({agent_id})");
}

// ─── Event dispatching ────────────────────────────────────────────────────────

async fn dispatch_text(text: &str, agent_id: uuid::Uuid, name: &str, state: &Arc<AppState>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(text) else {
        warn!("Bad JSON from {agent_id}");
        return;
    };

    let kind = val["type"].as_str().unwrap_or("");

    let result = match kind {
        "keys" => db::upsert_keys(&state.db, agent_id, &val).await,
        "window_focus" => db::insert_window(&state.db, agent_id, &val).await,
        "url" => db::insert_url(&state.db, agent_id, &val).await,
        "afk" | "active" => db::insert_activity(&state.db, agent_id, &val).await,
        "agent_info" => db::upsert_agent_info(&state.db, agent_id, &val).await,
        other => {
            warn!("Unknown event type '{other}' from {agent_id}");
            Ok(())
        }
    };

    if let Err(e) = result {
        error!("DB error ({kind} / {agent_id}): {e}");
        return;
    }

    // Fan-out to all connected dashboard viewers.
    state.broadcast(
        serde_json::json!({
            "event":      kind,
            "agent_id":   agent_id,
            "agent_name": name,
            "data":       val,
        })
        .to_string(),
    );
}
