//! Monitoring server
//!
//! ## Environment variables
//!
//! | Variable       | Default                                        |
//! |----------------|------------------------------------------------|
//! | `DATABASE_URL` | `postgres://monitor:monitor@localhost:5432/monitor` |
//! | `LISTEN_ADDR`  | `0.0.0.0:9000`                                |
//! | `STATIC_DIR`   | `./static`                                     |
//! | `RUST_LOG`     | `info`                                         |

mod api;
mod db;
mod state;
mod ws_agent;
mod ws_viewer;

use std::sync::Arc;

use axum::{routing::get, Router};
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

    // Run embedded migrations (baked in at compile time from ./migrations/).
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    info!("Database ready.");

    // ── App state ─────────────────────────────────────────────────────────
    let state = Arc::new(state::AppState::new(pool));

    // ── Routes ────────────────────────────────────────────────────────────
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into());

    let app = Router::new()
        // Agent WebSocket  – agents connect here
        .route("/ws/agent", get(ws_agent::handler))
        // Viewer WebSocket – dashboard connects here for live events
        .route("/ws/view", get(ws_viewer::handler))
        // REST API
        .nest("/api", api::router())
        // Static dashboard (index.html + assets)
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
