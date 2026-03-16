//! Shared application state, threaded through Axum via `Arc<AppState>`.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc::UnboundedSender};
use uuid::Uuid;

// ─── Agent info ───────────────────────────────────────────────────────────────

/// Metadata for a currently-connected agent.
#[derive(Debug, Clone)]
pub struct AgentConn {
    pub id:           Uuid,
    pub name:         String,
    pub connected_at: DateTime<Utc>,
}

// ─── Broadcast message ────────────────────────────────────────────────────────

/// A message fanned-out to every active dashboard viewer.
#[derive(Clone, Debug)]
pub enum Broadcast {
    /// Serialised JSON event (keystroke, window change, URL, etc.).
    Text(String),
}

// ─── App state ────────────────────────────────────────────────────────────────

pub struct AppState {
    /// Postgres connection pool.
    pub db: PgPool,

    /// Fan-out channel: every telemetry event is cloned to all viewers.
    pub tx: broadcast::Sender<Broadcast>,

    /// Currently-connected agents (keyed by DB UUID).
    pub agents: Mutex<HashMap<Uuid, AgentConn>>,

    /// Most-recent JPEG frame per agent – served by both the HTTP snapshot
    /// endpoint and the MJPEG stream.
    pub frames: Mutex<HashMap<Uuid, Vec<u8>>>,

    /// Per-agent command channels.
    ///
    /// Viewers send control JSON (MouseMove / MouseClick) to the server which
    /// looks up the target agent here and forwards the command string.  The
    /// agent's WebSocket handler drains its `Receiver` inside a `select!`.
    pub agent_cmds: Mutex<HashMap<Uuid, UnboundedSender<String>>>,

    /// Number of MJPEG viewers currently watching each agent.
    ///
    /// The MJPEG endpoint increments this on connect and decrements it when
    /// the HTTP connection closes (via a RAII [`CaptureGuard`]).
    /// - Count  0 → 1: send `{"type":"start_capture"}` to agent.
    /// - Count  1 → 0: send `{"type":"stop_capture"}` to agent.
    pub capture_viewers: Mutex<HashMap<Uuid, u32>>,
}

impl AppState {
    pub fn new(db: PgPool) -> Self {
        let (tx, _) = broadcast::channel(4096);
        Self {
            db,
            tx,
            agents:          Mutex::new(HashMap::new()),
            frames:          Mutex::new(HashMap::new()),
            agent_cmds:      Mutex::new(HashMap::new()),
            capture_viewers: Mutex::new(HashMap::new()),
        }
    }

    /// Send a JSON string to every connected viewer (fire-and-forget).
    pub fn broadcast(&self, msg: impl Into<String>) {
        let _ = self.tx.send(Broadcast::Text(msg.into()));
    }
}
