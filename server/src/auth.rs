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
    req: Request,
    next: Next,
) -> Response {
    if state.ui_password.is_none() {
        return if state.allow_insecure_dashboard_open {
            next.run(req).await
        } else {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Dashboard auth not configured" })),
            )
                .into_response()
        };
    }

    let extracted_session = extract_session(req.headers());
    let authenticated = match extracted_session {
        Some(t) => match state.sessions.lock() {
            Ok(guard) => guard.contains(&t),
            Err(_) => false, // lock poisoned: fail closed
        },
        None => false,
    };

    if authenticated {
        next.run(req).await
    } else {
        // Fail closed: don't log session tokens.
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
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let Some(ref expected) = state.ui_password else {
        if state.allow_insecure_dashboard_open {
            // No password configured — always succeed without a cookie.
            return Json(serde_json::json!({ "ok": true })).into_response();
        }

        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "UI_PASSWORD not configured" })),
        )
            .into_response();
    };

    // Secure timing attack mitigation with constant-time equality.
    let is_equal = subtle::ConstantTimeEq::ct_eq(body.password.as_bytes(), expected.as_bytes());
    if !bool::from(is_equal) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Wrong password" })),
        )
            .into_response();
    }

    let token = uuid::Uuid::new_v4().to_string();
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.insert(token.clone());
    } else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Session store unavailable" })),
        )
            .into_response();
    }
    info!("New dashboard session created.");

    // Auto-detect HTTPS from Traefik's X-Forwarded-Proto header, or fall back
    // to the COOKIE_SECURE env var. This ensures the Secure cookie attribute
    // is set automatically when running behind a TLS-terminating reverse proxy.
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let secure = forwarded_proto == "https"
        || std::env::var("COOKIE_SECURE")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

    // Use SameSite=None when Secure is set so the cookie is sent on
    // non-top-level requests (including WebSocket upgrades) in more
    // deployment/proxy scenarios.
    //
    // Browsers require Secure when SameSite=None; we only emit None in the
    // Secure branch.
    let cookie = if secure {
        format!(
            "session={}; HttpOnly; Secure; SameSite=None; Path=/; Max-Age=86400",
            token,
        )
    } else {
        // Local development / HTTP: keep a restrictive default.
        format!(
            "session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400",
            token,
        )
    };
    (
        [(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap())],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

/// `POST /api/logout` — revoke the current session cookie.
pub async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(token) = extract_session(&headers) {
        if let Ok(mut sessions) = state.sessions.lock() {
            sessions.remove(&token);
        }
        info!("Dashboard session revoked.");
    }

    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let secure = forwarded_proto == "https"
        || std::env::var("COOKIE_SECURE")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

    let clear = if secure {
        "session=; HttpOnly; Secure; SameSite=None; Path=/; Max-Age=0"
    } else {
        "session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
    };
    (
        [(header::SET_COOKIE, HeaderValue::from_static(clear))],
        StatusCode::OK,
    )
        .into_response()
}

/// `GET /api/auth/status` — let the SPA check whether it is already authenticated.
pub async fn status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if state.ui_password.is_none() {
        if state.allow_insecure_dashboard_open {
            return Json(serde_json::json!({
                "authenticated":     true,
                "password_required": false,
            }))
            .into_response();
        }

        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "authenticated":     false,
                "password_required": true,
                "error":             "UI_PASSWORD not configured",
            })),
        )
            .into_response();
    }

    let authenticated = match extract_session(&headers) {
        Some(t) => match state.sessions.lock() {
            Ok(guard) => guard.contains(&t),
            Err(_) => false,
        },
        None => false,
    };

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
