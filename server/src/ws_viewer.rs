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

use axum::{
    extract::{ws::Message, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::WebSocket;
use tokio::sync::broadcast::error::RecvError;
use tracing::{info, warn};
use uuid::Uuid;

use crate::state::{AppState, Broadcast};

pub async fn handler(
    ws:           WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| run(socket, state))
}

async fn run(mut ws: WebSocket, state: Arc<AppState>) {
    // ── Send initial agent list ───────────────────────────────────────────────
    let agents: Vec<_> = {
        let map = state.agents.lock().unwrap();
        map.values()
            .map(|a| {
                serde_json::json!({
                    "id":           a.id,
                    "name":         a.name,
                    "connected_at": a.connected_at,
                })
            })
            .collect()
    };

    let init = serde_json::json!({ "event": "init", "agents": agents }).to_string();
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

    // Serialise just the `cmd` sub-object and forward it to the agent.
    let cmd = serde_json::to_string(&val["cmd"]).unwrap_or_default();
    if cmd.is_empty() || cmd == "null" {
        return;
    }

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
