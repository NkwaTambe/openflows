//! Tenant management routes for the OpenFlows Manager control surface.
//!
//! These endpoints let an operator manage tenants and steer a tenant's
//! control-plane state over HTTP, without direct access to Redis or Coder.

use crate::server::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::control::{self, ControlMode};
use super::tasks;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tenants).post(add_tenant))
        .route("/{name}/control", get(get_control).put(set_control))
        .route("/{name}/tasks", axum::routing::post(tasks::assign_task))
}

// ── Tenant listing ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TenantSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub control_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ListTenantsResponse {
    pub tenants: Vec<TenantSummary>,
}

/// GET /api/v1/tenants
///
/// Enumerate every tenant from its Redis namespace (`ns:*`) and enrich each
/// with its control mode and bound repository when present.
pub async fn list_tenants(State(state): State<AppState>) -> Json<ListTenantsResponse> {
    let store = state.store();
    let keys: Vec<String> = store.raw_keys("ns:*").await;

    let mut tenants: Vec<TenantSummary> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for key in keys {
        if let Some(ns) = key.strip_prefix("ns:") {
            if let Some(tenant) = ns.split(':').next() {
                if tenant.is_empty() || !seen.insert(tenant.to_string()) {
                    continue;
                }
                let mode = control::read_control_mode(store, tenant).await;
                let repository = store
                    .raw_get(&format!("ns:{}:repository", tenant))
                    .await
                    .and_then(|v| v.as_str().map(String::from))
                    .filter(|s| !s.is_empty());
                tenants.push(TenantSummary {
                    name: tenant.to_string(),
                    repository,
                    control_mode: mode.as_str().to_string(),
                });
            }
        }
    }
    Json(ListTenantsResponse { tenants })
}

// ── Tenant add ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddTenantRequest {
    /// GitHub repository in `owner/repo` format.
    pub repo: String,
    /// Tenant name; defaults to the repo owner when omitted.
    #[serde(default)]
    pub name: Option<String>,
    /// Number of FORGE-SENTINEL pairs (fleet size); must be >= 1 when set.
    #[serde(default)]
    pub fleet: Option<u32>,
}

fn validate_tenant_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("tenant name must not be empty");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("tenant name may only contain ASCII letters, numbers, '.', '_' and '-'");
    }
    Ok(())
}

/// POST /api/v1/tenants
///
/// Register a tenant on the control surface: persist its bound repository (and
/// fleet-derived registry when supplied) into the tenant's namespace. This is
/// the control-plane reflection of `openflows tenant add`. Provisioning the
/// actual Coder workspace remains with the local CLI until the manager gains a
/// Coder provisioning seam.
pub async fn add_tenant(
    State(state): State<AppState>,
    Json(req): Json<AddTenantRequest>,
) -> impl IntoResponse {
    // owner/repo shape check.
    if !req.repo.contains('/') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "repo must be in 'owner/repo' format"
            })),
        );
    }
    let name = req
        .name
        .clone()
        .unwrap_or_else(|| req.repo.split('/').next().unwrap_or("").to_string());
    if let Err(e) = validate_tenant_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        );
    }
    if let Some(fleet) = req.fleet {
        if fleet < 1 {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "fleet must be >= 1 (a fleet of N means N FORGE-SENTINEL pairs)"
                })),
            );
        }
    }

    let store = state.store();
    store
        .raw_set(
            &format!("ns:{}:repository", name),
            serde_json::json!(req.repo),
        )
        .await;
    // If a fleet is supplied, persist a fleet-derived registry so the tenant's
    // controller can size its FORGE-SENTINEL slots without a local file.
    if let Some(fleet) = req.fleet {
        let registry = fleet_registry(fleet);
        store
            .raw_set(
                &format!("ns:{}:registry_json", name),
                serde_json::json!(registry),
            )
            .await;
    }
    // A freshly added tenant starts in `auto`.
    let mode = control::write_control_mode(&state, &name, &ControlMode::Auto).await;

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "name": name,
            "repo": req.repo,
            "fleet": req.fleet,
            "control_mode": mode.as_str(),
        })),
    )
}

/// Build a minimal fleet-sized registry JSON. Mirrors the local CLI's
/// `Registry::with_team_fleet` so a manager-added tenant carries its fleet.
fn fleet_registry(fleet: u32) -> String {
    serde_json::json!({
        "fleet": fleet,
        "forge": { "instances": fleet },
        "sentinel": { "instances": fleet }
    })
    .to_string()
}

// ── Control mode ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ControlResponse {
    pub tenant: String,
    pub control_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_control_mode: Option<String>,
}

/// GET /api/v1/tenants/{name}/control
pub async fn get_control(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<ControlResponse> {
    let mode = control::read_control_mode(state.store(), &name).await;
    Json(ControlResponse {
        tenant: name,
        control_mode: mode.as_str().to_string(),
        previous_control_mode: None,
    })
}

#[derive(Debug, Deserialize)]
pub struct SetControlRequest {
    pub mode: String,
}

/// PUT /api/v1/tenants/{name}/control
pub async fn set_control(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SetControlRequest>,
) -> impl IntoResponse {
    let mode = match ControlMode::parse(&req.mode.to_lowercase()) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "invalid mode '{}' — expected one of: {}",
                        req.mode,
                        config::CONTROL_MODES.join(", ")
                    )
                })),
            )
        }
    };
    let previous = control::write_control_mode(&state, &name, &mode).await;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "tenant": name,
            "control_mode": mode.as_str(),
            "previous_control_mode": previous.as_str(),
        })),
    )
}
