//! Direct task assignment to a tenant.
//!
//! An operator can push work into a tenant without waiting for a GitHub issue
//! by posting a task. The task is injected into the tenant's **ticket queue**
//! (as an open ticket) so the controller actually assigns and executes it — it
//! is not a dead-letter that nothing consumes.
//!
//! All writes use atomic store primitives (a Lua script on Redis) so concurrent
//! assignments and the controller's own issue-sync cannot lose a ticket, and so
//! a malformed stored value fails instead of silently erasing the queue.

use crate::{error::ManagerError, server::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pocketflow_core::SharedStore;
use serde::{Deserialize, Serialize};

/// Fully-qualified Redis key holding a tenant's assigned tasks.
pub fn tasks_key(tenant: &str) -> String {
    format!("ns:{}:tasks", tenant)
}

fn tickets_key(tenant: &str) -> String {
    format!("ns:{}:{}", tenant, config::KEY_TICKETS)
}

fn ticket_counter_key(tenant: &str) -> String {
    format!("ns:{}:ticket_counter", tenant)
}

/// A unit of work assigned directly to a tenant through the control surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    /// Opaque caller-supplied payload (e.g. structured task fields).
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: u64,
}

/// Request body for `POST /api/v1/tenants/{name}/tasks`.
#[derive(Debug, Deserialize)]
pub struct AssignTaskRequest {
    /// Human-readable task title.
    pub title: String,
    /// Optional detail / acceptance text.
    #[serde(default)]
    pub body: String,
    /// Where the task came from (e.g. `cli`, `api`).
    #[serde(default = "default_source")]
    pub source: String,
    /// Optional structured payload carried alongside the task.
    #[serde(default)]
    pub payload: serde_json::Value,
}

fn default_source() -> String {
    "api".to_string()
}

/// Inject a task into a tenant's queue.
///
/// Ordering matters for correctness: the **ticket is dispatched first** so an
/// assignment never leaves an orphaned task record with no executable ticket.
/// The task record is bookkeeping; if it cannot be written the work is already
/// queued, so we surface a warning rather than an error for the lost record.
/// IDs come from an atomic counter so concurrent assignments never collide.
pub async fn append_task(
    store: &SharedStore,
    tenant: &str,
    req: AssignTaskRequest,
) -> Result<Task, ManagerError> {
    // Seed the id counter from the existing ticket count when it is first used,
    // so a tenant that already has tickets (e.g. after an upgrade, or from
    // earlier assignments before this counter existed) never gets a repeating
    // `T-CLI-*` id. SETNX makes this race-safe: concurrent first-use only sets
    // it once, and INCR then produces unique, strictly-increasing ids.
    let counter_key = ticket_counter_key(tenant);
    let existing = store
        .raw_get_result(&tickets_key(tenant))
        .await
        .map_err(ManagerError::Service)?
        .and_then(|v| serde_json::from_value::<Vec<serde_json::Value>>(v).ok())
        .map(|t| t.len())
        .unwrap_or(0);
    store
        .raw_set_if_absent(&counter_key, serde_json::json!(existing))
        .await
        .map_err(ManagerError::Service)?;
    let n = store.raw_incr(&counter_key).await?;

    // Dispatch first: atomically append the open ticket.
    let ticket = config::Ticket {
        id: format!("T-CLI-{n:03}"),
        title: req.title.clone(),
        body: req.body.clone(),
        priority: 0,
        branch: None,
        status: config::TicketStatus::Open,
        issue_url: None,
        attempts: 0,
    };
    store
        .raw_append(
            &tickets_key(tenant),
            serde_json::to_value(&ticket).unwrap_or_default(),
        )
        .await
        .map_err(ManagerError::Service)?;

    // Then record the task (bookkeeping). A failure here must not fail the
    // assignment, because the work is already queued — but it is worth logging.
    let task = Task {
        id: format!("task-{n:03}"),
        title: req.title,
        body: req.body,
        source: req.source,
        payload: req.payload,
        created_at: now_millis(),
    };
    if let Err(e) = store
        .raw_append(
            &tasks_key(tenant),
            serde_json::to_value(&task).unwrap_or_default(),
        )
        .await
    {
        tracing::warn!(
            tenant,
            task_id = %task.id,
            error = %e,
            "task dispatched as a ticket but the task record could not be persisted"
        );
    }
    Ok(task)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// POST /api/v1/tenants/{name}/tasks
pub async fn assign_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<AssignTaskRequest>,
) -> Result<impl IntoResponse, ManagerError> {
    if req.title.trim().is_empty() {
        return Err(ManagerError::BadRequest("title is required".to_string()));
    }
    // Validate the name before it becomes a Redis key / scan pattern, then
    // reject unknown tenants so an operator cannot believe they assigned work
    // to a fleet that was never registered.
    crate::routes::tenants::validate_tenant_name(&name)
        .map_err(|e| ManagerError::BadRequest(e.to_string()))?;
    if !crate::routes::tenants::tenant_exists(state.store(), &name).await? {
        return Err(ManagerError::NotFound(format!("no such tenant: '{name}'")));
    }
    let task = append_task(state.store(), &name, req).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "tenant": name, "task": task })),
    ))
}
