//! Error types shared by the OpenFlows Manager binary and HTTP server.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Service(#[from] anyhow::Error),

    /// A client supplied an invalid request (maps to HTTP 400).
    #[error("{0}")]
    BadRequest(String),

    /// The named resource does not exist (maps to HTTP 404).
    #[error("{0}")]
    NotFound(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    message: String,
}

impl IntoResponse for ManagerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ManagerError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ManagerError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            // Internal failures never leak internal details (URLs, tokens) to
            // the client — log locally and return a sanitized 500.
            other => {
                tracing::error!(error = %other, "OpenFlows Manager request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };
        (
            status,
            Json(ErrorBody {
                error: status.canonical_reason().unwrap_or("error").to_string(),
                message,
            }),
        )
            .into_response()
    }
}
