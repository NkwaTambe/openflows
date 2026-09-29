//! Versioned HTTP route composition for the OpenFlows Manager API.
//!
//! Operational probes (`/health`, `/ready`) are mounted unauthenticated at the
//! root for orchestrators. The product control surface lives under `/api/v1`
//! and is bearer-token authenticated at that edge (see `server::create_router`).

use crate::server::AppState;
use axum::{routing::get, Json, Router};
use serde::Serialize;

pub mod control;
pub mod health;
pub mod tasks;
pub mod tenants;

pub fn api_v1_router() -> Router<AppState> {
    Router::new()
        // The index route gives clients a cheap way to confirm the v1 mount
        // exists and that they are authenticated.
        .route("/", get(api_index))
        .nest("/tenants", tenants::router())
}

#[derive(Serialize)]
struct ApiIndexResponse {
    version: &'static str,
}

async fn api_index() -> Json<ApiIndexResponse> {
    Json(ApiIndexResponse { version: "v1" })
}
