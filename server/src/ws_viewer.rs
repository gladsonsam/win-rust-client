//! WebSocket handler for dashboard viewers.
//!
//! Dashboards connect to `ws://<host>/ws/view`.
//!
//! ## Viewer → server messages
//!
//! ```json
//! { "type": "control", "agent_id": "<uuid>", "cmd": { "type": "MouseMove", "x": 100, "y": 200 } }
//! { "type": "control", "agent_id": "<uuid>", "cmd": { "type": "MouseClick", "x": 100, "y": 200, "button": "Left" } }
//! ```
//!
//! The server looks up the agent by UUID and forwards the `cmd` JSON to it
//! via the per-agent command channel registered in `AppState::agent_cmds`.
//!
//! ## Server → viewer messages
//!
//! On connect: `{ "event": "init", "agents": [...] }`
//! Then real-time: every telemetry event broadcast by `ws_agent`.

use std::sync::Arc;

use axum::extract::ws::WebSocket;
use axum::{
    extract::{ws::Message, State, WebSocketUpgrade},
    response::IntoResponse,
};
use tokio::sync::broadcast::error::RecvError;
use tracing::{info, warn};
use uuid::Uuid;

use crate::state::{AppState, Broadcast};

// Conservative bounds for viewer -> server control messages.
// This prevents large JSON objects from turning into expensive parses or
// unbounded command payload forwarding.
const MAX_VIEWER_TEXT_BYTES: usize = 64 * 1024;
const MAX_TYPE_TEXT_CHARS: usize = 2_000;
const MAX_NOTIFY_TITLE_CHARS: usize = 64;
const MAX_NOTIFY_MESSAGE_CHARS: usize = 256;

pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| run(socket, state))
}

async fn run(mut ws: WebSocket, state: Arc<AppState>) {
    // ── Send initial agent list (includes offline agents + last session times) ──
    let agents = match crate::db::list_agents(&state.db).await {
        Ok(rows) => rows,
        Err(_) => Vec::new(),
    };

    let online: std::collections::HashMap<uuid::Uuid, chrono::DateTime<chrono::Utc>> = {
        let map = state.agents.lock().unwrap();
        map.iter().map(|(id, a)| (*id, a.connected_at)).collect()
    };

    let mut out: Vec<serde_json::Value> = Vec::with_capacity(agents.len());
    for a in agents {
        let id = match a["id"].as_str().and_then(|s| s.parse::<Uuid>().ok()) {
            Some(id) => id,
            None => continue,
        };
        let (last_connected_at, last_disconnected_at) =
            crate::db::agent_last_session_times(&state.db, id)
                .await
                .unwrap_or((None, None));
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

    let init = serde_json::json!({ "event": "init", "agents": out }).to_string();
    if ws.send(Message::Text(init)).await.is_err() {
        return;
    }

    // ── Subscribe to live events ──────────────────────────────────────────────
    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            // Broadcast from an agent handler → forward to this viewer.
            msg = rx.recv() => {
                match msg {
                    Ok(Broadcast::Text(text)) => {
                        if ws.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                    Err(RecvError::Lagged(n)) => {
                        warn!("Viewer lagged, dropped {n} messages");
                    }
                }
            }

            // Message from the viewer.
            frame = ws.recv() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        handle_viewer_message(&text, &state);
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    info!("Viewer disconnected.");
}

// ─── Viewer → agent control forwarding ───────────────────────────────────────

fn handle_viewer_message(text: &str, state: &Arc<AppState>) {
    if text.len() > MAX_VIEWER_TEXT_BYTES {
        warn!(
            "Dropping viewer message: payload too large ({} bytes)",
            text.len()
        );
        return;
    }

    let Ok(val) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };

    if val["type"].as_str() != Some("control") {
        return;
    }

    let Some(agent_id_str) = val["agent_id"].as_str() else {
        return;
    };
    let Ok(agent_id) = agent_id_str.parse::<Uuid>() else {
        return;
    };

    // Validate command "shape" before forwarding to the agent.
    let cmd_type = val["cmd"]["type"].as_str().unwrap_or("");
    let cmd_ok = match cmd_type {
        "MouseMove" => {
            let x_ok = val["cmd"]["x"]
                .as_i64()
                .map(|v| v >= i32::MIN as i64 && v <= i32::MAX as i64)
                .unwrap_or(false);
            let y_ok = val["cmd"]["y"]
                .as_i64()
                .map(|v| v >= i32::MIN as i64 && v <= i32::MAX as i64)
                .unwrap_or(false);
            x_ok && y_ok
        }
        "MouseClick" => {
            let x_ok = val["cmd"]["x"]
                .as_i64()
                .map(|v| v >= i32::MIN as i64 && v <= i32::MAX as i64)
                .unwrap_or(false);
            let y_ok = val["cmd"]["y"]
                .as_i64()
                .map(|v| v >= i32::MIN as i64 && v <= i32::MAX as i64)
                .unwrap_or(false);
            let button_ok = val["cmd"]["button"]
                .as_str()
                .map(|b| matches!(b, "left" | "right" | "middle"))
                .unwrap_or(true); // omitted => defaults to "left" in the agent
            x_ok && y_ok && button_ok
        }
        "TypeText" => val["cmd"]["text"]
            .as_str()
            .map(|s| s.chars().count() <= MAX_TYPE_TEXT_CHARS)
            .unwrap_or(false),
        "KeyPress" => val["cmd"]["key"]
            .as_str()
            .map(|k| matches!(k, "enter" | "backspace" | "tab" | "escape"))
            .unwrap_or(false),
        "Notify" => {
            let title_ok = val["cmd"]["title"]
                .as_str()
                .map(|s| s.chars().count() <= MAX_NOTIFY_TITLE_CHARS)
                .unwrap_or(false);
            let msg_ok = val["cmd"]["message"]
                .as_str()
                .map(|s| s.chars().count() <= MAX_NOTIFY_MESSAGE_CHARS)
                .unwrap_or(false);
            title_ok && msg_ok
        }
        _ => false,
    };

    if !cmd_ok {
        warn!("Dropping viewer control command: invalid cmd type/shape");
        return;
    }

    // Serialise just the `cmd` sub-object and forward it to the agent.
    let cmd = serde_json::to_string(&val["cmd"]).unwrap_or_default();
    if cmd.is_empty() || cmd == "null" {
        return;
    }

    // WebSocket-first mode: forward commands to the connected agent over its
    // per-agent command channel.
    let sent = state
        .agent_cmds
        .lock()
        .unwrap()
        .get(&agent_id)
        .map(|tx| tx.send(cmd).is_ok());

    if sent == Some(false) {
        warn!("Agent {agent_id} command channel closed");
    }
}
