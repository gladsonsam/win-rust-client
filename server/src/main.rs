//! Sentinel – monitoring server
//!
//! ## Environment variables
//!
//! | Variable       | Default                                             |
//! |----------------|-----------------------------------------------------|
//! | `DATABASE_URL` | `postgres://monitor:monitor@localhost:5432/monitor` |
//! | `LISTEN_ADDR`  | `0.0.0.0:9000`                                      |
//! | `STATIC_DIR`   | `./static`                                          |
//! | `UI_PASSWORD`  | *(unset – deny access; set ALLOW_INSECURE_DASHBOARD_OPEN=true to allow)* |
//! | `RUST_LOG`     | `info`                                              |
//!
//! Set `UI_PASSWORD` to enable password protection for the dashboard.
//! The agent WebSocket (`/ws/agent`) uses a shared secret (`AGENT_SECRET`)
//! for auth (agents can still connect without browser cookies).

mod api;
mod auth;
mod db;
mod state;
mod ws_agent;
mod ws_viewer;

use std::sync::Arc;

use axum::{
    extract::Request,
    extract::State,
    http::StatusCode,
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use axum::http::header::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use axum::response::IntoResponse;
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
    let db_url = read_env_or_file("DATABASE_URL")
        .unwrap_or_else(|| "postgres://monitor:monitor@localhost:5432/monitor".into());

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

    // ── Periodic telemetry retention (keys / windows+activity / URLs) ───────
    let pool_retention = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = db::prune_telemetry_by_retention(&pool_retention).await {
            tracing::warn!(error = %e, "initial retention prune failed");
        }
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(e) = db::prune_telemetry_by_retention(&pool_retention).await {
                tracing::warn!(error = %e, "retention prune failed");
            }
        }
    });

    // ── UI password ───────────────────────────────────────────────────────
    let ui_password = read_env_or_file("UI_PASSWORD").filter(|s| !s.is_empty());
    let allow_insecure_dashboard_open = read_env_or_file("ALLOW_INSECURE_DASHBOARD_OPEN")
        .map(|v| parse_bool(&v))
        .unwrap_or(false);

    if ui_password.is_some() {
        info!("Dashboard password protection enabled.");
    } else {
        if allow_insecure_dashboard_open {
            info!("Dashboard password protection disabled (insecure opt-in).");
        } else {
            info!("Dashboard password protection disabled (deny access; set UI_PASSWORD).");
        }
    }

    // ── App state ─────────────────────────────────────────────────────────
    let agent_secret = read_env_or_file("AGENT_SECRET").filter(|s| !s.is_empty());
    let allow_insecure_agent_auth = read_env_or_file("ALLOW_INSECURE_AGENT_AUTH")
        .map(|v| parse_bool(&v))
        .unwrap_or(false);
    if agent_secret.is_some() {
        info!("Agent authentication enabled (AGENT_SECRET set).");
    } else {
        if allow_insecure_agent_auth {
            info!("Agent authentication disabled (insecure opt-in).");
        } else {
            info!("Agent authentication disabled (deny agent connections; set AGENT_SECRET).");
        }
    }

    let state = Arc::new(state::AppState::new(
        pool,
        ui_password,
        allow_insecure_dashboard_open,
        agent_secret,
        allow_insecure_agent_auth,
    ));

    // ── Routes ────────────────────────────────────────────────────────────
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into());

    // Healthcheck endpoint for containers / load balancers.
    let health_routes = Router::new().route("/healthz", get(|| async { (StatusCode::OK, "ok") }));

    // Auth endpoints — always open (needed to obtain / clear the session cookie).
    let auth_routes = Router::new()
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/auth/status", get(auth::status));

    // Everything else requires a valid session when UI_PASSWORD is set.
    let protected = Router::new()
        .route("/ws/view", get(ws_viewer::handler))
        .nest("/api", api::router())
        .route_layer(from_fn_with_state(state.clone(), auth::require_auth));

    let app = Router::new()
        // Agent WebSocket — never gated by UI auth (agents use their own secret).
        .route("/ws/agent", get(ws_agent::handler))
        .merge(health_routes)
        .merge(auth_routes)
        .merge(protected)
        // Static dashboard (index.html + assets) — served last as fallback.
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        // CORS:
        // - default permissive to preserve current dev behavior
        // - set CORS_ORIGINS="https://dashboard.example.com,https://other.example.com"
        //   to restrict in production.
        .layer(cors_layer_from_env())
        .layer(from_fn_with_state(
            // Enforce HTTPS via X-Forwarded-Proto. This works when you
            // terminate TLS at a reverse proxy (Traefik, Nginx, etc.).
            https_enforced(),
            require_https,
        ))
        .with_state(state);

    // ── Listen ────────────────────────────────────────────────────────────
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:9000".into());
    info!("Listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn https_enforced() -> bool {
    // Default off to avoid breaking existing local setups that don't have
    // X-Forwarded-Proto (plain `http://:9000`).
    std::env::var("ENFORCE_HTTPS")
        .ok()
        .map(|v| parse_bool(&v))
        .unwrap_or(true)
}

async fn require_https(
    State(enforce): State<bool>,
    req: Request,
    next: Next,
) -> Response {
    if !enforce {
        return next.run(req).await;
    }

    // Allow health checks without HTTPS enforcement.
    if req.uri().path() == "/healthz" {
        return next.run(req).await;
    }

    let forwarded_proto = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Some reverse proxies set X-Forwarded-Proto to `wss` for WebSocket
    // upgrades. Treat it as valid because it still means TLS.
    if forwarded_proto.eq_ignore_ascii_case("https")
        || forwarded_proto.eq_ignore_ascii_case("wss")
    {
        next.run(req).await
    } else {
        (
            StatusCode::UPGRADE_REQUIRED,
            "HTTPS required (set ENFORCE_HTTPS=false for local HTTP testing).",
        )
            .into_response()
    }
}

fn cors_layer_from_env() -> CorsLayer {
    let raw = std::env::var("CORS_ORIGINS").unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        // Dev/default behavior: don't actively constrain CORS.
        return CorsLayer::permissive();
    }

    let origins: Vec<HeaderValue> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<HeaderValue>().ok())
        .collect();

    if origins.is_empty() {
        return CorsLayer::permissive();
    }

    // Since the dashboard uses cookie auth, we must allow credentials when
    // cross-origin requests are expected.
    CorsLayer::new()
        .allow_origin(origins)
        .allow_credentials(true)
}

/// Read config either from `NAME` or `NAME_FILE` (Docker secrets pattern).
fn read_env_or_file(name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(name) {
        return Some(val);
    }
    let file_key = format!("{name}_FILE");
    let path = std::env::var(file_key).ok()?;
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim(),
        "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
    )
}
