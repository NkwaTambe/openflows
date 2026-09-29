//! Direct task assignment to a tenant.
//!
//! An operator can push work into a tenant without waiting for a GitHub issue
//! by posting a task. The task is recorded under `ns:{tenant}:tasks` AND
//! injected into the tenant's `tickets` queue (as an open ticket) so the
//! controller actually assigns and executes it — it is not a dead-letter that
//! nothing consumes.

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
/// Records the task and appends a matching open `Ticket` to the tenant's
/// `tickets` list so the controller picks it up on its next poll. Errors from
/// the backing store are propagated rather than swallowed.
pub async fn append_task(
    store: &SharedStore,
    tenant: &str,
    req: AssignTaskRequest,
) -> Result<Task, ManagerError> {
    let mut tasks: Vec<Task> = store
        .raw_get_result(&tasks_key(tenant))
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let task = Task {
        id: generate_task_id(&tasks),
        title: req.title.clone(),
        body: req.body.clone(),
        source: req.source.clone(),
        payload: req.payload.clone(),
        created_at: now_millis(),
    };
    tasks.push(task.clone());
    store
        .raw_set_result(
            &tasks_key(tenant),
            serde_json::to_value(&tasks).unwrap_or_default(),
        )
        .await?;

    // Inject an open ticket so the task actually runs. It shares the task's
    // title/body and carries a stable id in the ticket namespace.
    let tickets_key = format!("ns:{}:{}", tenant, config::KEY_TICKETS);
    let mut tickets: Vec<config::Ticket> = store
        .raw_get_result(&tickets_key)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let ticket_id = format!("T-CLI-{:03}", tickets.len() + 1);
    let ticket = config::Ticket {
        id: ticket_id,
        title: task.title.clone(),
        body: task.body.clone(),
        priority: 0,
        branch: None,
        status: config::TicketStatus::Open,
        issue_url: None,
        attempts: 0,
    };
    tickets.push(ticket);
    store
        .raw_set_result(
            &tickets_key,
            serde_json::to_value(&tickets).unwrap_or_default(),
        )
        .await?;

    Ok(task)
}

fn generate_task_id(tasks: &[Task]) -> String {
    let n = tasks.len() + 1;
    format!("task-{n:03}")
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
    // Reject unknown tenants so an operator cannot believe they assigned work
    // to a fleet that was never registered.
    if !crate::routes::tenants::tenant_exists(state.store(), &name).await? {
        return Err(ManagerError::NotFound(format!("no such tenant: '{name}'")));
    }
    let task = append_task(state.store(), &name, req).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "tenant": name, "task": task })),
    ))
}
