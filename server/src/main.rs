//! Monitoring server
//!
//! ## Environment variables
//!
//! | Variable       | Default                                             |
//! |----------------|-----------------------------------------------------|
//! | `DATABASE_URL` | `postgres://monitor:monitor@localhost:5432/monitor` |
//! | `LISTEN_ADDR`  | `0.0.0.0:9000`                                      |
//! | `STATIC_DIR`   | `./static`                                          |
//! | `UI_PASSWORD`  | *(unset – open access)*                             |
//! | `RUST_LOG`     | `info`                                              |
//!
//! Set `UI_PASSWORD` to enable password protection for the dashboard.
//! The agent WebSocket (`/ws/agent`) is always unauthenticated so agents
//! can connect without browser cookies.

mod api;
mod auth;
mod db;
mod state;
mod ws_agent;
mod ws_viewer;

use std::sync::Arc;

use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Logging ───────────────────────────────────────────────────────────
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();

    // ── Database ──────────────────────────────────────────────────────────
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://monitor:monitor@localhost:5432/monitor".into());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&db_url)
        .await
        .map_err(|e| anyhow::anyhow!("Database connection failed: {e}"))?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    info!("Database ready.");

    // ── UI password ───────────────────────────────────────────────────────
    let ui_password = std::env::var("UI_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty());

    if ui_password.is_some() {
        info!("Dashboard password protection enabled.");
    } else {
        info!("Dashboard password protection disabled (set UI_PASSWORD to enable).");
    }

    // ── App state ─────────────────────────────────────────────────────────
    let state = Arc::new(state::AppState::new(pool, ui_password));

    // ── Routes ────────────────────────────────────────────────────────────
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into());

    // Auth endpoints — always open (needed to obtain / clear the session cookie).
    let auth_routes = Router::new()
        .route("/api/login",       post(auth::login))
        .route("/api/logout",      post(auth::logout))
        .route("/api/auth/status", get(auth::status));

    // Everything else requires a valid session when UI_PASSWORD is set.
    let protected = Router::new()
        .route("/ws/view", get(ws_viewer::handler))
        .nest("/api", api::router())
        .route_layer(from_fn_with_state(state.clone(), auth::require_auth));

    let app = Router::new()
        // Agent WebSocket — never gated by UI auth (agents use their own secret).
        .route("/ws/agent", get(ws_agent::handler))
        .merge(auth_routes)
        .merge(protected)
        // Static dashboard (index.html + assets) — served last as fallback.
        .fallback_service(
            ServeDir::new(&static_dir).append_index_html_on_directories(true),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    // ── Listen ────────────────────────────────────────────────────────────
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:9000".into());
    info!("Listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
