//! Tenant management routes for the OpenFlows Manager control surface.
//!
//! These endpoints let an operator manage tenants and steer a tenant's
//! control-plane state over HTTP, without direct access to Redis or Coder.

use crate::{error::ManagerError, server::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::control::{self, ControlMode};
use super::tasks;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tenants).post(add_tenant))
        .route("/{name}/control", get(get_control).put(set_control))
        .route("/{name}/tasks", axum::routing::post(tasks::assign_task))
}

/// Whether a tenant has been registered on the control surface (has any key in
/// its `ns:{name}:*` namespace).
pub async fn tenant_exists(
    store: &pocketflow_core::SharedStore,
    name: &str,
) -> Result<bool, ManagerError> {
    // raw_keys returns the full matching keys; a registered tenant has at least
    // its repository binding (or a control-mode key) present.
    let keys: Vec<String> = store.raw_keys(&format!("ns:{}:*", name)).await;
    Ok(!keys.is_empty())
}

// ── Tenant listing ─────────────────────────────────────────────────────────

/// GET /api/v1/tenants
///
/// Enumerate every tenant from its Redis namespace (`ns:*`) and enrich each
/// with its control mode and bound repository when present.
pub async fn list_tenants(State(state): State<AppState>) -> Result<Json<Value>, ManagerError> {
    let store = state.store();
    let keys: Vec<String> = store.raw_keys("ns:*").await;

    let mut tenants: Vec<Value> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for key in keys {
        if let Some(ns) = key.strip_prefix("ns:") {
            if let Some(tenant) = ns.split(':').next() {
                if tenant.is_empty() || !seen.insert(tenant.to_string()) {
                    continue;
                }
                let mode = control::read_control_mode(store, tenant).await?;
                let repository = store
                    .raw_get(&format!("ns:{}:repository", tenant))
                    .await
                    .and_then(|v| v.as_str().map(String::from))
                    .filter(|s| !s.is_empty());
                tenants.push(json!({
                    "name": tenant,
                    "repository": repository,
                    "control_mode": mode.as_str(),
                }));
            }
        }
    }
    Ok(Json(json!({ "tenants": tenants })))
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
/// fleet as metadata) into the tenant's namespace, and initialize its control
/// mode to `auto` **only when the tenant is new**. Re-adding an existing tenant
/// preserves its current control mode (e.g. it stays paused), so an operator
/// cannot accidentally resume a fleet they paused for maintenance.
///
/// Provisioning the actual Coder workspace (and writing the fleet registry)
/// remains with the local `openflows tenant add`, which has access to Coder and
/// the bundled base registry. The manager records the control-surface state.
pub async fn add_tenant(
    State(state): State<AppState>,
    Json(req): Json<AddTenantRequest>,
) -> Result<(StatusCode, Json<Value>), ManagerError> {
    if !req.repo.contains('/') {
        return Err(ManagerError::BadRequest(
            "repo must be in 'owner/repo' format".to_string(),
        ));
    }
    let name = req
        .name
        .clone()
        .unwrap_or_else(|| req.repo.split('/').next().unwrap_or("").to_string());
    validate_tenant_name(&name).map_err(|e| ManagerError::BadRequest(e.to_string()))?;
    if let Some(fleet) = req.fleet {
        if fleet < 1 {
            return Err(ManagerError::BadRequest(
                "fleet must be >= 1 (a fleet of N means N FORGE-SENTINEL pairs)".to_string(),
            ));
        }
    }

    let store = state.store();
    let exists = tenant_exists(store, &name).await?;
    let repo_key = format!("ns:{}:repository", name);

    store.raw_set_result(&repo_key, json!(req.repo)).await?;

    // Persist fleet as lightweight metadata (the authoritative fleet registry is
    // written during provisioning by the local CLI, not fabricated here).
    if let Some(fleet) = req.fleet {
        store
            .raw_set_result(&format!("ns:{}:fleet", name), json!(fleet))
            .await?;
    }

    let status = if exists {
        // Existing tenant: leave its control mode untouched.
        StatusCode::OK
    } else {
        // New tenant starts in `auto`.
        control::write_control_mode(&state, &name, &ControlMode::Auto).await?;
        StatusCode::CREATED
    };

    let mode = control::read_control_mode(store, &name).await?;
    Ok((
        status,
        Json(json!({
            "name": name,
            "repo": req.repo,
            "fleet": req.fleet,
            "control_mode": mode.as_str(),
        })),
    ))
}

// ── Control mode ───────────────────────────────────────────────────────────

/// GET /api/v1/tenants/{name}/control
pub async fn get_control(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ManagerError> {
    if !tenant_exists(state.store(), &name).await? {
        return Err(ManagerError::NotFound(format!("no such tenant: '{name}'")));
    }
    let mode = control::read_control_mode(state.store(), &name).await?;
    Ok(Json(json!({
        "tenant": name,
        "control_mode": mode.as_str(),
    })))
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
) -> Result<Json<Value>, ManagerError> {
    if !tenant_exists(state.store(), &name).await? {
        return Err(ManagerError::NotFound(format!("no such tenant: '{name}'")));
    }

    let mode = match ControlMode::parse(&req.mode.to_lowercase()) {
        Some(m) => m,
        None => {
            return Err(ManagerError::BadRequest(format!(
                "invalid mode '{}' — expected one of: {}",
                req.mode,
                config::CONTROL_MODES.join(", ")
            )))
        }
    };

    // Only fully-implemented modes are accepted. `drained`/`targeted` are
    // documented but their fleet-steering semantics are not yet implemented by
    // the controller, so accepting them would let an operator set a state that
    // silently does nothing.
    if !matches!(mode, ControlMode::Auto | ControlMode::Paused) {
        return Err(ManagerError::BadRequest(format!(
            "mode '{}' is not yet implemented; supported modes: auto, paused",
            mode.as_str()
        )));
    }

    let previous = control::write_control_mode(&state, &name, &mode).await?;
    Ok(Json(json!({
        "tenant": name,
        "control_mode": mode.as_str(),
        "previous_control_mode": previous.as_str(),
    })))
}
