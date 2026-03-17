//! HTTP authentication for the dashboard UI.
//!
//! Password is read from the `UI_PASSWORD` environment variable at startup.
//! If the variable is unset or empty, the UI is fully open (no auth).
//!
//! ## Session lifecycle
//!
//! 1. `POST /api/login` with `{"password":"…"}` → server validates, stores a
//!    random UUID token in memory, and sets an `HttpOnly` cookie `session=<token>`.
//! 2. Every protected request reads the cookie and checks it against the
//!    in-memory set.
//! 3. `POST /api/logout` removes the token and clears the cookie.
//! 4. Sessions are in-memory only — they reset when the server restarts.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use tracing::info;

use crate::state::AppState;

// ─── Middleware ───────────────────────────────────────────────────────────────

/// Axum middleware: rejects requests without a valid session cookie.
/// Passes through unconditionally when no `UI_PASSWORD` is configured.
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    req:          Request,
    next:         Next,
) -> Response {
    if state.ui_password.is_none() {
        return next.run(req).await;
    }

    let authenticated = extract_session(req.headers())
        .map(|t| state.sessions.lock().unwrap().contains(&t))
        .unwrap_or(false);

    if authenticated {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Unauthorized" })),
        )
            .into_response()
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    password: String,
}

/// `POST /api/login` — validate password and issue a session cookie.
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(body):   Json<LoginRequest>,
) -> Response {
    let Some(ref expected) = state.ui_password else {
        // No password configured — always succeed without a cookie.
        return Json(serde_json::json!({ "ok": true })).into_response();
    };

    if body.password != *expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Wrong password" })),
        )
            .into_response();
    }

    let token  = uuid::Uuid::new_v4().to_string();
    state.sessions.lock().unwrap().insert(token.clone());
    info!("New dashboard session created.");

    let cookie = format!(
        "session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400",
        token,
    );
    (
        [(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap())],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

/// `POST /api/logout` — revoke the current session cookie.
pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers:      HeaderMap,
) -> Response {
    if let Some(token) = extract_session(&headers) {
        state.sessions.lock().unwrap().remove(&token);
        info!("Dashboard session revoked.");
    }
    let clear = "session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0";
    (
        [(header::SET_COOKIE, HeaderValue::from_static(clear))],
        StatusCode::OK,
    )
        .into_response()
}

/// `GET /api/auth/status` — let the SPA check whether it is already authenticated.
pub async fn status(
    State(state): State<Arc<AppState>>,
    headers:      HeaderMap,
) -> Response {
    let password_required = state.ui_password.is_some();

    if !password_required {
        return Json(serde_json::json!({
            "authenticated":     true,
            "password_required": false,
        }))
        .into_response();
    }

    let authenticated = extract_session(&headers)
        .map(|t| state.sessions.lock().unwrap().contains(&t))
        .unwrap_or(false);

    let status_code = if authenticated {
        StatusCode::OK
    } else {
        StatusCode::UNAUTHORIZED
    };

    (
        status_code,
        Json(serde_json::json!({
            "authenticated":     authenticated,
            "password_required": true,
        })),
    )
        .into_response()
}

// ─── Cookie helper ────────────────────────────────────────────────────────────

fn extract_session(headers: &HeaderMap) -> Option<String> {
    let cookie_str = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_str.split(';') {
        if let Some(val) = part.trim().strip_prefix("session=") {
            return Some(val.to_string());
        }
    }
    None
}
