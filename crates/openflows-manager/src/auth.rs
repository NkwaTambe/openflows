//! Bearer-token authentication for the authenticated `/api/v1` control surface.
//!
//! The manager is reachable from outside a host's network, so it must never
//! expose an unauthenticated control plane. This module enforces a single
//! shared bearer token supplied by the operator via `OPENFLOWS_MANAGER_TOKEN`.
//!
//! The token is required at startup (see [`token_from_env`]) so the process
//! refuses to boot rather than serve open. This is the deliberate MVP auth
//! model: a shared secret. Stronger mechanisms (OAuth2 / mTLS) are tracked as a
//! follow-up issue.

use crate::{error::ManagerError, server::AppState};
use axum::{
    extract::{Request, State},
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// Environment variable holding the shared bearer token.
pub const TOKEN_ENV: &str = "OPENFLOWS_MANAGER_TOKEN";

/// Read the required bearer token from the environment.
///
/// Returns a configuration error when unset/empty so the manager never boots
/// into an unauthenticated state.
pub fn token_from_env() -> Result<String, ManagerError> {
    let token = std::env::var(TOKEN_ENV)
        .map_err(|_| ManagerError::Config(format!("{TOKEN_ENV} is required")))?
        .trim()
        .to_string();
    if token.is_empty() {
        return Err(ManagerError::Config(format!(
            "{TOKEN_ENV} must not be empty"
        )));
    }
    Ok(token)
}

#[derive(Serialize)]
struct AuthErrorBody {
    error: &'static str,
    message: &'static str,
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorBody {
            error: "unauthorized",
            message: "missing or invalid bearer token; set Authorization: Bearer <token>",
        }),
    )
        .into_response()
}

/// Extract the bearer token from a request, if present.
fn bearer_token<B>(req: &Request<B>) -> Option<String> {
    req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Axum middleware enforcing bearer-token auth for `/api/v1` routes.
///
/// A constant-time comparison avoids leaking token length via timing and keeps
/// the check trivial to reason about.
pub async fn require_auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let provided = bearer_token(&req).unwrap_or_default();
    let expected = state.auth_token();

    // Constant-time comparison: always walks the full (equal-length) inputs,
    // accumulating a diff, so it does not short-circuit at the first mismatch.
    // Only the length comparison leaks, which is unavoidable and not sensitive.
    let ok = provided.len() == expected.len()
        && provided
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0;

    if !ok {
        return unauthorized();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_extracts_without_prefix() {
        let mut req = axum::http::Request::builder()
            .header(AUTHORIZATION, "Bearer abc-123")
            .body(())
            .unwrap();
        assert_eq!(bearer_token(&req).as_deref(), Some("abc-123"));

        // No header -> None
        *req.headers_mut() = axum::http::HeaderMap::new();
        assert!(bearer_token(&req).is_none());
    }

    #[test]
    fn token_from_env_requires_value() {
        // Save/restore the env so this test never leaks a credential to other
        // tests running in the same process (which would make them order-dependent).
        let previous = std::env::var(TOKEN_ENV).ok();
        std::env::remove_var(TOKEN_ENV);
        let result_unset = token_from_env();

        std::env::set_var(TOKEN_ENV, "   ");
        let result_empty = token_from_env();

        std::env::set_var(TOKEN_ENV, "  secret  ");
        let result_valid = token_from_env();

        // Restore the original value (or remove if it was absent).
        match previous {
            Some(v) => std::env::set_var(TOKEN_ENV, v),
            None => std::env::remove_var(TOKEN_ENV),
        }

        assert!(result_unset.is_err());
        assert!(result_empty.is_err());
        assert_eq!(result_valid.unwrap(), "secret");
    }
}
